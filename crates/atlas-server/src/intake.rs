//! D41: document → in-memory extraction/redaction → terms. No cache or public prompt provenance.
use crate::{
    find::{self, Item, msg},
    routes::AppState,
};
use atlas_core::{
    Atlas, Graph,
    node::{NodeKey, NodeKind},
    search::{SearchOptions, Tier},
};
use atlas_intake::{Input, Limits, RawTerm, TermKind, terms};
use atlas_llm::Llm;
use axum::{
    Json,
    body::to_bytes,
    extract::{FromRequest, Multipart, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Instant;

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_BODY: usize = MAX_BYTES + 64 * 1024;

pub struct Error(StatusCode, &'static str, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"detail":self.2,"msg":msg(self.1,json!({}),self.2)})),
        )
            .into_response()
    }
}
fn invalid() -> Error {
    Error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "intake.error.invalid",
        "Invalid intake request.",
    )
}
fn too_large() -> Error {
    Error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "intake.error.too_large",
        "The document is larger than 5 MB.",
    )
}
fn unavailable() -> Error {
    Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "intake.error.save_unavailable",
        "Saving documents is unavailable.",
    )
}
fn sign_in() -> Error {
    Error(
        StatusCode::UNAUTHORIZED,
        "intake.error.sign_in_to_save",
        "Sign in to save a document.",
    )
}

#[derive(Default, Deserialize)]
struct Upload {
    #[serde(default)]
    #[serde(alias = "q")]
    text: String,
    llm: Option<u8>,
    #[serde(default)]
    save: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(skip)]
    file: Option<Vec<u8>>,
}

async fn upload(request: Request) -> Result<Upload, Error> {
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if content_type.starts_with("multipart/form-data") {
        let mut fields = Multipart::from_request(request, &()).await.map_err(|_| invalid())?;
        let mut out = Upload::default();
        let mut seen = std::collections::HashSet::new();
        let mut total = 0usize;
        while let Some(mut field) = fields.next_field().await.map_err(|e| {
            if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                too_large()
            } else {
                invalid()
            }
        })? {
            let name = field.name().ok_or_else(invalid)?.to_owned();
            if !seen.insert(name.clone()) {
                return Err(invalid());
            }
            let mut value = vec![];
            while let Some(chunk) = field.chunk().await.map_err(|e| {
                if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    too_large()
                } else {
                    invalid()
                }
            })? {
                total += chunk.len();
                if total > MAX_BODY || value.len() + chunk.len() > MAX_BYTES {
                    return Err(too_large());
                }
                value.extend_from_slice(&chunk);
            }
            if name == "file" {
                out.file = Some(value);
                continue;
            }
            let value = String::from_utf8(value).map_err(|_| invalid())?;
            match name.as_str() {
                "text" => out.text = value,
                "title" => out.title = value,
                "lang" => out.lang = Some(value),
                "llm" => out.llm = Some(value.parse().map_err(|_| invalid())?),
                "save" => {
                    out.save = match value.trim() {
                        "true" => true,
                        "false" | "" => false,
                        _ => return Err(invalid()),
                    }
                }
                _ => return Err(invalid()),
            }
        }
        if out.file.is_some() && !out.text.is_empty() {
            return Err(invalid());
        }
        Ok(out)
    } else if content_type.starts_with("application/json") {
        let bytes = to_bytes(request.into_body(), MAX_BODY).await.map_err(|_| too_large())?;
        serde_json::from_slice(&bytes).map_err(|_| invalid())
    } else {
        Err(Error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "intake.error.unsupported",
            "Use JSON text or a multipart PDF, DOCX or text file.",
        ))
    }
}

fn clock(t: Instant) -> u64 {
    t.elapsed().as_millis() as u64
}

