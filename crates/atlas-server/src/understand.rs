//! One D56 owner for search, resolution and document intake. The model extracts mentions
//! and intent; only the domain index supplies graph identities. Private input stays in memory.
use crate::{routes::AppState, search};
use atlas_core::{
    node::NodeKind,
    search::{
        SearchOptions,
        domain::{Hit, Reason},
        messages::Message as Copy,
    },
};
use atlas_intake::{Prepared, RawTerm, TermKind, terms};
use atlas_llm::{Cache, CacheMode, CompletionRequest, JsonSchema, Llm, Message};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

pub const METHOD: &str = "atlas-understand-v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    #[default]
    Search,
    FindGroup,
    FindStudy,
    FindModelSample,
    Explain,
    Connect,
}

#[derive(Deserialize)]
struct Extracted {
    terms: Vec<RawTerm>,
    #[serde(default)]
    intent: Intent,
    references: Vec<Reference>,
}

#[derive(Deserialize)]
struct Reference {
    kind: NodeKind,
    text: String,
    english: String,
    negated: bool,
}

pub struct Understood {
    pub hits: Vec<Hit>,
    pub terms: Vec<Value>,
    pub metadata: Value,
    pub intent: Intent,
    pub present: Vec<u32>,
    pub excluded: Vec<u32>,
    pub focus: Vec<String>,
    pub input: Value,
    pub query_execution: Value,
}

impl Understood {
    pub fn json(&self, s: &AppState) -> Value {
        let present: Vec<_> = self.present.iter().map(|&t| s.atlas().hpo.term(t).id.clone()).collect();
        let excluded: Vec<_> = self
            .excluded
            .iter()
            .map(|&t| s.atlas().hpo.term(t).id.clone())
            .collect();
        json!({"method":METHOD,"input":self.input,"model":self.metadata,
            "intent":self.intent,"entities":self.terms,"focus_ids":self.focus,
            "present":present,"excluded":excluded,
            "plan_input":{"focus_ids":self.focus,"intent":self.intent,
                "present":present,"excluded":excluded}})
    }
}

fn model_needed(p: &Prepared, lang: &str, hits: &[Hit]) -> bool {
    let clear: HashSet<_> = hits.iter().filter(|h| h.strong).map(|h| h.key).collect();
    p.format != atlas_intake::Format::Paste
        || p.truncated
        || p.sent.len() > 512
        || lang != "en"
        || p.sent.trim().contains(['\n', '?'])
        || clear.len() != 1
        || hits
            .iter()
            .any(|h| matches!(h.reason, Reason::TranslatedName | Reason::TranslatedAlias))
}

fn request(text: &str, lang: &str) -> CompletionRequest {
    let mut schema = terms::schema().schema;
    schema["required"].as_array_mut().unwrap().push(json!("intent"));
    schema["required"].as_array_mut().unwrap().push(json!("references"));
    schema["properties"]["intent"] = json!({"type":"string","enum":
        ["search","find_group","find_study","find_model_sample","explain","connect"]});
    schema["properties"]["references"] = json!({"type":"array","maxItems":40,"items":{
        "type":"object","additionalProperties":false,"required":["kind","text","english","negated"],
        "properties":{"kind":{"type":"string","enum":["organisation","study","paper","grant","person","asset","pathway"]},
            "text":{"type":"string"},"english":{"type":"string"},"negated":{"type":"boolean"}}}});
    CompletionRequest::new(vec![
        Message::system(include_str!("understand.prompt.txt")),
        Message::user(json!({"language":lang,"text":text}).to_string()),
    ])
    .with_schema(JsonSchema::new("atlas_understand", schema))
    .with_max_tokens(2500)
}

fn kind(t: TermKind) -> NodeKind {
    match t {
        TermKind::Gene | TermKind::Variant => NodeKind::Gene,
        TermKind::Condition => NodeKind::Disease,
        TermKind::Phenotype => NodeKind::Phenotype,
    }
}