pub fn validate(
    atlas: &Atlas,
    graph: &Graph,
    index: &atlas_core::search::domain::Index,
    raw: &[RawTerm],
    spans: Option<&[atlas_intake::Located]>,
) -> (Vec<Value>, Vec<Item>) {
    let mut out = vec![];
    let mut items = vec![];
    for (idx, t) in raw.iter().enumerate() {
        let kind = match t.kind {
            TermKind::Gene | TermKind::Variant => NodeKind::Gene,
            TermKind::Condition => NodeKind::Disease,
            TermKind::Phenotype => NodeKind::Phenotype,
        };
        let word = if t.kind == TermKind::Variant {
            t.gene.as_str()
        } else {
            t.text.as_str()
        };
        let mut matched = None;
        let variant = (t.kind == TermKind::Variant)
            .then(|| {
                terms::lexical(&t.text)
                    .into_iter()
                    .find(|v| v.kind == TermKind::Variant)
            })
            .flatten();
        for candidate in [word, t.english.as_str()].into_iter().filter(|s| !s.is_empty()) {
            // A variant may only resolve its declared gene; a translated condition cannot turn into a gene.
            if t.kind == TermKind::Variant && (candidate != word || variant.is_none()) {
                continue;
            }
            let direct = if kind == NodeKind::Gene {
                atlas
                    .gene(candidate)
                    .or_else(|| atlas.gene(&candidate.to_uppercase()))
                    .or_else(|| graph.gene_alias(candidate).and_then(|a| atlas.gene(&a.symbol)))
                    .map(|idx| {
                        (
                            atlas.node_ref(NodeKey { kind, idx }),
                            candidate.to_owned(),
                            "alias".to_owned(),
                        )
                    })
            } else {
                None
            };
            let exact = || {
                atlas
                    .search()
                    .search(
                        candidate,
                        SearchOptions {
                            limit: 100,
                            include_retired: false,
                        },
                    )
                    .into_iter()
                    .find(|h| h.node.kind == kind && h.tier == Tier::Exact)
                    .map(|h| {
                        (
                            atlas.node_ref(h.node),
                            h.matched.to_owned(),
                            serde_json::to_value(h.match_kind).unwrap().as_str().unwrap().to_owned(),
                        )
                    })
            };
            matched = direct.or_else(exact);
            if matched.is_some() {
                break;
            }
        }
        if matched.is_none() && matches!(t.kind, TermKind::Condition | TermKind::Phenotype) {
            let r = crate::resolve::resolve(atlas, graph, index, &t.text);
            matched = r
                .ranked
                .into_iter()
                .find(|h| h.strong && h.key.kind == kind)
                .map(|h| (h.node, h.matched, h.match_kind.into()));
        }
        let m = matched.map(|(node, matched, match_kind)| {
            items.push(find::item(
                graph,
                node.clone(),
                Some(matched.clone()),
                &match_kind,
                None,
            ));
            json!({"node":node,"matched":matched,"match_kind":match_kind})
        });
        let span = spans
            .and_then(|sp| sp.get(idx))
            .map(|s| json!({"start":s.start,"end":s.end,"quote":s.quote}));
        let optional = |s: &String| if s.is_empty() { None } else { Some(s.clone()) };
        out.push(json!({"id":format!("t{}",idx+1),"kind":t.kind,"text":t.text,"span":span,"hgvs":variant.as_ref().map(|v|v.hgvs.clone()).or_else(|| if t.kind == TermKind::Variant {None} else {optional(&t.hgvs)}),"gene":optional(&t.gene),"negated":t.negated,
            "status":if m.is_some() { "found" } else { "not_found" },"match":m}));
    }
    (out, items)
}

fn query(terms: &[Value]) -> String {
    let mut seen = std::collections::HashSet::new();
    terms
        .iter()
        .filter(|t| t["status"] == "found" && t["negated"] != true)
        .filter_map(|t| t["text"].as_str())
        .filter(|s| seen.insert(*s))
        .collect::<Vec<_>>()
        .join(" ")
}

fn explicit_connection(headers: &HeaderMap) -> bool {
    ["x-llm-connection", "x-llm-key"].iter().any(|name| {
        headers
            .get(*name)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| !v.trim().is_empty())
    })
}

fn processor_label(model: &Llm, name: &str, model_id: Option<&str>) -> Option<String> {
    let info = model.registry().get(name).ok()?.info(None);
    let provider = match name {
        "kisski" => "KISSKI",
        "ollama" => "local Ollama",
        _ => match info.kind.as_str() {
            "openai" => "OpenAI",
            "anthropic" => "Anthropic",
            "openrouter" => "OpenRouter",
            "gemini" => "Google Gemini",
            other => other,
        },
    };
    Some(format!(
        "{provider} ({})",
        model_id.or(info.default_model.as_deref()).unwrap_or("configured model")
    ))
}

fn processor_info(model: &Llm, headers: &HeaderMap) -> (String, Option<String>) {
    let call = crate::llm::call(model, headers, []);
    // Consent identifies all configured recipients the default infrastructure fallback may use.
    let mut names = vec![call.connection.clone()];
    if !explicit_connection(headers) {
        names.extend(model.default_chain());
    }
    let mut seen = std::collections::HashSet::new();
    let processors: Vec<_> = names
        .iter()
        .filter(|n| seen.insert(*n))
        .filter_map(|n| processor_label(model, n, None))
        .collect();
    let processor = if processors.is_empty() {
        None
    } else {
        Some(processors.join(" / "))
    };
    (call.connection, processor)
}

pub async fn processor(State(s): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let (connection, processor) = s
        .llm
        .llm
        .as_ref()
        .map(|l| {
            let (c, p) = processor_info(l, &headers);
            (Some(c), p)
        })
        .unwrap_or_default();
    let consent = match &processor {
        Some(p) => msg(
            "intake.consent",
            json!({"processor":p}),
            format!("Your redacted document is sent to {p} to find search terms."),
        ),
        None => msg(
            "intake.processor.lexical",
            json!({}),
            "Terms are found on this server without a model call.",
        ),
    };
    Json(json!({"connection":connection,"processor":processor,"msg":consent}))
}