/// Lexical fallback uses the already indexed mentions and intake's variant parser. Unknown
/// model terms stay visible as unresolved; untranslated clinical phrases are never fabricated.
fn lexical(text: &str, hits: &[Hit]) -> Vec<RawTerm> {
    let mut raw = terms::lexical(text);
    for hit in hits.iter().filter(|h| h.strong || h.reason == Reason::Mention) {
        let kind = match hit.key.kind {
            NodeKind::Gene => TermKind::Gene,
            NodeKind::Disease => TermKind::Condition,
            NodeKind::Phenotype => TermKind::Phenotype,
            _ => continue,
        };
        raw.push(RawTerm {
            kind,
            text: hit.matched.clone(),
            english: String::new(),
            gene: String::new(),
            hgvs: String::new(),
            negated: false,
        });
    }
    raw
}

/// Redaction happened in atlas-intake::prepare before this function. Each endpoint calls
/// this once. No prompt/reply is registered in the public provenance endpoint or disk cache.
pub async fn prepared(s: &AppState, headers: &HeaderMap, p: &Prepared, lang: &str, enabled: bool) -> Understood {
    prepared_options(s, headers, p, lang, enabled, false).await
}

async fn prepared_options(
    s: &AppState,
    headers: &HeaderMap,
    p: &Prepared,
    lang: &str,
    enabled: bool,
    include_retired: bool,
) -> Understood {
    let context = s.search.lexical_context(
        s.atlas(),
        &s.graph,
        &p.sent,
        SearchOptions {
            limit: 100,
            include_retired,
        },
    );
    let mut hits = search::from_context(
        s,
        context.clone(),
        SearchOptions {
            limit: 100,
            include_retired,
        },
    );
    let needed = model_needed(p, lang, &hits);
    let mut raw = None;
    let mut references: Vec<_> = hits
        .iter()
        .filter(|h| {
            (h.strong || h.reason == Reason::Mention)
                && !matches!(h.key.kind, NodeKind::Phenotype | NodeKind::Gene | NodeKind::Disease)
        })
        .map(|h| Reference {
            kind: h.key.kind,
            text: h.matched.clone(),
            english: String::new(),
            negated: false,
        })
        .collect();
    let mut intent = Intent::Search;
    let mut metadata = json!({"used":false,"origin":"lexical","status":"clear_entity",
        "connection":null,"id":null,"activities":[]});
    if needed {
        metadata["status"] = json!(if enabled { "unavailable" } else { "disabled" });
        metadata["reason"] =
            Copy::new(if enabled { "model_unavailable" } else { "model_disabled" }, json!({})).rendered(lang);
    }
    if needed
        && enabled
        && let Some(model) = &s.llm.llm
    {
        let private = Llm::new(
            model.registry().clone(),
            Cache::new(model.cache().dir(), CacheMode::Off),
        )
        .with_fallbacks(model.default_chain());
        let call = crate::llm::call(&private, headers, []);
        let explicit = ["x-llm-connection", "x-llm-key"].iter().any(|name| {
            headers
                .get(*name)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| !v.trim().is_empty())
        });
        let result = if explicit {
            private.complete_json::<Extracted>(&call, request(&p.sent, lang)).await
        } else {
            private
                .complete_default_json::<Extracted>(call.visitor.as_deref(), &[], request(&p.sent, lang))
                .await
        };
        match result {
            Ok(done) => {
                intent = done.value.intent;
                raw = Some(done.value.terms);
                references = done.value.references;
                let last = done.calls.last().expect("completed call");
                metadata = json!({"used":true,"origin":"llm","status":"understood",
                    "connection":last.response.connection,"id":last.response.requested_model,
                    "latency_ms":done.calls.iter().map(|c| c.response.latency_ms).sum::<u64>(),
                    "cost_usd":done.calls.iter().filter_map(|c|c.response.cost_usd).reduce(|a,b|a+b),
                    "activities":done.calls.iter().map(|c| &c.provenance).collect::<Vec<_>>()});
            }
            Err(e) => {
                metadata["status"] = json!(if matches!(e, atlas_llm::LlmError::FreeQuota { .. }) {
                    "quota"
                } else {
                    "unavailable"
                })
            }
        }
    }
    let raw = raw.unwrap_or_else(|| {
        let mut raw = lexical(&p.sent, &hits);
        for (&idx, negated) in context
            .present
            .iter()
            .map(|t| (t, false))
            .chain(context.excluded.iter().map(|t| (t, true)))
        {
            raw.push(RawTerm {
                kind: TermKind::Phenotype,
                text: s.atlas().hpo.term(idx).name.clone(),
                english: String::new(),
                gene: String::new(),
                hgvs: String::new(),
                negated,
            });
        }
        raw
    });
    let raw = raw
        .into_iter()
        .take(terms::MAX_TERMS)
        .filter(|t| t.text.chars().count() <= 200 && t.english.chars().count() <= 200 && t.gene.len() <= 100)
        .collect();
    let (located, rejected) = terms::locate(&p.sent, raw);
    let mut raw: Vec<_> = located.iter().map(|l| l.term.clone()).collect();
    // The shared lexical context supplies absence when the model is disabled/unavailable.
    if metadata["used"] != true {
        for t in &mut raw {
            if t.kind == TermKind::Phenotype {
                let idx = s.atlas().hpo.canonical(&t.text).or_else(|| {
                    s.search
                        .suggest(
                            s.atlas(),
                            &s.graph,
                            &t.text,
                            SearchOptions {
                                limit: 8,
                                include_retired,
                            },
                        )
                        .into_iter()
                        .find(|h| h.strong && h.key.kind == NodeKind::Phenotype)
                        .map(|h| h.key.idx)
                });
                if idx.is_some_and(|idx| context.excluded.contains(&idx)) {
                    t.negated = true;
                }
            }
        }
    }
    // A model cannot attach an absent gene to a real variant mention.
    for t in &mut raw {
        if t.kind == TermKind::Variant
            && (t.gene.is_empty()
                || !p
                    .sent
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .any(|word| word.eq_ignore_ascii_case(&t.gene)))
        {
            t.gene.clear();
        }
    }
    let mut entities = vec![];
    let model_used = metadata["used"] == true;
    if model_used {
        hits.clear();
    }
    let mut focus = vec![];
    let mut present = vec![];
    let mut excluded = vec![];
    let mut seen_hits: HashSet<_> = hits.iter().map(|h| h.key).collect();
    for (idx, t) in raw.iter().enumerate() {
        let mut choices = vec![];
        let variant = (t.kind == TermKind::Variant)
            .then(|| {
                terms::lexical(&t.text)
                    .into_iter()
                    .find(|v| v.kind == TermKind::Variant)
            })
            .flatten();
        let word = if t.kind == TermKind::Variant {
            if variant.is_some() { t.gene.as_str() } else { "" }
        } else {
            &t.text
        };
        let english = if matches!(t.kind, TermKind::Condition | TermKind::Phenotype) {
            t.english.as_str()
        } else {
            ""
        };
        for word in [word, english].into_iter().filter(|w| !w.is_empty()) {
            for mut h in s.search.suggest(
                s.atlas(),
                &s.graph,
                word,
                SearchOptions {
                    limit: 8,
                    include_retired,
                },
            ) {
                if h.key.kind != kind(t.kind) {
                    continue;
                }
                if let Some(existing) = choices.iter_mut().find(|x: &&mut Hit| x.key == h.key) {
                    if h.strong && !existing.strong {
                        h.localise(lang);
                        *existing = h;
                    }
                    continue;
                }
                // Source-derived label equality is the only automatic linker. Fuzzy candidates
                // remain editable choices. Model-extracted entities never establish a diagnosis.
                h.localise(lang);
                choices.push(h);
            }
            if choices.iter().any(|h| h.strong) {
                break;
            }
        }
        let exact: Vec<_> = choices.iter().filter(|h| h.strong).collect();
        let selected = if exact.len() == 1 { Some(exact[0]) } else { None };
        if let Some(h) = selected {
            if h.key.kind == NodeKind::Phenotype {
                if t.negated {
                    excluded.push(h.key.idx)
                } else {
                    present.push(h.key.idx)
                }
            } else if !t.negated {
                focus.push(h.node.id.clone());
            }
        }
        let status = if selected.is_some() {
            "found"
        } else if exact.len() > 1 {
            "ambiguous"
        } else {
            "not_found"
        };
        let span = &located[idx];
        entities.push(json!({"id":format!("t{}",idx+1),"kind":t.kind,"text":t.text,"english":english,"negated":t.negated,
            "gene":if t.gene.is_empty(){None}else{Some(&t.gene)},"hgvs":variant.as_ref().map(|v|&v.hgvs),
            "status":status,"match":selected.map(|h|json!({"node":h.node,"matched":h.matched,"match_kind":h.match_kind})),
            "choices":choices,"span":{"start":span.start,"end":span.end,"quote":span.quote}}));
        if !t.negated {
            for mut h in choices {
                if model_used {
                    h.strong = false;
                }
                if seen_hits.insert(h.key) {
                    hits.push(h);
                }
            }
        }
    }
    for reference in references
        .into_iter()
        .take(40)
        .filter(|r| r.text.chars().count() <= 200 && r.english.chars().count() <= 200)
    {
        let proxy = RawTerm {
            kind: TermKind::Condition,
            text: reference.text.clone(),
            english: String::new(),
            gene: String::new(),
            hgvs: String::new(),
            negated: reference.negated,
        };
        let (spans, _) = terms::locate(&p.sent, vec![proxy]);
        let Some(span) = spans.first() else {
            continue;
        };
        let mut choices = vec![];
        for word in [&reference.text, &reference.english]
            .into_iter()
            .filter(|w| !w.is_empty())
        {
            for mut h in s.search.suggest(
                s.atlas(),
                &s.graph,
                word,
                SearchOptions {
                    limit: 8,
                    include_retired,
                },
            ) {
                if h.key.kind != reference.kind {
                    continue;
                }
                h.localise(lang);
                if let Some(existing) = choices.iter_mut().find(|x: &&mut Hit| x.key == h.key) {
                    if h.strong && !existing.strong {
                        *existing = h;
                    }
                } else {
                    choices.push(h);
                }
            }
            if choices.iter().any(|h| h.strong) {
                break;
            }
        }
        let exact: Vec<_> = choices.iter().filter(|h| h.strong).collect();
        let selected = if exact.len() == 1 { Some(exact[0]) } else { None };
        if let Some(h) = selected
            && !reference.negated
        {
            focus.push(h.node.id.clone());
        }
        entities.push(json!({"id":format!("g{}",entities.len()+1),"kind":reference.kind,"text":reference.text,"english":reference.english,
            "negated":reference.negated,"status":if selected.is_some(){"found"}else if exact.len()>1{"ambiguous"}else{"not_found"},
            "match":selected.map(|h|json!({"node":h.node,"matched":h.matched,"match_kind":h.match_kind})),
            "span":{"start":span.start,"end":span.end,"quote":span.quote},"choices":choices}));
        if !reference.negated {
            for mut h in choices {
                if model_used {
                    h.strong = false;
                }
                if seen_hits.insert(h.key) {
                    hits.push(h);
                }
            }
        }
    }
    present.sort_unstable();
    present.dedup();
    excluded.sort_unstable();
    excluded.dedup();
    present.retain(|p| !excluded.contains(p));
    focus.sort();
    focus.dedup();
    // Use positive linked IDs as graph seeds. This retrieves their actual connections,
    // rather than letting model-authored names become facts. Raw symptom ranking is discarded
    // after successful extraction, then recomputed from the present/absent profile.
    for id in &focus {
        for mut h in search::ranked(
            s,
            id,
            SearchOptions {
                limit: 100,
                include_retired,
            },
        ) {
            if model_used {
                h.strong = false;
            }
            if seen_hits.insert(h.key) {
                hits.push(h);
            }
        }
    }
    hits.retain(|h| {
        !(h.reason == Reason::Phenotypes || (h.key.kind == NodeKind::Phenotype && excluded.contains(&h.key.idx)))
    });
    // Explicitly absent symptoms cannot enter the positive fusion profile.
    if !present.is_empty() {
        let query = atlas_core::search::domain::QueryMatches {
            hits,
            present: present.clone(),
            excluded: excluded.clone(),
        };
        hits = search::with_phenotypes(
            s,
            query,
            SearchOptions {
                limit: 100,
                include_retired,
            },
        );
    }
    for h in &mut hits {
        h.localise(lang);
    }
    metadata["rejected_mentions"] = json!(rejected);
    if metadata["used"] != true && !needed {
        metadata["reason"] = Value::Null;
    }
    // Semantic entities and inferred research leads have different confidence. Direct
    // linked choices lead; keep the existing ranker's internal order for graph expansions.
    hits.sort_by_key(|h| {
        let wanted = match intent {
            Intent::FindGroup => {
                h.key.kind == NodeKind::Organisation
                    && matches!(
                        s.graph.org(h.key.idx).kind,
                        atlas_core::graph::OrgKind::PatientGroup | atlas_core::graph::OrgKind::ExpertCentre
                    )
            }
            Intent::FindStudy => h.key.kind == NodeKind::Study,
            Intent::FindModelSample => h.key.kind == NodeKind::Asset,
            Intent::Connect => matches!(h.key.kind, NodeKind::Person | NodeKind::Organisation),
            Intent::Search | Intent::Explain => false,
        };
        (!wanted, !(focus.contains(&h.node.id) || h.strong))
    });
    let input = json!({"sha256":p.sha256,"bytes":p.bytes,"format":p.format,"truncated":p.truncated,
        "redactions":p.redacted.counts,"redacted_total":p.redacted.total()});
    let mut found = Understood {
        hits,
        terms: entities,
        metadata,
        intent,
        present,
        excluded,
        focus,
        input,
        query_execution: Value::Null,
    };
    found.query_execution = crate::query_execution::execute(s, headers, p, &found, lang, enabled).await;
    found
}

pub async fn text(
    s: &AppState,
    headers: &HeaderMap,
    text: String,
    lang: &str,
    enabled: bool,
) -> Result<Understood, crate::routes::ApiError> {
    text_options(s, headers, text, lang, enabled, false).await
}

pub async fn text_options(
    s: &AppState,
    headers: &HeaderMap,
    text: String,
    lang: &str,
    enabled: bool,
    include_retired: bool,
) -> Result<Understood, crate::routes::ApiError> {
    if lang.len() > 20 {
        return Err(crate::routes::ApiError(
            axum::http::StatusCode::BAD_REQUEST,
            "search.error.invalid".into(),
        ));
    }
    let p = tokio::task::spawn_blocking(move || {
        atlas_intake::prepare(atlas_intake::Input::Paste(&text), &atlas_intake::Limits::default())
    })
    .await
    .map_err(crate::routes::internal)?
    .map_err(|e| crate::routes::ApiError(axum::http::StatusCode::from_u16(e.status()).unwrap(), e.key().into()))?;
    Ok(prepared_options(s, headers, &p, lang, enabled, include_retired).await)
}

#[cfg(test)]
#[path = "understand_tests.rs"]
mod tests;