pub async fn intake(State(s): State<AppState>, request: Request) -> Result<Json<Value>, Error> {
    let started = Instant::now();
    let headers = request.headers().clone();
    let u = upload(request).await?;
    if u.title.chars().count() > 200 || u.lang.as_ref().is_some_and(|l| l.len() > 20) {
        return Err(invalid());
    }
    let account = if u.save {
        let vault = atlas_accounts::DocumentVault::open(&atlas_accounts::AccountsConfig::from_env())
            .map_err(|_| unavailable())?;
        let user = vault.user(&headers).map_err(|_| unavailable())?.ok_or_else(sign_in)?;
        if !vault.can_save() {
            return Err(unavailable());
        }
        Some((vault, user.id))
    } else {
        None
    };
    let lang = u.lang.clone().unwrap_or_else(|| "en".into());
    let enabled = u.llm != Some(0);
    let p = tokio::task::spawn_blocking(move || {
        let input = u.file.as_deref().map(Input::File).unwrap_or(Input::Paste(&u.text));
        atlas_intake::prepare(input, &Limits::default()).map(|p| (p, u.title))
    })
    .await
    .map_err(|_| invalid())?
    .map_err(|e| {
        Error(
            StatusCode::from_u16(e.status()).unwrap(),
            e.key(),
            match e {
                atlas_intake::IntakeError::TooLarge => "The document is larger than 5 MB.",
                atlas_intake::IntakeError::TooManyPages(_) => "Read at most 30 pages.",
                atlas_intake::IntakeError::Unsupported => "Use PDF, DOCX or text.",
                atlas_intake::IntakeError::NoText => "No text layer was found.",
                atlas_intake::IntakeError::Empty => "The document is empty.",
                _ => "The document could not be read.",
            },
        )
    })?;
    let (p, title) = p;
    let mt = Instant::now();
    let found = crate::understand::prepared(&s, &headers, &p, &lang, enabled).await;
    let model_ms = clock(mt);
    let mut metadata = found.metadata.clone();
    if let (Some(model), Some(connection), Some(id)) =
        (&s.llm.llm, metadata["connection"].as_str(), metadata["id"].as_str())
    {
        metadata["processor"] = json!(processor_label(model, connection, Some(id)));
    }
    if metadata["status"] == "quota" {
        metadata["reason"] = msg(
            "intake.model.quota",
            json!({}),
            "The model is unavailable; terms were found locally.",
        );
    }
    let understood = found.json(&s);
    let validated = found.terms;
    let (_, items) = find::from_ranked(&s, &found.hits);
    let validate_ms = 0;
    let saved = if let Some((vault, user_id)) = account {
        Some(
            vault
                .save(
                    &user_id,
                    &atlas_accounts::DocumentMeta {
                        format: serde_json::to_value(p.format).unwrap().as_str().unwrap().into(),
                        bytes: p.bytes as u64,
                        sha256: p.sha256.clone(),
                    },
                    &atlas_accounts::DocumentContent {
                        title,
                        text: p.redacted.text.clone(),
                        terms: json!(validated),
                    },
                )
                .map_err(|_| unavailable())?,
        )
    } else {
        None
    };
    let counts = serde_json::to_value(&p.redacted.counts).unwrap();
    let items_msg: Vec<_> = counts
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, n)| {
            let label = match k.as_str() {
                "name" => "name(s)",
                "date_of_birth" => "date(s) of birth",
                "address" => "address(es)",
                "phone" => "phone number(s)",
                "email" => "email address(es)",
                "record_number" => "record number(s)",
                "insurance_number" => "insurance number(s)",
                "iban" => "IBAN(s)",
                _ => k,
            };
            msg(
                &format!("intake.redaction.kind.{k}"),
                json!({"n":n}),
                format!("Removed {n} {label}"),
            )
        })
        .collect();
    let summary = if items_msg.is_empty() {
        msg("intake.redaction.none", json!({}), "No obvious identifiers were found.")
    } else {
        msg(
            "intake.redaction.summary",
            json!({"items":items_msg}),
            format!("Removed {} obvious identifiers.", p.redacted.total()),
        )
    };
    Ok(Json(
        json!({"intake":{"sha256":p.sha256,"bytes":p.bytes,"format":p.format,"pages":p.pages,"chars":p.chars,"sent_chars":p.sent.chars().count(),"truncated":p.truncated},
        "redaction":{"total":p.redacted.total(),"counts":counts,"summary":summary},"query":query(&validated),"terms":validated,
        "checked":find::checked::build(s.atlas(),&s.graph,&items,"document"),"sent_text":p.sent,"model":metadata,
        "understood":understood,"query_execution":found.query_execution,"results":found.hits,"timing_ms":{"extract":p.extract_ms,"redact":p.redact_ms,"model":model_ms,"validate":validate_ms,"total":clock(started)},"saved":saved,
        "notice":msg("intake.notice.not_diagnosis",json!({}),"Research and connection information; not a diagnosis or treatment recommendation.")}),
    ))
}

#[derive(Deserialize)]
pub struct Edited {
    terms: Vec<RawTerm>,
}
pub async fn edited(
    State(s): State<AppState>,
    body: Result<Json<Edited>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Value>, Error> {
    let Json(p) = body.map_err(|_| invalid())?;
    if p.terms.len() > 50
        || p.terms.iter().any(|t| {
            t.text.trim().is_empty()
                || t.text.chars().count() > 200
                || t.english.chars().count() > 200
                || t.gene.len() > 100
                || t.hgvs.len() > 200
        })
    {
        return Err(invalid());
    }
    let (terms, items) = validate(s.atlas(), &s.graph, &s.search, &p.terms, None);
    Ok(Json(
        json!({"query":query(&terms),"terms":terms,"checked":find::checked::build(s.atlas(),&s.graph,&items,"document")}),
    ))
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
