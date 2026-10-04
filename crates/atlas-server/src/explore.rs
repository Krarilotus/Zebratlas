//! Canonical, schema-grounded natural-language queries pass the shared algebra
//! guard before bounded nrese execution. Exact symbols and edited neighborhood
//! plans use a deterministic, bounded adjacency compiler over the same store.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

use atlas_core::graph::{GraphEdge, RecordWithhold, Relation};
use atlas_core::node::{Edge, NodeKey, NodeKind, NodeRef};
use atlas_core::provenance::RecordRef;
use atlas_core::search::SearchOptions;
use atlas_core::text::normalize_label;
use atlas_core::{Atlas, Graph};
use atlas_llm::JsonSchema;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::routes::{ApiError, ApiResult, AppState, internal};
use crate::{explore_sparql, nodes, views};

const MAX_QUERY: usize = 32_000;
const MAX_NODES: usize = 100;
const MAX_EDGES: usize = 160;
const MAX_WORK: usize = 8_000;
static GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

fn explore_error(status: StatusCode, message: impl Into<String>) -> ApiError {
    ApiError(
        status,
        json!({"key":"explore.error","fallback":message.into(),"params":{}}),
    )
}

fn model_error(error: String) -> ApiError {
    let lower = error.to_ascii_lowercase();
    if lower.contains("model_rate_limited") || lower.contains("rate limited") {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            json!({"key":"model_rate_limited","code":"model_rate_limited",
            "fallback":"The selected model has reached its request limit. Search names or choose another available model.","params":{}}),
        )
    } else {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"key":"model_unavailable","code":"model_unavailable",
            "fallback":"The model could not complete this query. Name search and the graph are still available.","params":{}}),
        )
    }
}

fn routing_error(error: crate::explore_failover::RoutingFailure) -> ApiError {
    let mut public = if error.code == "unsupported_question" {
        ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"key":"unsupported_question","code":"unsupported_question","fallback":"No reliable gene or disease scope was found. Choose a known name to run a bounded source lookup.","params":{}}),
        )
    } else {
        model_error(error.code.to_owned())
    };
    public.1["params"] = error.diagnostics;
    public
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    #[default]
    All,
    Conditions,
    Researchers,
    Studies,
    Papers,
    Funding,
    Models,
    Therapies,
    Resources,
    Groups,
    Outcomes,
    Gaps,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filters {
    pub country: Option<String>,
    pub recruiting: Option<bool>,
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub focus: Vec<String>,
    #[serde(default)]
    pub intent: Intent,
    #[serde(default)]
    pub filters: Filters,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExploreRequest {
    #[serde(default)]
    pub query: String,
    pub limit: Option<usize>,
    pub mode: Option<String>,
    /// A verified edited plan reruns without a model call.
    pub plan: Option<Plan>,
}

#[derive(Deserialize)]
pub struct CommunityParams {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupRequest {
    pub query: String,
    pub limit: Option<usize>,
}

/// Explicit model-free name search. Approximate matches are offered for the user
/// to select; they never become a guessed interpretation of a scientific question.
pub async fn lookup(State(state): State<AppState>, Json(request): Json<LookupRequest>) -> ApiResult {
    let query = request.query.trim().to_owned();
    let limit = request.limit.unwrap_or(5);
    if query.is_empty() || query.len() > 512 || !(1..=10).contains(&limit) {
        return Err(explore_error(
            StatusCode::BAD_REQUEST,
            "Enter a name of up to 512 bytes and a limit from 1 to 10",
        ));
    }
    let shown_query = query.clone();
    let matches = tokio::task::spawn_blocking(move || {
        let index = state.explore_index.get_or_init(|| ConnectedIndex::new(&state.graph));
        let mut matches = BTreeMap::new();
        for node in link(&state.atlas, &state.graph, index, &query) {
            matches.insert(
                node.id.clone(),
                json!({"id":node.id,"label":node.label,"kind":node.kind,"match":"exact"}),
            );
        }
        if matches.is_empty() {
            for hit in state.atlas.search().search(
                &query,
                SearchOptions {
                    limit,
                    include_retired: false,
                },
            ) {
                let node = state.atlas.node_ref(hit.node);
                if available(&state.atlas, &state.graph, &node.id).is_some() {
                    matches
                        .entry(node.id.clone())
                        .or_insert_with(|| json!({"id":node.id,"label":node.label,"kind":node.kind,"match":"fuzzy"}));
                }
            }
            if matches.is_empty()
                && let Some(corrected) = state.atlas.search().correct(&query)
            {
                for node in link(&state.atlas, &state.graph, index, &corrected) {
                    matches.insert(
                        node.id.clone(),
                        json!({"id":node.id,"label":node.label,"kind":node.kind,"match":"fuzzy"}),
                    );
                }
            }
        }
        matches.into_values().take(limit).collect::<Vec<_>>()
    })
    .await
    .map_err(internal)?;
    Ok(Json(
        json!({"query":shown_query,"mode":"indexed_name_lookup","model_calls":0,"engine":"indexed-atlas","matches":matches}),
    ))
}

/// A compact binary-search index for connected-layer labels. Built once off the async worker.
#[derive(Default)]
pub struct ConnectedIndex {
    labels: Vec<(Box<str>, NodeKey)>,
    countries: BTreeSet<String>,
}

impl ConnectedIndex {
    fn new(graph: &Graph) -> Self {
        let mut labels = Vec::new();
        for kind in [
            NodeKind::Person,
            NodeKind::Organisation,
            NodeKind::Asset,
            NodeKind::Study,
            NodeKind::Grant,
            NodeKind::Paper,
        ] {
            for idx in 0..graph.node_count(kind) as u32 {
                let key = NodeKey { kind, idx };
                // Withheld labels must never reach linking or a planning prompt.
                if graph.node_withheld(key).is_none() {
                    labels.push((normalize_label(&graph.node_ref(key).label).into_boxed_str(), key));
                }
            }
        }
        labels.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let countries = graph
            .studies()
            .iter()
            .flat_map(|study| study.countries.iter().cloned())
            .chain(graph.data().orgs.iter().filter_map(|org| org.country.clone()))
            .collect();
        Self { labels, countries }
    }

    fn exact(&self, text: &str) -> impl Iterator<Item = NodeKey> + '_ {
        let q = normalize_label(text);
        let lo = self.labels.partition_point(|(label, _)| label.as_ref() < q.as_str());
        let hi = self.labels.partition_point(|(label, _)| label.as_ref() <= q.as_str());
        self.labels[lo..hi].iter().take(8).map(|(_, key)| *key)
    }
}

fn available(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeRef> {
    if let Some(key) = graph.node(id) {
        if graph.node_withheld(key).is_some() {
            return None;
        }
    }
    nodes::any_ref(atlas, graph, id)
}

fn link(atlas: &Atlas, graph: &Graph, index: &ConnectedIndex, text: &str) -> Vec<NodeRef> {
    let mut found = BTreeMap::new();
    if let Some(node) = available(atlas, graph, text) {
        found.insert(node.id.clone(), node);
    }
    for hit in atlas.search().exact(text, 8) {
        let node = atlas.node_ref(hit.node);
        found.insert(node.id.clone(), node);
    }
    for alias in graph.genes_by_alias(text).take(8) {
        if let Some(gene) = atlas.gene(&alias.symbol) {
            let node = atlas.node_ref(NodeKey {
                kind: NodeKind::Gene,
                idx: gene,
            });
            found.insert(node.id.clone(), node);
        }
    }
    // Gene compounds in letters (e.g. STXBP1-Mutation) retain their linked symbol.
    if !text.contains(char::is_whitespace) {
        for part in text.split('-') {
            if let Some(idx) = atlas.gene(&part.to_ascii_uppercase()) {
                let node = atlas.node_ref(NodeKey {
                    kind: NodeKind::Gene,
                    idx,
                });
                found.insert(node.id.clone(), node);
            }
        }
    }
    // Exact atlas entities outrank case-folded organism/model labels. Orthologs belong
    // in the result graph, rather than becoming extra interpretations of a human gene.
    if found
        .values()
        .any(|node| matches!(node.kind, NodeKind::Gene | NodeKind::Disease | NodeKind::Phenotype))
    {
        return found.into_values().collect();
    }
    for key in index.exact(text) {
        let id = graph.node_ref(key).id;
        if let Some(node) = available(atlas, graph, &id) {
            found.insert(id, node);
        }
    }
    found.into_values().collect()
}

fn local_understanding(
    atlas: &Atlas,
    graph: &Graph,
    index: &ConnectedIndex,
    query: &str,
) -> (Vec<NodeRef>, Intent, Option<String>) {
    let mut found = BTreeMap::new();
    for node in link(atlas, graph, index, query) {
        found.insert(node.id.clone(), node);
    }
    let words: Vec<_> = query
        .split_whitespace()
        .take(256)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-'))
        .collect();
    if found.is_empty() || words.len() > 6 {
        // Longest exact phrases first; bounded ngrams, no substring/token scans.
        let mut covered = vec![false; words.len()];
        for n in (1..=5.min(words.len())).rev() {
            for (i, phrase) in words.windows(n).enumerate() {
                if i > 0
                    && matches!(
                        words[i - 1].to_ascii_lowercase().as_str(),
                        "no" | "without" | "denies" | "kein" | "keine"
                    )
                {
                    continue;
                }
                let phrase = phrase.join(" ");
                if phrase.len() < 3 {
                    continue;
                }
                // Gene/disease acronyms such as CAN and HAS overlap normal speech.
                // A lone symbol search remains valid; prose stopwords do not become
                // additional scientific entities merely because an alias matches.
                if n == 1 && words.len() > 1 && ordinary_word(&phrase) {
                    continue;
                }
                // A disease name's internal words are not additional entities.
                // HGNC's genuine TYPE alias, for example, does not make SGCG a
                // target of "Niemann-Pick disease type C". Explicit approved
                // symbols and CURIEs remain available inside a matched name.
                if covered[i..i + n].iter().all(|v| *v)
                    && atlas.gene(&phrase.to_ascii_uppercase()).is_none()
                    && available(atlas, graph, &phrase).is_none()
                {
                    continue;
                }
                let matches = link(atlas, graph, index, &phrase);
                if !matches.is_empty() {
                    covered[i..i + n].fill(true);
                }
                for node in matches {
                    found.insert(node.id.clone(), node);
                }
                if found.len() >= 8 {
                    break;
                }
            }
            if found.len() >= 8 {
                break;
            }
        }
    }
    let mut suggestion = None;
    if found.is_empty() && query.len() <= 96 && words.len() <= 8 {
        // Only short queries need autocomplete and typo correction.
        for hit in atlas.search().search(
            query,
            SearchOptions {
                limit: 3,
                include_retired: false,
            },
        ) {
            let node = atlas.node_ref(hit.node);
            found.insert(node.id.clone(), node);
        }
        if found.is_empty() {
            suggestion = atlas.search().correct(query);
            if let Some(corrected) = &suggestion {
                for node in link(atlas, graph, index, corrected) {
                    found.insert(node.id.clone(), node);
                }
            }
        }
    }
    let q = query.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| q.contains(w));
    let intent = if any(&["researcher", "scientist", "expert", "forscher"]) {
        Intent::Researchers
    } else if any(&["trial", "study", "studies", "recruit", "studie"]) {
        Intent::Studies
    } else if any(&["paper", "publication", "article", "pubmed"]) {
        Intent::Papers
    } else if any(&["funding", "grant", "funder", "finanzierung"]) {
        Intent::Funding
    } else if any(&["model", "cell line", "sample", "biobank"]) {
        Intent::Models
    } else if any(&["therapy", "therapies", "treatment", "drug", "therapie"]) {
        Intent::Therapies
    } else if any(&["group", "foundation", "association", "families", "stiftung"]) {
        Intent::Groups
    } else if any(&["outcome", "measure", "endpoint"]) {
        Intent::Outcomes
    } else if any(&["registry", "dataset", "resource", "register"]) {
        Intent::Resources
    } else if any(&["missing", "gap", "lücke"]) {
        Intent::Gaps
    } else {
        Intent::All
    };
    (found.into_values().take(8).collect(), intent, suggestion)
}

fn ordinary_word(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "can"
            | "has"
            | "had"
            | "have"
            | "and"
            | "are"
            | "was"
            | "were"
            | "will"
            | "may"
            | "not"
            | "the"
            | "for"
            | "from"
            | "with"
            | "which"
            | "who"
            | "how"
            | "our"
            | "your"
            | "you"
            | "all"
            | "any"
            | "but"
            | "then"
            | "that"
            | "this"
            | "there"
            | "other"
            | "both"
            | "her"
            | "his"
            | "its"
            | "only"
            | "some"
            | "type"
            | "rank"
            | "und"
            | "hat"
            | "ist"
            | "sind"
            | "war"
            | "wir"
            | "wie"
            | "der"
            | "die"
            | "das"
            | "den"
            | "dem"
            | "ein"
            | "eine"
            | "einen"
            | "mit"
            | "von"
            | "aus"
            | "bei"
            | "auch"
            | "nur"
            | "noch"
            | "nicht"
    )
}

fn understanding_schema() -> JsonSchema {
    JsonSchema::new(
        "zebratlas_query_plan",
        json!({"type":"object","additionalProperties":false,
        "required":["terms","excluded_terms","intent","filters"],"properties":{
        "terms":{"type":"array","maxItems":8,"items":{"type":"string","maxLength":120}},
        "excluded_terms":{"type":"array","maxItems":8,"items":{"type":"string","maxLength":120}},
        "intent":{"enum":["all","conditions","researchers","studies","papers","funding","models","therapies","resources","groups","outcomes","gaps"]},
        "filters":{"type":"object","additionalProperties":false,"required":["country","recruiting","kind"],"properties":{
        "country":{"type":["string","null"],"maxLength":100},"recruiting":{"type":["boolean","null"]},"kind":{"type":["string","null"],"maxLength":100}}}}}),
    )
}

const MODEL_SKILL: &str = include_str!("explore-model-skill.md");

/// Operator-owned artifact lineage, never client/model configuration or a claim that
/// a differently configured endpoint has been independently verified by this request.
fn dataset_lineage() -> Value {
    static DATASET: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    DATASET
        .get_or_init(|| {
            let Some(path) = std::env::var_os("ATLAS_OPERATIONAL_MANIFEST") else {
                return Value::Null;
            };
            use std::io::Read;
            let Ok(file) = std::fs::File::open(path) else {
                return Value::Null;
            };
            let mut bytes = Vec::new();
            if file.take(256 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 256 * 1024 {
                return Value::Null;
            }
            let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) else {
                return Value::Null;
            };
            if manifest["public_release"] != false || manifest["runtime_reasoning"] != false {
                return Value::Null;
            }
            let valid_hash = |key: &str| {
                manifest[key]
                    .as_str()
                    .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
            };
            if !valid_hash("rdf_sha256") || !valid_hash("graph_sha256") || !valid_hash("atlas_sha256") {
                return Value::Null;
            }
            let reasoning = std::env::var_os("ATLAS_REASONING_MANIFEST").and_then(|path| {
                let file = std::fs::File::open(path).ok()?;
                let mut bytes = Vec::new();
                file.take(256 * 1024 + 1).read_to_end(&mut bytes).ok()?;
                if bytes.len() > 256 * 1024 { return None; }
                let p:Value = serde_json::from_slice(&bytes).ok()?;
                if p["runtime_reasoning"] != true || p["source_rdf_sha256"] != manifest["rdf_sha256"] { return None; }
                Some(json!({"engine":p["engine"],"mode":p["mode"],"rule":p["rule"],"rules_sha256":p["rules_sha256"],
                    "asserted_quads":p["asserted_quads"],"derived_triples":p["derived_quads"],"proofs_required":true,
                    "scope":"Source ontology hierarchy transitivity; no clinical, eligibility or gene-association inference"}))
            });
            json!({"kind":"repo_graph_operational_adapter","visibility":manifest["visibility"],
            "rdf_sha256":manifest["rdf_sha256"],"graph_snapshot_sha256":manifest["graph_sha256"],
            "atlas_snapshot_sha256":manifest["atlas_sha256"],"coverage":manifest["report"],
            "identity_gate_sha256":manifest["identity_gate_sha256"],"ontology":manifest["ontology"],
            "public_release":false,"runtime_reasoning_available":reasoning.is_some(),"reasoning":reasoning,
            "configuration_source":"operator_manifest"})
        })
        .clone()
}

/// Query jobs constrain actual predicates; filters are applied to hydrated source metadata.
fn relation_scope(intent: Intent) -> Vec<String> {
    let mut relations: Vec<&str> = match intent {
        Intent::Gaps => return vec![],
        Intent::All => vec![
            "gene_associated_with_condition",
            "about_gene",
            "about_condition",
            "author_of",
            "principal_investigator_of",
            "studies_condition",
            "names_gene",
            "serves_condition",
            "serves_gene",
            "model_of",
            "resource_for",
            "studied_for",
            "targets",
        ],
        Intent::Conditions => vec!["gene_associated_with_condition", "has_phenotype"],
        Intent::Researchers => vec![
            "about_gene",
            "about_condition",
            "author_of",
            "principal_investigator_of",
            "awarded_to",
        ],
        Intent::Studies => vec!["studies_condition", "names_gene", "sponsored_by"],
        Intent::Papers => vec!["about_gene", "about_condition", "claims_about", "author_of"],
        Intent::Funding => vec![
            "about_gene",
            "about_condition",
            "funds",
            "principal_investigator_of",
            "awarded_to",
            "held_by",
        ],
        Intent::Models => vec!["model_of", "resource_for", "orthologous_to", "held_by"],
        Intent::Therapies => vec!["studied_for", "targets", "held_by"],
        Intent::Resources | Intent::Outcomes => vec!["resource_for", "held_by"],
        Intent::Groups => vec!["serves_condition", "serves_gene"],
    };
    relations.push(views::GENE_RELATION);
    relations.into_iter().map(str::to_owned).collect()
}

fn local_filters(index: &ConnectedIndex, query: &str) -> Filters {
    let q = format!(" {} ", normalize_label(query));
    let country = index
        .countries
        .iter()
        .filter(|c| c.len() > 2)
        .find(|c| q.contains(&format!(" {} ", normalize_label(c))))
        .cloned();
    let recruiting = if q.contains(" recruiting ") || q.contains(" recruiting trials ") {
        Some(!q.contains(" not recruiting ") && !q.contains(" no recruiting "))
    } else {
        None
    };
    Filters {
        country,
        recruiting,
        kind: None,
    }
}

pub(crate) fn keyword_filters(graph: &Graph, query: &str) -> Filters {
    let index = ConnectedIndex {
        labels: Vec::new(),
        countries: graph
            .studies()
            .iter()
            .flat_map(|study| study.countries.iter().cloned())
            .chain(graph.data().orgs.iter().filter_map(|org| org.country.clone()))
            .collect(),
    };
    local_filters(&index, query)
}

pub async fn schema(State(s): State<AppState>) -> ApiResult {
    static DATA: std::sync::LazyLock<crate::explore_stats::DurableScanCache<Value>> =
        std::sync::LazyLock::new(|| crate::explore_stats::DurableScanCache::new(std::time::Duration::from_secs(5)));
    let state = s.clone();
    let data = DATA
        .get_or_try_init(move || {
            let started = Instant::now();
            eprintln!("source statistics: scan started");
            let value = crate::explore_stats::summarize(&state.atlas, &state.graph);
            eprintln!(
                "source statistics: scan ready ({:.2}s)",
                started.elapsed().as_secs_f64()
            );
            Ok(value)
        })
        .await
        .map_err(internal)?;
    let data = data.as_ref();
    let classes: Vec<Value> = [
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Asset,
        NodeKind::Study,
        NodeKind::Grant,
        NodeKind::Paper,
    ]
    .into_iter()
    .map(|kind| json!({"kind":kind,"count":s.graph.node_count(kind)}))
    .collect();
    Ok(Json(
        json!({"version":1,"classes":classes,"atlas":s.atlas.stats(),"data":data,
        "relations":Relation::ALL.map(Relation::as_str),"core_relations":[views::GENE_RELATION, views::phenotype_relation(false), views::phenotype_relation(true)],
        "understanding_schema":understanding_schema().schema,"model_skill":MODEL_SKILL,
        "limits":{"query_bytes":MAX_QUERY,"nodes":MAX_NODES,"edges":MAX_EDGES},
        "execution":{"sparql_configured":std::env::var("ATLAS_SPARQL_URL").is_ok_and(|s|!s.trim().is_empty()),"store_snapshot_equivalence":"unverified","dataset":dataset_lineage()}}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparqlRequest {
    pub sparql: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub focus: Vec<String>,
    /// Validated question context; never compiled into SPARQL constraints.
    pub semantic_focus: Option<Vec<String>>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub reasoning: bool,
}

fn validated_sparql_context(s: &AppState, ids: &[String]) -> Result<Vec<NodeRef>, ApiError> {
    if ids.len() > 16 {
        return Err(explore_error(StatusCode::BAD_REQUEST, "Too many context identifiers"));
    }
    s.withhold.with_query_visibility(|allowed| {
        let mut seen = BTreeSet::new();
        ids.iter()
            .map(|id| {
                if id.is_empty() || id.len() > 256 || !seen.insert(id) {
                    return Err(explore_error(StatusCode::BAD_REQUEST, "Invalid or duplicate context identifier"));
                }
                available(&s.atlas, &s.graph, id)
                    .filter(|node| node.id == *id && allowed(id))
                    .ok_or_else(|| explore_error(StatusCode::BAD_REQUEST, "Context is unavailable"))
            })
            .collect()
    })
}

pub async fn sparql(State(s): State<AppState>, Json(req): Json<SparqlRequest>) -> ApiResult {
    let limit = req.limit.unwrap_or(40);
    if req.sparql.trim().is_empty()
        || req.sparql.len() > 16 * 1024
        || req.query.len() > MAX_QUERY
        || req.focus.len() > 16
        || !(1..=160).contains(&limit)
    {
        return Err(explore_error(
            StatusCode::BAD_REQUEST,
            "SPARQL, focus or result limit is invalid",
        ));
    }
    let linked = validated_sparql_context(&s, &req.focus)?;
    if let Some(semantic) = &req.semantic_focus {
        validated_sparql_context(&s, semantic)?;
    }
    let _permit = GATE
        .try_acquire()
        .map_err(|_| explore_error(StatusCode::TOO_MANY_REQUESTS, "Search is busy"))?;
    let outcome = crate::explore_query::run(
        &s,
        &req.sparql,
        limit,
        req.reasoning,
        &req.focus,
        req.semantic_focus.as_deref(),
    )
    .await
    .map_err(|e| explore_error(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    let mut body = canonical_response(&s, &req.query, &linked, outcome, limit);
    body["interpretation"]["mode"] = json!("sparql");
    Ok(Json(body))
}

pub async fn search(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<ExploreRequest>) -> ApiResult {
    let started = Instant::now();
    let lang = search_language(&headers)?;
    if req.query.len() > MAX_QUERY || (req.query.trim().is_empty() && req.plan.is_none()) {
        return Err(explore_error(
            StatusCode::BAD_REQUEST,
            "Enter a query of up to 32000 bytes",
        ));
    }
    if req
        .mode
        .as_deref()
        .is_some_and(|m| m != "knowledge" && m != "community")
    {
        return Err(explore_error(StatusCode::BAD_REQUEST, "Unknown graph mode"));
    }
    let _permit = GATE
        .try_acquire()
        .map_err(|_| explore_error(StatusCode::TOO_MANY_REQUESTS, "Search is busy; try again shortly"))?;
    let limit = req.limit.unwrap_or(20).clamp(1, 40);
    let prepared = if req.query.trim().is_empty() {
        None
    } else {
        Some(
            atlas_intake::prepare(
                atlas_intake::Input::Paste(&req.query),
                &atlas_intake::Limits {
                    max_model_chars: MAX_QUERY,
                    ..Default::default()
                },
            )
            .map_err(|error| explore_error(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?,
        )
    };
    let query = prepared.as_ref().map(|p| p.sent.clone()).unwrap_or_default();
    let s2 = s.clone();
    let q2 = query.clone();
    let (mut entities, mut intent, suggestion) = tokio::task::spawn_blocking(move || {
        let empty = ConnectedIndex::default();
        let fast = local_understanding(s2.atlas(), &s2.graph, &empty, &q2);
        if !fast.0.is_empty() || q2.trim().is_empty() {
            fast
        } else {
            let index = s2.explore_index.get_or_init(|| ConnectedIndex::new(&s2.graph));
            local_understanding(s2.atlas(), &s2.graph, index, &q2)
        }
    })
    .await
    .map_err(internal)?;
    let mode = "indexed";
    let warning: Option<&str> = None;
    let activities: Vec<String> = Vec::new();
    let country_index = ConnectedIndex {
        labels: vec![],
        countries: s
            .graph
            .studies()
            .iter()
            .flat_map(|study| study.countries.iter().cloned())
            .chain(s.graph.data().orgs.iter().filter_map(|org| org.country.clone()))
            .collect(),
    };
    let mut filters = local_filters(&country_index, &query);
    let model: Option<String> = None;
    let connection: Option<String> = None;
    if let Some(plan) = req.plan {
        validate_plan(&s.atlas, &s.graph, &plan)?;
        entities = plan
            .focus
            .iter()
            .filter_map(|id| available(&s.atlas, &s.graph, id))
            .collect();
        intent = plan.intent;
        filters = plan.filters;
    } else if query.split_whitespace().count() > 1 || entities.is_empty() || headers.contains_key("x-llm-connection") {
        let linked_ids: Vec<String> = entities.iter().map(|n| n.id.clone()).collect();
        let outcome = crate::explore_query::understand(
            &s,
            &headers,
            prepared.as_ref().expect("nonempty natural-language query"),
            &linked_ids,
            lang,
        )
        .await
        .map_err(routing_error)?;
        return Ok(Json(canonical_response(&s, &query, &entities, outcome, limit)));
    }
    if req.mode.as_deref() == Some("community") && intent == Intent::All {
        intent = Intent::Researchers;
    }
    let plan = Plan {
        focus: entities.iter().map(|e| e.id.clone()).collect(),
        intent,
        filters,
    };
    validate_plan(&s.atlas, &s.graph, &plan)?;
    let mut engine = "indexed-atlas";
    let mut sparql = None;
    let mut executed_queries = Vec::new();
    let mut store_edges = None;
    let mut store_truncated = false;
    if !plan.focus.is_empty() {
        let relation_scope = relation_scope(intent);
        match explore_sparql::execute(&plan.focus, &relation_scope, MAX_EDGES).await {
            Ok(Some(result)) => {
                engine = "nrese";
                for receipt in &result.queries {
                    executed_queries.push(json!({"seed_ids":receipt.seed_ids,"stage":"neighborhood","sparql":receipt.sparql,"engine":"nrese","row_count":receipt.row_count,"limit":receipt.row_cap,"row_cap":receipt.row_cap,"reasoning":receipt.reasoning}));
                }
                sparql = Some(result.query);
                store_truncated = result.truncated;
                let mut triples = result.triples;
                let mut frontier: Vec<String> = triples
                    .iter()
                    .flat_map(|t| [&t.from, &t.to])
                    .filter(|id| !plan.focus.contains(id))
                    .filter(|id| {
                        s.atlas.disease_idx(id).is_some()
                            || s.atlas.gene(id).is_some()
                            || (matches!(intent, Intent::Researchers | Intent::All | Intent::Funding)
                                && s.graph.node(id).is_some_and(|k| {
                                    matches!(
                                        k.kind,
                                        NodeKind::Paper | NodeKind::Grant | NodeKind::Study | NodeKind::Asset
                                    )
                                }))
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                // Discovery may inspect indexed witnesses, but only intermediates actually
                // returned by the store can enter the next store query. Prefer a useful
                // path for this job over lexical catalogue identifiers.
                let witness_score = |id: &String| {
                    let researchers = s.graph.incident(id).take(250).any(|incident| {
                        let edge = incident.edge;
                        let other = if edge.from == *id { &edge.to } else { &edge.from };
                        available(&s.atlas, &s.graph, other).is_some_and(|n| n.kind == NodeKind::Person)
                            && s.graph.records_withheld(&edge.records).is_none()
                    });
                    let disease = s
                        .atlas
                        .disease_idx(id)
                        .map(|d| {
                            s.atlas.disease_at(d).genes.iter().any(|link| {
                                link.is_causal()
                                    && s.atlas
                                        .gene(&link.symbol)
                                        .is_some_and(|g| plan.focus.contains(&s.atlas.gene_at(g).id().to_owned()))
                            })
                        })
                        .unwrap_or(false);
                    usize::from(researchers) * 100 + usize::from(disease) * 20
                };
                frontier.sort_by_cached_key(|id| (std::cmp::Reverse(witness_score(id)), id.clone()));
                frontier.truncate(16);
                if !frontier.is_empty() {
                    if let Some(second) = explore_sparql::execute(&frontier, &relation_scope, MAX_EDGES)
                        .await
                        .map_err(|e| explore_error(StatusCode::SERVICE_UNAVAILABLE, e))?
                    {
                        for receipt in &second.queries {
                            executed_queries.push(json!({"seed_ids":receipt.seed_ids,"stage":"frontier","sparql":receipt.sparql,"engine":"nrese","row_count":receipt.row_count,"limit":receipt.row_cap,"row_cap":receipt.row_cap,"reasoning":receipt.reasoning}));
                        }
                        store_truncated |= second.truncated;
                        triples.extend(second.triples);
                    }
                }
                store_edges = Some(triples);
            }
            Ok(None) => {}
            Err(error) => return Err(explore_error(StatusCode::SERVICE_UNAVAILABLE, error)),
        }
    }
    let s2 = s.clone();
    let plan2 = plan.clone();
    let mut body =
        tokio::task::spawn_blocking(move || retrieve(&s2.atlas, &s2.graph, &plan2, limit, store_edges.as_deref()))
            .await
            .map_err(internal)?;
    body["query"] = json!(query);
    body["plan"] = json!(plan);
    body["interpretation"] = json!({"mode":mode,"entities":entities,"intent":intent,"warning":warning,"suggestion":suggestion,"model":model,"connection":connection,"activities":activities,"redactions":prepared.as_ref().map(|p| &p.redacted.counts)});
    body["execution"] = json!({"engine":engine,"sparql":sparql,"queries":executed_queries,"dataset":if engine == "nrese" { dataset_lineage() } else {Value::Null},"runtime_reasoning":false,"elapsed_ms":started.elapsed().as_millis() as u64,"truncated":store_truncated || body["truncated"].as_bool().unwrap_or(false)});
    body.as_object_mut().expect("object").remove("truncated");
    Ok(Json(body))
}

/// The canonical engine's rows remain the answer. Only exact returned graph IDs
/// and actual returned triples are projected into the visual graph; an aggregate
/// or empty answer never becomes an unrelated intent neighborhood.
fn canonical_response(
    s: &AppState,
    query: &str,
    linked: &[NodeRef],
    outcome: crate::explore_query::QueryOutcome,
    limit: usize,
) -> Value {
    let returned: BTreeSet<String> = outcome.result_ids.iter().cloned().collect();
    let mut ids = returned.clone();
    for t in &outcome.triples {
        ids.insert(t.from.clone());
        ids.insert(t.to.clone());
    }
    let inputs: BTreeSet<String> = linked.iter().map(|n| n.id.clone()).collect();
    // Inputs identify the question, including an aggregate with no entity rows.
    // They are never included as answer results unless the store returned them.
    let mut nodes: BTreeMap<String, NodeRef> = linked.iter().cloned().map(|n| (n.id.clone(), n)).collect();
    for id in ids {
        if nodes.len() >= MAX_NODES {
            break;
        }
        if let Some(node) = available(&s.atlas, &s.graph, &id) {
            nodes.insert(id, node);
        }
    }
    let edges: Vec<Value> = outcome.triples.iter().filter(|t| nodes.contains_key(&t.from) && nodes.contains_key(&t.to)).take(MAX_EDGES).filter_map(|t| {
        // Source metadata can be attached only to the exact returned assertion.
        let id = format!("{}|{}|{}",t.from,t.relation,t.to);
        if t.relation == "subclass_of" {
            let proof = outcome.reasoning_proofs.iter().find(|p|p["source"] == t.from && p["target"] == t.to)?;
            let kind = if proof["origin"] == "inferred" {"inferred"} else {"observed"};
            let evidence: Vec<Value> = proof["premise_records"].as_array().into_iter().flatten()
                .flat_map(|p|p["records"].as_array().into_iter().flatten()).take(32).map(|r|json!({
                    "source":r["source"]["value"],"record":r["record"]["value"],"record_locator":r["locator"]["value"],
                    "sha256":r["hash"]["value"],"source_sha256":r["sourceHash"]["value"],
                    "url":r["url"]["value"],"retrieved_at":r["retrieved"]["value"],"version":r["version"]["value"]
                })).collect();
            return Some(json!({"id":id,"source":t.from,"target":t.to,"relation":"subclass_of","label":"subclass of",
                "kind":kind,"highlighted":true,"runtime_reasoning":kind == "inferred","proof":proof,
                "evidence":evidence,"reason":if kind == "inferred" {"nrese verified ontology transitivity"} else {"Source ontology hierarchy assertion"}}));
        }
        if let Some(inc) = s.graph.incident(&t.from).find(|inc| inc.edge.from == t.from && inc.edge.to == t.to && inc.edge.relation.as_str() == t.relation) {
            graph_edge(&s.graph,inc.edge)
        } else if let Some(edge) = atlas_adjacency(&s.atlas,&t.from).into_iter().find(|edge| edge["source"] == t.from && edge["target"] == t.to && edge["relation"] == t.relation) {
            Some(edge)
        } else {
            Some(json!({"id":id,"source":t.from,"target":t.to,"relation":t.relation,"label":t.relation.replace('_'," "),"highlighted":true,"evidence":canonical_evidence(&outcome.query_execution,&id,true)}))
        }
    }).collect();
    let mut results: Vec<Value> = nodes.values().filter(|n| returned.contains(&n.id)).take(limit).map(|n|json!({"id":n.id,"label":n.label,"kind":n.kind,"reason":"Returned by the checked graph query","score":1,"evidence":canonical_evidence(&outcome.query_execution,&n.id,false),"url":reach(&s.graph,n)})).collect();
    for result in &mut results {
        enrich_result(&s.graph, result);
    }
    let graph_nodes: Vec<Value> = nodes
        .values()
        .map(|n| json!({"id":n.id,"label":n.label,"kind":n.kind,"subkind":node_subkind(&s.graph,n),"matched":returned.contains(&n.id)||inputs.contains(&n.id),"input":inputs.contains(&n.id)}))
        .collect();
    let reasoning = outcome.query_execution["answer"]["results"]
        .as_array()
        .is_some_and(|results| {
            results
                .iter()
                .any(|r| r["activity"]["parameters"]["infer"].as_bool() == Some(true))
        });
    let executed_queries: Vec<Value> = outcome.query_execution["answer"]["results"].as_array().into_iter().flatten()
        .filter(|result| result["query"].is_string()).map(|result| json!({
            "sparql":result["query"],"engine":result["backend"],"stage":"canonical",
            "row_cap":result["activity"]["parameters"]["row_cap"],"limit":result["activity"]["parameters"]["row_cap"],
            "reasoning":result["activity"]["parameters"]["infer"],"activity":result["activity"],
            "elapsed_ms":result["latency_ms"],"truncated":result["truncated"],
            "row_count":result["data"]["results"]["bindings"].as_array().map(Vec::len)
        })).collect();
    json!({"query":query,"interpretation":{"mode":"agent","entities":linked,"intent":"query","model":outcome.model,"connection":outcome.connection,"activities":outcome.activities,"routing":outcome.query_execution["routing"]},
        "results":results,"graph":{"nodes":graph_nodes,"edges":edges},
        "execution":{"engine":outcome.backend,"sparql":outcome.sparql,"queries":executed_queries,"elapsed_ms":outcome.elapsed_ms,"truncated":outcome.truncated || returned.len()>limit,"dataset":dataset_lineage(),"runtime_reasoning":reasoning,"reasoning_warning":outcome.reasoning_warning,"reasoning_proofs":outcome.reasoning_proofs},
        "query_execution":outcome.query_execution})
}

fn search_language(headers: &HeaderMap) -> Result<&str, ApiError> {
    let lang = headers
        .get("x-atlas-lang")
        .map(|h| h.to_str())
        .transpose()
        .map_err(|_| explore_error(StatusCode::BAD_REQUEST, "Invalid search language."))?
        .unwrap_or("en");
    if !matches!(lang, "en" | "de") {
        return Err(explore_error(StatusCode::BAD_REQUEST, "Invalid search language."));
    }
    Ok(lang)
}

/// Source-record metadata for an already returned entity, never extra search results.
/// Keep metadata evidence separate from evidence for the query's returned assertion.
fn enrich_result(graph: &Graph, result: &mut Value) {
    let Some(id) = result["id"].as_str() else {
        return;
    };
    let Some(key) = graph.node(id) else {
        return;
    };
    if graph.node_withheld(key).is_some() {
        return;
    }
    let mut facts: Vec<(String, String)> = Vec::new();
    let mut fields = serde_json::Map::new();
    match key.kind {
        NodeKind::Study => {
            let s = graph.study(key.idx);
            fields.insert("status".into(), json!(s.status));
            facts.extend([
                ("study_kind".into(), s.kind.as_str().into()),
                ("status".into(), s.status.clone()),
                ("sponsor".into(), s.sponsor.clone()),
                (
                    "country".into(),
                    s.countries.iter().take(8).cloned().collect::<Vec<_>>().join(", "),
                ),
            ]);
            if let Some(n) = s.enrollment {
                facts.push(("enrollment".into(), n.to_string()));
            }
            let action = nodes::study_reach(graph, &s.id);
            fields.insert(
                "how_to_get".into(),
                json!({"route":"official_page","url":action["url"]}),
            );
        }
        NodeKind::Paper => {
            let p = graph.paper(key.idx);
            facts.push(("journal".into(), p.journal.clone()));
            if let Some(year) = p.year {
                facts.push(("year".into(), year.to_string()));
            }
            if let Some(doi) = &p.doi {
                facts.push(("doi".into(), doi.clone()));
            }
        }
        NodeKind::Person => {
            let p = graph.person(key.idx);
            facts.extend(p.affiliations.iter().take(3).map(|a| ("affiliation".into(), a.clone())));
            let reach = nodes::person_reach(p);
            if reach["url"].is_string() {
                fields.insert("how_to_get".into(), json!({"route":reach["kind"],"url":reach["url"]}));
            }
        }
        NodeKind::Organisation => {
            let o = graph.org(key.idx);
            facts.push(("organisation_kind".into(), o.kind.as_str().into()));
            if let Some(c) = &o.country {
                facts.push(("country".into(), c.clone()));
            }
            if let Some(description) = &o.description {
                facts.push(("description".into(), description.clone()));
            }
            if let Some(url) = o.contact_url.as_ref().or(o.url.as_ref()) {
                fields.insert(
                    "how_to_get".into(),
                    json!({"route":if o.contact_url.is_some() { "contact_form" } else { "official_page" },"url":url}),
                );
            }
            if let Some(initiative) = graph
                .data()
                .initiatives
                .iter()
                .find(|i| i.id == o.id && i.verified && graph.records_withheld(&i.records).is_none())
            {
                // Use an acquired official action, never a similarly named group's link.
                if let Some(action) = initiative
                    .actions
                    .iter()
                    .filter(|a| graph.records_withheld(&a.records).is_none() && !a.url.is_empty())
                    .min_by_key(|a| {
                        let audience = a.audience.to_lowercase();
                        !(audience.contains("famil")
                            || audience.contains("caregiver")
                            || audience.contains("patient") && !audience.contains("organisation"))
                    })
                {
                    fields.insert("official_action".into(), json!({"action":action.action,"url":action.url,"outcome":action.outcome,"audience":action.audience,"availability":action.availability,
                        "source":{"url":action.source_url,"version":action.source_version,"sha256":action.sha256,"record_locator":action.record_locator,"retrieved_at":action.retrieved_at}}));
                    fields.insert("url".into(), json!(action.url));
                }
            }
        }
        NodeKind::Grant => {
            let g = graph.grant(key.idx);
            facts.extend([
                ("agency".into(), g.agency.clone()),
                ("country".into(), g.country.clone()),
            ]);
            if !g.organisation.is_empty() {
                fields.insert("holder".into(), json!({"name":g.organisation}));
            }
            if !g.url.is_empty() {
                fields.insert("how_to_get".into(), json!({"route":"official_page","url":g.url}));
            }
        }
        NodeKind::Asset => {
            let a = graph.asset(key.idx);
            facts.push(("asset_kind".into(), a.kind.as_str().into()));
            facts.extend(a.facts.iter().filter(|(k, _)| k != "access_context").take(7).cloned());
            if let Some(status) = a.fact("status") {
                fields.insert("status".into(), json!(status));
            }
            let holder = a
                .holder
                .as_deref()
                .and_then(|id| graph.node(id))
                .filter(|k| graph.node_withheld(*k).is_none())
                .map(|k| graph.node_ref(k));
            if let Some(name) = holder.as_ref().map(|h| h.label.as_str()).or(a.holder_name.as_deref()) {
                fields.insert(
                    "holder".into(),
                    json!({"id":holder.as_ref().map(|h| &h.id),"name":name}),
                );
            }
            fields.insert(
                "how_to_get".into(),
                json!({"route":a.access.route,"url":a.access.url,"note":a.access.note}),
            );
        }
        _ => {}
    }
    fields.insert(
        "facts".into(),
        json!(
            facts
                .into_iter()
                .filter(|(k, v)| !k.is_empty() && k.len() <= 100 && !v.trim().is_empty() && v.len() <= 1600)
                .take(8)
                .map(|(key, value)| json!({"key":key,"value":value}))
                .collect::<Vec<_>>()
        ),
    );
    fields.insert(
        "metadata_evidence".into(),
        json!(graph_evidence(graph, graph.node_records(key))),
    );
    fields.insert("provenance_id".into(), json!(graph.node_ref(key).id));
    if let Some(object) = result.as_object_mut() {
        object.extend(fields);
    }
}

fn node_subkind(graph: &Graph, node: &NodeRef) -> Option<&'static str> {
    let key = graph.node(&node.id)?;
    match key.kind {
        NodeKind::Organisation => Some(graph.org(key.idx).kind.as_str()),
        NodeKind::Study => Some(graph.study(key.idx).kind.as_str()),
        NodeKind::Asset => Some(graph.asset(key.idx).kind.as_str()),
        _ => None,
    }
}

fn canonical_evidence(execution: &Value, id: &str, edge: bool) -> Vec<Value> {
    let iri = atlas_ask::query::iri(if edge { "edge" } else { "id" }, id);
    let iri = iri.trim_matches(['<', '>']);
    let text = |value: &Value| value.as_str().or_else(|| value["value"].as_str()).map(str::to_owned);
    let mut records = Vec::new();
    for result in execution["answer"]["results"].as_array().into_iter().flatten() {
        for record in result["provenance"].as_array().into_iter().flatten() {
            if text(&record["item"])
                .as_deref()
                .is_none_or(|item| item != id && item != iri)
            {
                continue;
            }
            let source = text(&record["source"])
                .or_else(|| text(&record["record"]))
                .or_else(|| text(&record["source_url"]));
            let Some(source) = source else {
                continue;
            };
            let mut evidence = serde_json::Map::new();
            evidence.insert("source".into(), json!(source));
            for (field, names) in [
                ("url", &["url", "source_url"][..]),
                ("record", &["locator", "record_locator"][..]),
                ("retrieved_at", &["retrieved", "retrieved_at"][..]),
                ("version", &["version"][..]),
                ("sha256", &["hash", "sha256"][..]),
            ] {
                if let Some(value) = names.iter().find_map(|name| text(&record[*name])) {
                    evidence.insert(field.into(), json!(value));
                }
            }
            let evidence = Value::Object(evidence);
            if !records.contains(&evidence) {
                records.push(evidence);
            }
            if records.len() == 12 {
                return records;
            }
        }
    }
    records
}

fn validate_plan(atlas: &Atlas, graph: &Graph, plan: &Plan) -> Result<(), ApiError> {
    if plan.focus.len() > 8
        || plan
            .focus
            .iter()
            .any(|id| id.len() > 256 || available(atlas, graph, id).is_none())
    {
        return Err(explore_error(
            StatusCode::BAD_REQUEST,
            "Plan focus must use available graph identifiers",
        ));
    }
    if [&plan.filters.country, &plan.filters.kind]
        .into_iter()
        .flatten()
        .any(|v| v.len() > 100)
    {
        return Err(explore_error(StatusCode::BAD_REQUEST, "Filter too long"));
    }
    if plan.filters.recruiting.is_some() && !matches!(plan.intent, Intent::Studies | Intent::All) {
        return Err(explore_error(
            StatusCode::BAD_REQUEST,
            "Recruiting filter requires studies",
        ));
    }
    Ok(())
}

fn source_evidence(atlas: &Atlas, record: &RecordRef) -> Value {
    let e = atlas.provenance.entity(record.entity);
    json!({"source":e.id,"url":e.url,"record":atlas.provenance.cite(record),"retrieved_at":e.retrieved_at,"version":e.version,"sha256":e.sha256,"licence":e.licence})
}

fn graph_evidence(graph: &Graph, records: &[u32]) -> Vec<Value> {
    records.iter().take(8).map(|&r| {
        let record = graph.record(r);
        let entity = graph.provenance().entity(record.entity);
        json!({"source":entity.id,"url":record.url.as_deref().unwrap_or(&entity.url),"record":format!("{}#{}",entity.file,record.locator),"retrieved_at":record.fetched_at.as_ref().or(entity.retrieved_at.as_ref()),"version":entity.version,"sha256":atlas_core::graph::hex(&record.sha256),"licence":entity.licence,"licence_class":graph.licence_class(records)})
    }).collect()
}

fn graph_edge(graph: &Graph, edge: &GraphEdge) -> Option<Value> {
    if graph.records_withheld(&edge.records).is_some() {
        return None;
    }
    Some(
        json!({"id":edge.id(),"source":edge.from,"target":edge.to,"relation":edge.relation.as_str(),"label":edge.relation.as_str().replace('_'," "),"kind":edge.kind,"reason":edge.reason,"highlighted":true,"evidence":graph_evidence(graph,&edge.records)}),
    )
}

fn atlas_edge(atlas: &Atlas, edge: Edge, records: Vec<&RecordRef>) -> Value {
    let evidence: Vec<Value> = records.into_iter().take(8).map(|r| source_evidence(atlas, r)).collect();
    json!({"id":edge.id,"source":edge.from,"target":edge.to,"relation":edge.relation,"label":edge.relation.replace('_'," "),"kind":edge.kind,"highlighted":true,"evidence":evidence})
}

fn atlas_adjacency(atlas: &Atlas, id: &str) -> Vec<Value> {
    let mut edges = Vec::new();
    if let Some(d) = atlas.disease_idx(id) {
        let disease = atlas.disease_at(d);
        for (_, links, edge) in views::gene_edges(atlas, &disease.id, &disease.genes) {
            edges.push(atlas_edge(atlas, edge, links.iter().map(|g| &g.record).collect()));
        }
        for pe in disease.phenotypes.iter().take(24) {
            edges.push(atlas_edge(
                atlas,
                views::phenotype_edge(atlas, &disease.id, pe, false),
                pe.annotations.iter().map(|a| &a.record).collect(),
            ));
        }
    } else if let Some(g) = atlas.gene(id) {
        for &d in atlas.gene_at(g).diseases.iter().take(40) {
            let disease = atlas.disease_at(d);
            for (gene, links, edge) in views::gene_edges(atlas, &disease.id, &disease.genes) {
                if gene.id == id {
                    edges.push(atlas_edge(atlas, edge, links.iter().map(|g| &g.record).collect()));
                }
            }
        }
    } else if let Some(p) = atlas.hpo.canonical(id) {
        for &d in atlas.by_phenotype(p).iter().take(40) {
            let disease = atlas.disease_at(d);
            if let Some(pe) = disease.phenotypes.iter().find(|e| e.term == p) {
                edges.push(atlas_edge(
                    atlas,
                    views::phenotype_edge(atlas, &disease.id, pe, false),
                    pe.annotations.iter().map(|a| &a.record).collect(),
                ));
            }
        }
    }
    edges
}

fn adjacency(atlas: &Atlas, graph: &Graph, id: &str, work: &mut usize, truncated: &mut bool) -> Vec<Value> {
    let mut edges = atlas_adjacency(atlas, id);
    for incident in graph.incident(id) {
        *work += 1;
        if *work > MAX_WORK {
            *truncated = true;
            break;
        }
        if let Some(edge) = graph_edge(graph, incident.edge) {
            edges.push(edge);
        }
    }
    edges
}

fn wanted(graph: &Graph, node: &NodeRef, intent: Intent, filters: &Filters) -> bool {
    let key = graph.node(&node.id);
    let intent_ok = match intent {
        Intent::All | Intent::Gaps => true,
        Intent::Conditions => node.kind == NodeKind::Disease,
        Intent::Researchers => node.kind == NodeKind::Person,
        Intent::Studies => node.kind == NodeKind::Study,
        Intent::Papers => node.kind == NodeKind::Paper,
        Intent::Funding => {
            node.kind == NodeKind::Grant
                || key.is_some_and(|k| {
                    k.kind == NodeKind::Asset && graph.asset(k.idx).kind == atlas_core::graph::AssetKind::FundingCall
                })
        }
        Intent::Models => key.is_some_and(|k| {
            k.kind == NodeKind::Asset
                && matches!(
                    graph.asset(k.idx).kind,
                    atlas_core::graph::AssetKind::Model
                        | atlas_core::graph::AssetKind::CellLine
                        | atlas_core::graph::AssetKind::Biobank
                )
        }),
        Intent::Therapies => key.is_some_and(|k| {
            k.kind == NodeKind::Asset
                && matches!(
                    graph.asset(k.idx).kind,
                    atlas_core::graph::AssetKind::Programme
                        | atlas_core::graph::AssetKind::Drug
                        | atlas_core::graph::AssetKind::Designation
                )
        }),
        Intent::Resources => key.is_some_and(|k| {
            k.kind == NodeKind::Asset
                && matches!(
                    graph.asset(k.idx).kind,
                    atlas_core::graph::AssetKind::Dataset
                        | atlas_core::graph::AssetKind::Registry
                        | atlas_core::graph::AssetKind::Biobank
                )
        }),
        Intent::Groups => key.is_some_and(|k| {
            k.kind == NodeKind::Organisation
                && matches!(
                    graph.org(k.idx).kind,
                    atlas_core::graph::OrgKind::PatientGroup | atlas_core::graph::OrgKind::ExpertCentre
                )
        }),
        Intent::Outcomes => key.is_some_and(|k| {
            k.kind == NodeKind::Asset && graph.asset(k.idx).kind == atlas_core::graph::AssetKind::OutcomeMeasure
        }),
    };
    if !intent_ok {
        return false;
    }
    if let Some(recruiting) = filters.recruiting {
        if !key.is_some_and(|k| k.kind == NodeKind::Study && graph.study(k.idx).is_recruiting() == recruiting) {
            return false;
        }
    }
    if let Some(country) = &filters.country {
        let q = normalize_label(country);
        let matches = key.is_some_and(|k| match k.kind {
            NodeKind::Study => graph.study(k.idx).countries.iter().any(|c| normalize_label(c) == q),
            NodeKind::Organisation => graph
                .org(k.idx)
                .country
                .as_deref()
                .is_some_and(|c| normalize_label(c) == q),
            NodeKind::Grant => normalize_label(&graph.grant(k.idx).country) == q,
            NodeKind::Asset => graph
                .asset(k.idx)
                .fact("country")
                .is_some_and(|c| normalize_label(c) == q),
            _ => false,
        });
        if !matches {
            return false;
        }
    }
    if let Some(kind) = &filters.kind {
        let subtype = key
            .map(|k| match k.kind {
                NodeKind::Study => graph.study(k.idx).kind.as_str(),
                NodeKind::Organisation => graph.org(k.idx).kind.as_str(),
                NodeKind::Asset => graph.asset(k.idx).kind.as_str(),
                _ => node.kind.as_str(),
            })
            .unwrap_or(node.kind.as_str());
        if normalize_label(kind) != normalize_label(subtype) {
            return false;
        }
    }
    true
}

fn reach(graph: &Graph, node: &NodeRef) -> Option<String> {
    graph.node(&node.id).and_then(|k| match k.kind {
        NodeKind::Study => Some(format!("https://clinicaltrials.gov/study/{}", node.id)),
        NodeKind::Paper => Some(format!(
            "https://pubmed.ncbi.nlm.nih.gov/{}/",
            node.id.trim_start_matches("PMID:")
        )),
        NodeKind::Person => nodes::person_reach(graph.person(k.idx))["url"]
            .as_str()
            .map(str::to_owned),
        NodeKind::Grant => Some(graph.grant(k.idx).url.clone()),
        NodeKind::Organisation => graph
            .org(k.idx)
            .contact_url
            .clone()
            .or_else(|| graph.org(k.idx).url.clone()),
        NodeKind::Asset => graph
            .asset(k.idx)
            .access
            .url
            .clone()
            .or_else(|| graph.asset(k.idx).verify_url.clone()),
        _ => None,
    })
}

fn retrieve(
    atlas: &Atlas,
    graph: &Graph,
    plan: &Plan,
    limit: usize,
    store: Option<&[explore_sparql::SparqlTriple]>,
) -> Value {
    let mut nodes_by_id = BTreeMap::<String, NodeRef>::new();
    let mut edges_by_id = BTreeMap::<String, Value>::new();
    let mut depths = HashMap::<String, usize>::new();
    let mut paths = HashMap::<String, Vec<String>>::new();
    let mut queue = VecDeque::new();
    let mut truncated = false;
    let mut work = 0;
    let mut type_counts = BTreeMap::<String, usize>::new();
    let mut nearby_edges = BTreeMap::<String, Value>::new();
    for id in &plan.focus {
        if let Some(node) = available(atlas, graph, id) {
            nodes_by_id.insert(id.clone(), node);
            depths.insert(id.clone(), 0);
            paths.insert(id.clone(), vec![]);
            queue.push_back(id.clone());
        }
    }
    let mut stored = HashMap::<String, Vec<Value>>::new();
    if let Some(triples) = store {
        for t in triples {
            // Hydrate only real retained assertions, never invent provenance for store bindings.
            let Some(from) = available(atlas, graph, &t.from) else {
                continue;
            };
            let Some(to) = available(atlas, graph, &t.to) else {
                continue;
            };
            let eid = atlas_core::node::edge_id(&from.id, &t.relation, &to.id);
            let edge = graph
                .edge_by_id(&eid)
                .and_then(|i| graph_edge(graph, graph.edge(i)))
                .or_else(|| atlas_adjacency(atlas, &from.id).into_iter().find(|e| e["id"] == eid));
            if let Some(edge) = edge {
                stored.entry(from.id).or_default().push(edge.clone());
                stored.entry(to.id).or_default().push(edge);
            }
        }
    }
    while let Some(id) = queue.pop_front() {
        let depth = depths[&id];
        if depth >= 2 {
            continue;
        }
        let mut candidates = if store.is_some() {
            stored.remove(&id).unwrap_or_default()
        } else {
            adjacency(atlas, graph, &id, &mut work, &mut truncated)
        };
        candidates.sort_by(|a, b| {
            let other = |edge: &Value| {
                if edge["source"] == id {
                    edge["target"].as_str().unwrap_or_default().to_owned()
                } else {
                    edge["source"].as_str().unwrap_or_default().to_owned()
                }
            };
            let priority = |edge: &Value| {
                available(atlas, graph, &other(edge)).map_or(0, |n| {
                    usize::from(wanted(graph, &n, plan.intent, &plan.filters)) * 10
                        + usize::from(matches!(n.kind, NodeKind::Disease | NodeKind::Gene)) * 5
                        + usize::from(
                            plan.intent == Intent::Researchers && matches!(n.kind, NodeKind::Paper | NodeKind::Grant),
                        ) * 8
                        + usize::from(
                            plan.intent == Intent::All
                                && matches!(n.kind, NodeKind::Paper | NodeKind::Grant | NodeKind::Study),
                        ) * 4
                })
            };
            priority(b)
                .cmp(&priority(a))
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        let total = candidates.len();
        for edge in &candidates {
            if let Some(eid) = edge["id"].as_str() {
                nearby_edges.entry(eid.to_owned()).or_insert_with(|| edge.clone());
            }
        }
        let mut admitted = 0;
        for edge in candidates {
            let from = edge["source"].as_str().unwrap_or_default();
            let to = edge["target"].as_str().unwrap_or_default();
            let other = if from == id { to } else { from };
            let Some(node) = available(atlas, graph, other) else {
                continue;
            };
            if plan.intent == Intent::All && !nodes_by_id.contains_key(other) {
                if graph.node(other).is_some_and(|key| {
                    key.kind == NodeKind::Asset
                        && matches!(
                            graph.asset(key.idx).kind,
                            atlas_core::graph::AssetKind::Other | atlas_core::graph::AssetKind::OrthologGene
                        )
                }) {
                    truncated = true;
                    continue;
                }
                if node.kind == NodeKind::Gene
                    && atlas.disease_idx(&id).is_some_and(|d| {
                        !atlas
                            .disease_at(d)
                            .genes
                            .iter()
                            .any(|link| link.is_causal() && link.symbol == node.label)
                    })
                {
                    // Broad inferred enrichment does not become a causal diagnosis root.
                    truncated = true;
                    continue;
                }
                // A general human-gene overview is a balanced, connected preview.
                // Catalogues and other genes remain available through explicit intents/clicks.
                if (node.kind == NodeKind::Gene && (depth > 0 || nodes_by_id[&id].kind == NodeKind::Gene))
                    || (depth > 0 && node.kind == NodeKind::Disease)
                {
                    truncated = true;
                    continue;
                }
                let cap = match node.kind {
                    NodeKind::Gene => 3,
                    NodeKind::Asset => 3,
                    NodeKind::Phenotype => 3,
                    NodeKind::Disease => 6,
                    NodeKind::Person => 6,
                    NodeKind::Paper | NodeKind::Grant => 5,
                    _ => 4,
                };
                if type_counts.get(node.kind.as_str()).copied().unwrap_or(0) >= cap {
                    truncated = true;
                    continue;
                }
            }
            if admitted >= if depth == 0 { 40 } else { 12 } {
                truncated = true;
                break;
            }
            if nodes_by_id.len() >= MAX_NODES && !nodes_by_id.contains_key(other) {
                truncated = true;
                continue;
            }
            if edges_by_id.len() >= MAX_EDGES {
                truncated = true;
                break;
            }
            let eid = edge["id"].as_str().unwrap_or_default().to_owned();
            if !nodes_by_id.contains_key(other) {
                *type_counts.entry(node.kind.as_str().to_owned()).or_default() += 1;
                nodes_by_id.insert(other.to_owned(), node);
                depths.insert(other.to_owned(), depth + 1);
                let mut path = paths[&id].clone();
                path.push(eid.clone());
                paths.insert(other.to_owned(), path);
                queue.push_back(other.to_owned());
            }
            edges_by_id.insert(eid, edge);
            admitted += 1;
        }
        truncated |= total > if depth == 0 { 40 } else { 12 };
    }
    let mut results: Vec<Value> = nodes_by_id.values().filter(|node| wanted(graph, node, plan.intent, &plan.filters)).map(|node| {
        let depth = depths.get(&node.id).copied().unwrap_or(0);
        let path: Vec<&Value> = paths.get(&node.id).into_iter().flatten().filter_map(|id| edges_by_id.get(id)).collect();
        let mut evidence: Vec<Value> = path.iter().flat_map(|e| e["evidence"].as_array().into_iter().flatten().cloned()).take(12).collect();
        if evidence.is_empty() {
            if let Some(key) = graph.node(&node.id) { evidence = graph_evidence(graph, graph.node_records(key)); }
            else if let Some(d) = atlas.disease_idx(&node.id) { evidence = atlas.disease_at(d).derived_from.iter().take(8).map(|r| source_evidence(atlas,r)).collect(); }
            else if let Some(g) = atlas.gene(&node.id) {
                evidence = atlas.gene_at(g).diseases.iter().take(8).flat_map(|&d| atlas.disease_at(d).genes.iter())
                    .filter(|link| link.symbol == node.label).take(8).map(|link|source_evidence(atlas,&link.record)).collect();
            }
        }
        let reason = if depth == 0 { format!("Matched {}", node.label) }
            else { path.iter().map(|e| e["label"].as_str().unwrap_or_default()).collect::<Vec<_>>().join(" → ") };
        let recruiting_boost = graph.node(&node.id).is_some_and(|k| k.kind == NodeKind::Study && graph.study(k.idx).is_recruiting());
        let specificity = atlas.disease_idx(&node.id).map(|d| {
            let disease = atlas.disease_at(d);
            let causal: Vec<_> = disease.genes.iter().filter(|link|link.is_causal()).collect();
            let focused = causal.iter().any(|link|atlas.gene(&link.symbol).is_some_and(|g|plan.focus.iter().any(|id| id == atlas.gene_at(g).id())));
            if focused {15.0 / causal.len().max(1) as f64} else {0.0}
        }).unwrap_or(0.0);
        let causal_gene = node.kind == NodeKind::Gene && plan.focus.iter().filter_map(|id|atlas.disease_idx(id))
            .any(|d|atlas.disease_at(d).genes.iter().any(|link|link.is_causal() && link.symbol == node.label));
        let research_links: Vec<_> = edges_by_id.values().filter(|edge|node.kind == NodeKind::Person
            && (edge["source"] == node.id || edge["target"] == node.id)
            && matches!(edge["relation"].as_str(),Some("author_of" | "principal_investigator_of"))).collect();
        let investigator = research_links.iter().any(|edge|edge["relation"] == "principal_investigator_of");
        let research_boost = if investigator {12.0} else {0.0} + research_links.len().saturating_sub(1).min(5) as f64 * 2.0;
        json!({"id":node.id,"label":node.label,"kind":node.kind,"reason":reason,"score":100.0-depth as f64*20.0+specificity+research_boost+if causal_gene {15.0} else {0.0}+if recruiting_boost {5.0} else {0.0},"evidence":evidence,"url":reach(graph,node),"edge_ids":paths.get(&node.id)})
    }).collect();
    results.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .partial_cmp(&a["score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a["label"].as_str().cmp(&b["label"].as_str()))
    });
    truncated |= results.len() > limit;
    results.truncate(limit);
    // Results retain the balanced foreground. Context only reuses evidence-backed
    // edges already encountered in this bounded query, directly beside foreground.
    let foreground: BTreeSet<String> = nodes_by_id.keys().cloned().collect();
    for edge in edges_by_id.values_mut() {
        edge["context"] = json!(false);
    }
    let mut observed_nodes = foreground.clone();
    for edge in nearby_edges.values() {
        for field in ["source", "target"] {
            if let Some(id) = edge[field].as_str() {
                observed_nodes.insert(id.to_owned());
            }
        }
    }
    let observed_edges = nearby_edges.len();
    for (eid, mut edge) in nearby_edges {
        if edges_by_id.contains_key(&eid) {
            continue;
        }
        let from = edge["source"].as_str().unwrap_or_default().to_owned();
        let to = edge["target"].as_str().unwrap_or_default().to_owned();
        if !foreground.contains(&from) && !foreground.contains(&to) {
            continue;
        }
        let Some(from_node) = available(atlas, graph, &from) else {
            continue;
        };
        let Some(to_node) = available(atlas, graph, &to) else {
            continue;
        };
        let missing =
            usize::from(!nodes_by_id.contains_key(&from)) + usize::from(from != to && !nodes_by_id.contains_key(&to));
        if nodes_by_id.len() + missing > MAX_NODES || edges_by_id.len() >= MAX_EDGES {
            truncated = true;
            continue;
        }
        let context = !foreground.contains(&from) || !foreground.contains(&to);
        edge["context"] = json!(context);
        if context {
            edge["highlighted"] = json!(false);
            edge["anchor_id"] = json!(if foreground.contains(&from) { &from } else { &to });
            edge["direction"] = json!(if foreground.contains(&from) {
                "outgoing"
            } else {
                "incoming"
            });
        }
        nodes_by_id.entry(from).or_insert(from_node);
        nodes_by_id.entry(to).or_insert(to_node);
        edges_by_id.insert(eid, edge);
    }
    let omitted_nodes = observed_nodes.len().saturating_sub(nodes_by_id.len());
    let omitted_edges = observed_edges.saturating_sub(edges_by_id.len());
    truncated |= omitted_nodes > 0 || omitted_edges > 0;
    let counts = json!({"foreground_nodes":foreground.len(),"context_nodes":nodes_by_id.len()-foreground.len(),
        "shown_nodes":nodes_by_id.len(),"shown_edges":edges_by_id.len(),"observed_nodes":observed_nodes.len(),
        "observed_edges":observed_edges,"omitted_nodes":omitted_nodes,"omitted_edges":omitted_edges,"counts_exact":false});
    let rendered: Vec<Value> = nodes_by_id
        .values()
        .map(|node| json!({"id":node.id,"label":node.label,"kind":node.kind,"subkind":node_subkind(graph,node),"matched":foreground.contains(&node.id) && plan.focus.contains(&node.id),"context":!foreground.contains(&node.id)}))
        .collect();
    for result in &mut results {
        enrich_result(graph, result);
    }
    json!({"results":results,"graph":{"nodes":rendered,"edges":edges_by_id.into_values().collect::<Vec<_>>(),"counts":counts},"truncated":truncated})
}

pub async fn community(State(s): State<AppState>, Query(params): Query<CommunityParams>) -> ApiResult {
    let started = Instant::now();
    let s2 = s.clone();
    let focus = tokio::task::spawn_blocking(move || {
        let mut counts = BTreeMap::<String, usize>::new();
        for edge in s2.graph.edges() {
            if edge.relation.as_str() != "about_gene" || s2.graph.records_withheld(&edge.records).is_some() {
                continue;
            }
            if available(&s2.atlas, &s2.graph, &edge.to).is_some_and(|n| n.kind == NodeKind::Gene) {
                *counts.entry(edge.to.clone()).or_default() += 1;
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.into_iter().take(3).map(|(id, _)| id).collect::<Vec<_>>()
    })
    .await
    .map_err(internal)?;
    let limit = params.limit.unwrap_or(40).clamp(1, 40);
    let per_seed = limit.div_ceil(focus.len().max(1));
    let mut neighborhoods = Vec::new();
    // A shared store LIMIT sorts neither by seed nor by relevance. Execute each
    // root independently so a high-degree first root cannot exhaust another's
    // first hop or researcher frontier. Sequential calls stay within the store's
    // existing two-worker gate and retain its per-query bounds.
    for id in &focus {
        neighborhoods.push(
            search(
                State(s.clone()),
                HeaderMap::new(),
                Json(ExploreRequest {
                    query: String::new(),
                    limit: Some(per_seed),
                    mode: Some("community".into()),
                    plan: Some(Plan {
                        focus: vec![id.clone()],
                        intent: Intent::Researchers,
                        filters: Filters::default(),
                    }),
                }),
            )
            .await?
            .0,
        );
    }
    let mut response = merge_community(&focus, &neighborhoods, limit);
    response["execution"]["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    Ok(Json(response))
}

/// Shortest factual path through an already bounded, source-backed neighborhood.
/// Traversal is undirected for discovery; returned assertions retain direction.
fn community_paths(seed: &str, edges: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut queue = VecDeque::from([(seed.to_owned(), Vec::<Value>::new())]);
    let mut seen = BTreeSet::from([seed.to_owned()]);
    let mut paths = BTreeMap::new();
    while let Some((id, path)) = queue.pop_front() {
        paths.insert(id.clone(), path.clone());
        if path.len() >= 3 {
            continue;
        }
        for edge in edges {
            let a = edge["source"].as_str().unwrap_or("");
            let b = edge["target"].as_str().unwrap_or("");
            let next = if a == id {
                b
            } else if b == id {
                a
            } else {
                continue;
            };
            if seen.insert(next.to_owned()) {
                let mut next_path = path.clone();
                next_path.push(edge.clone());
                queue.push_back((next.to_owned(), next_path));
            }
        }
    }
    paths
}

fn merge_community(focus: &[String], neighborhoods: &[Value], limit: usize) -> Value {
    let mut response = neighborhoods
        .first()
        .cloned()
        .unwrap_or_else(|| json!({"execution":{"engine":"indexed-atlas"}}));
    let mut candidates = BTreeMap::<String, Value>::new();
    let mut occurrences = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut source_edges = Vec::<Vec<Value>>::new();
    for (index, body) in neighborhoods.iter().enumerate() {
        for node in body["graph"]["nodes"].as_array().into_iter().flatten() {
            if let Some(id) = node["id"].as_str() {
                candidates
                    .entry(id.to_owned())
                    .and_modify(|existing| {
                        if node["context"] == false {
                            existing["context"] = json!(false);
                        }
                    })
                    .or_insert_with(|| node.clone());
                occurrences.entry(id.to_owned()).or_default().insert(index);
            }
        }
        source_edges.push(body["graph"]["edges"].as_array().cloned().unwrap_or_default());
    }
    let mut queues = Vec::<VecDeque<Vec<Value>>>::new();
    for (index, seed) in focus.iter().enumerate() {
        let edges = &source_edges[index];
        let paths = community_paths(seed, edges);
        let mut queue = VecDeque::new();
        // Preserve actual shared-paper/grant bridges before the contextual
        // budget. Sharing an ID is never converted into a collaboration edge.
        for (id, roots) in &occurrences {
            if roots.len() > 1 && roots.contains(&index) && !focus.contains(id) {
                let path = paths.get(id).cloned().unwrap_or_default();
                if !path.is_empty() {
                    queue.push_back(path);
                }
            }
        }
        for result in neighborhoods[index]["results"].as_array().into_iter().flatten() {
            if let Some(id) = result["id"].as_str() {
                let source_path: Vec<Value> = result["edge_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|edge_id| edges.iter().find(|edge| edge["id"] == *edge_id).cloned())
                    .collect();
                let path = if source_path.len() == result["edge_ids"].as_array().map_or(0, Vec::len)
                    && !source_path.is_empty()
                {
                    source_path
                } else {
                    paths.get(id).cloned().unwrap_or_default()
                };
                if !path.is_empty() {
                    queue.push_back(path);
                }
            }
        }
        // Context remains attached to the seed by complete factual paths;
        // lexical edge order can never introduce a disconnected orphan.
        for edge in edges {
            for id in [edge["source"].as_str(), edge["target"].as_str()].into_iter().flatten() {
                let mut path = paths.get(id).cloned().unwrap_or_default();
                if id != seed && path.is_empty() {
                    continue;
                }
                if path.iter().all(|p| p["id"] != edge["id"]) {
                    path.push(edge.clone());
                }
                if !path.is_empty() {
                    queue.push_back(path);
                }
            }
        }
        queues.push(queue);
    }
    let mut nodes = BTreeMap::<String, Value>::new();
    let mut edges = BTreeMap::<String, Value>::new();
    let mut membership = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut edge_membership = BTreeMap::<String, BTreeSet<usize>>::new();
    for (index, seed) in focus.iter().enumerate() {
        if let Some(node) = candidates.get(seed) {
            nodes.insert(seed.clone(), node.clone());
            membership.entry(seed.clone()).or_default().insert(index);
        }
    }
    loop {
        let mut pending = false;
        for (index, queue) in queues.iter_mut().enumerate() {
            let Some(path) = queue.pop_front() else { continue };
            pending = true;
            let new_nodes: BTreeSet<&str> = path
                .iter()
                .flat_map(|edge| [edge["source"].as_str(), edge["target"].as_str()])
                .flatten()
                .filter(|id| !nodes.contains_key(*id))
                .collect();
            let new_edges = path
                .iter()
                .filter(|edge| !edges.contains_key(edge["id"].as_str().unwrap_or("")))
                .count();
            if nodes.len() + new_nodes.len() > MAX_NODES
                || edges.len() + new_edges > MAX_EDGES
                || new_nodes.iter().any(|id| !candidates.contains_key(*id))
            {
                continue;
            }
            for edge in path {
                let id = edge["id"].as_str().unwrap_or("").to_owned();
                edge_membership.entry(id.clone()).or_default().insert(index);
                for endpoint in ["source", "target"] {
                    let node_id = edge[endpoint].as_str().unwrap_or("").to_owned();
                    membership.entry(node_id.clone()).or_default().insert(index);
                    nodes
                        .entry(node_id.clone())
                        .or_insert_with(|| candidates[&node_id].clone());
                }
                edges
                    .entry(id)
                    .and_modify(|existing| {
                        if edge["context"] == false {
                            existing["context"] = json!(false);
                        }
                        if edge["highlighted"] == true {
                            existing["highlighted"] = json!(true);
                        }
                    })
                    .or_insert(edge);
            }
        }
        if !pending {
            break;
        }
    }
    let root_ids = |indexes: &BTreeSet<usize>| indexes.iter().map(|i| focus[*i].clone()).collect::<Vec<_>>();
    // Sampler ownership is not factual membership: one seed's query can return
    // a shared paper's assertion to another displayed seed. Recover memberships
    // from the retained graph itself, including its source-backed person paths.
    // Only the bounded shown graph is inspected; no global cluster is inferred.
    let retained: Vec<_> = edges.values().cloned().collect();
    for (index, seed) in focus.iter().enumerate() {
        for (id, path) in community_paths(seed, &retained) {
            if path.len() > 2 {
                continue;
            }
            membership.entry(id).or_default().insert(index);
            for edge in path {
                edge_membership
                    .entry(edge["id"].as_str().unwrap_or("").to_owned())
                    .or_default()
                    .insert(index);
            }
        }
        // Multiple directed assertions between the same nodes remain distinct.
        // Include every actual direct edge, even if BFS chose another predicate.
        for edge in &retained {
            if edge["source"] == *seed || edge["target"] == *seed {
                edge_membership
                    .entry(edge["id"].as_str().unwrap_or("").to_owned())
                    .or_default()
                    .insert(index);
            }
        }
    }
    for (id, node) in &mut nodes {
        node["community_seed_ids"] = json!(root_ids(&membership[id]));
        if focus.contains(id) {
            node["matched"] = json!(true);
            node["context"] = json!(false);
        }
    }
    for (id, edge) in &mut edges {
        edge["community_seed_ids"] = json!(root_ids(&edge_membership[id]));
    }
    let mut results = Vec::new();
    let mut result_ids = BTreeSet::new();
    let mut result_queues: Vec<VecDeque<Value>> = neighborhoods
        .iter()
        .map(|body| body["results"].as_array().cloned().unwrap_or_default().into())
        .collect();
    while results.len() < limit {
        let mut pending = false;
        for queue in &mut result_queues {
            let Some(mut result) = queue.pop_front() else { continue };
            pending = true;
            let id = result["id"].as_str().unwrap_or("").to_owned();
            let complete_path = result["edge_ids"].as_array().is_some_and(|path| {
                !path.is_empty()
                    && path
                        .iter()
                        .all(|id| id.as_str().is_some_and(|id| edges.contains_key(id)))
            });
            if complete_path && nodes.contains_key(&id) && result_ids.insert(id.clone()) && results.len() < limit {
                result["community_seed_ids"] = json!(root_ids(&membership[&id]));
                results.push(result);
            }
        }
        if !pending {
            break;
        }
    }
    let coverage: Vec<Value> = focus.iter().enumerate().map(|(index, seed)| {
        let selected: Vec<&Value> = edges.iter().filter(|(id, _)| edge_membership[*id].contains(&index)).map(|(_, edge)| edge).collect();
        let shown_nodes = membership.values().filter(|roots| roots.contains(&index)).count();
        let people = nodes.iter().filter(|(id, node)| membership[*id].contains(&index) && node["kind"] == "person").count();
        let omitted_edges = source_edges[index].iter().filter(|edge| !edges.contains_key(edge["id"].as_str().unwrap_or(""))).count();
        let omitted_nodes = neighborhoods[index]["graph"]["nodes"].as_array().into_iter().flatten().filter(|node| !nodes.contains_key(node["id"].as_str().unwrap_or(""))).count();
        json!({"id":seed,"shown_nodes":shown_nodes,"shown_edges":selected.len(),"direct_edges":selected.iter().filter(|edge| edge["source"] == *seed || edge["target"] == *seed).count(),"people":people,
            "available_nodes":neighborhoods[index]["graph"]["nodes"].as_array().map_or(0,Vec::len),"available_edges":source_edges[index].len(),
            "available_scope":"bounded_per_seed_response","omitted_nodes":omitted_nodes,"omitted_edges":omitted_edges,
            "store_query_limit":MAX_EDGES,"frontier_seed_limit":16,"truncated":neighborhoods[index]["execution"]["truncated"].as_bool().unwrap_or(false) || omitted_edges > 0 || omitted_nodes > 0,"counts_exact":false})
    }).collect();
    let shared: Vec<Value> = membership
        .iter()
        .filter(|(id, roots)| roots.len() > 1 && !focus.contains(id))
        .map(|(id, roots)| json!({"id":id,"seed_ids":root_ids(roots)}))
        .collect();
    let foreground = nodes.values().filter(|node| node["context"] != true).count();
    response["graph"] = json!({"counts":{"shown_nodes":nodes.len(),"shown_edges":edges.len(),"foreground_nodes":foreground,"context_nodes":nodes.len()-foreground,"counts_exact":false},
        "community":{"strategy":"per_seed_round_robin","scope":"bounded_research_neighborhoods","node_limit":MAX_NODES,"edge_limit":MAX_EDGES,"seeds":coverage,"shared_nodes":shared},"nodes":nodes.into_values().collect::<Vec<_>>(),"edges":edges.into_values().collect::<Vec<_>>()});
    response["results"] = json!(results);
    response["plan"]["focus"] = json!(focus);
    response["interpretation"]["entities"] = json!(
        neighborhoods
            .iter()
            .flat_map(|body| body["interpretation"]["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned())
            .collect::<Vec<_>>()
    );
    let queries: Vec<Value> = neighborhoods
        .iter()
        .zip(focus)
        .flat_map(|(body, seed)| {
            body["execution"]["queries"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned()
                .map(move |mut query| {
                    query["community_seed_id"] = json!(seed);
                    query
                })
        })
        .collect();
    response["execution"]["sparql"] = queries
        .first()
        .map(|query| query["sparql"].clone())
        .unwrap_or(Value::Null);
    response["execution"]["queries"] = json!(queries);
    response["execution"]["truncated"] = json!(coverage.iter().any(|seed| seed["truncated"] == true));
    response["community_scope"] = json!(
        "Bounded source-backed research neighborhoods, sampled independently for each of three genes with the most about-gene research assertions; shared nodes retain their actual source identities."
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_failure_retains_safe_diagnostics_without_claiming_an_executed_answer() {
        let error = routing_error(crate::explore_failover::RoutingFailure {
            code: "model_rate_limited",
            diagnostics: json!({"status":"not_executed","linked_ids":["MONDO:0010726"],"routing":{"reason":"keyword_unsupported","attempted":[{"status":"skipped_cooldown"}]},"question_status":"not_checked"}),
        });
        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["params"]["linked_ids"], json!(["MONDO:0010726"]));
        assert_eq!(
            error.1["params"]["routing"]["attempted"][0]["status"],
            "skipped_cooldown"
        );
        assert_eq!(error.1["params"]["status"], "not_executed");
        assert!(error.1.get("results").is_none());
    }

    #[test]
    fn public_model_error_never_returns_provider_quota_payload_or_account_metadata() {
        let error = model_error("rate limited: {\"error\":{\"code\":429,\"message\":\"free-models-per-day remaining 0 upgrade\",\"metadata\":{\"user_id\":\"private-provider-user\",\"key\":\"private-key\"}}}".into());
        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1["code"], "model_rate_limited");
        let public = error.1.to_string();
        for secret in [
            "private-provider-user",
            "private-key",
            "metadata",
            "upgrade",
            "free-models-per-day",
        ] {
            assert!(!public.contains(secret));
        }
    }

    #[test]
    fn community_balances_three_roots_preserves_paths_and_shared_identity() {
        let seeds = vec![
            "gene:large".to_owned(),
            "gene:small".to_owned(),
            "gene:third".to_owned(),
        ];
        let bodies: Vec<Value> = seeds.iter().enumerate().map(|(index, seed)| {
            let n = if index == 0 { 90 } else { 6 };
            let mut nodes = vec![json!({"id":seed,"kind":"gene","context":false})];
            let mut edges = vec![];
            let mut results = vec![];
            for person in 0..n {
                let paper = if person == 0 { "paper:shared".to_owned() } else { format!("paper:{index}:{person}") };
                let id = format!("person:{index}:{person}");
                nodes.push(json!({"id":paper,"kind":"paper","context":false}));
                nodes.push(json!({"id":id,"kind":"person","label":"Same name deliberately different IDs","context":false}));
                let a = format!("{paper}|about_gene|{seed}");
                let b = format!("{id}|author_of|{paper}");
                edges.push(json!({"id":a,"source":paper,"target":seed,"relation":"about_gene","highlighted":true,"evidence":[{"record":format!("fixture-{index}-{person}") }]}));
                edges.push(json!({"id":b,"source":id,"target":paper,"relation":"author_of","highlighted":true,"evidence":[{"record":format!("fixture-{index}-{person}") }]}));
                results.push(json!({"id":id,"edge_ids":[a,b]}));
            }
            let sparql = format!("SELECT ?s WHERE {{ VALUES ?s {{ <https://example.invalid/seed/{index}> }} }} LIMIT 1");
            json!({"graph":{"nodes":nodes,"edges":edges},"results":results,"interpretation":{"entities":[{"id":seed}]},"plan":{"focus":[seed]},"execution":{"engine":"nrese","sparql":sparql,"queries":[{"seed_ids":[seed],"stage":"neighborhood","sparql":sparql,"engine":"nrese","row_count":n*2,"limit":MAX_EDGES}],"truncated":false}})
        }).collect();
        let merged = merge_community(&seeds, &bodies, 18);
        let nodes = merged["graph"]["nodes"].as_array().unwrap();
        let edges = merged["graph"]["edges"].as_array().unwrap();
        assert!(nodes.len() <= MAX_NODES && edges.len() <= MAX_EDGES);
        for (index, seed) in seeds.iter().enumerate() {
            let coverage = &merged["graph"]["community"]["seeds"][index];
            assert!(
                coverage["direct_edges"].as_u64().unwrap() >= 6,
                "each root gets actual direct edges"
            );
            assert!(
                coverage["people"].as_u64().unwrap() >= 6,
                "each root gets complete person paths"
            );
            assert!(nodes.iter().any(|n| n["id"] == *seed && n["matched"] == true));
            assert_eq!(
                coverage["direct_edges"].as_u64().unwrap() as usize,
                edges
                    .iter()
                    .filter(|edge| edge["source"] == *seed || edge["target"] == *seed)
                    .count()
            );
        }
        assert_eq!(nodes.iter().filter(|n| n["id"] == "paper:shared").count(), 1);
        assert_eq!(
            merged["graph"]["community"]["shared_nodes"][0]["seed_ids"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n["label"] == "Same name deliberately different IDs")
                .count(),
            nodes.iter().filter(|n| n["kind"] == "person").count()
        );
        for edge in edges {
            assert!(nodes.iter().any(|n| n["id"] == edge["source"]));
            assert!(nodes.iter().any(|n| n["id"] == edge["target"]));
            assert!(!edge["evidence"].as_array().unwrap().is_empty());
        }
        for result in merged["results"].as_array().unwrap() {
            assert!(
                result["edge_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|id| edges.iter().any(|edge| edge["id"] == *id))
            );
        }
        assert_eq!(merged["graph"]["community"]["seeds"][0]["truncated"], true);
        let queries = merged["execution"]["queries"].as_array().unwrap();
        assert_eq!(queries.len(), 3);
        for (index, query) in queries.iter().enumerate() {
            assert_eq!(query["community_seed_id"], seeds[index]);
            assert_eq!(
                query["row_count"],
                bodies[index]["execution"]["queries"][0]["row_count"]
            );
            assert_eq!(query["sparql"].as_str().unwrap().matches("SELECT").count(), 1);
        }
        assert_eq!(merged["execution"]["sparql"], queries[0]["sparql"]);
    }

    #[test]
    fn community_membership_uses_actual_shared_adjacency_not_query_ownership() {
        let seeds = vec!["gene:a".to_owned(), "gene:b".to_owned()];
        let first = json!({"graph":{"nodes":[{"id":"gene:a","kind":"gene"},{"id":"gene:b","kind":"gene"},{"id":"paper:shared","kind":"paper"}],
            "edges":[{"id":"a","source":"paper:shared","target":"gene:a","evidence":[{"record":"fixture"}]},{"id":"b","source":"paper:shared","target":"gene:b","evidence":[{"record":"fixture"}]}]},"execution":{"truncated":false}});
        let second =
            json!({"graph":{"nodes":[{"id":"gene:b","kind":"gene"}],"edges":[]},"execution":{"truncated":false}});
        let merged = merge_community(&seeds, &[first, second], 10);
        assert_eq!(merged["graph"]["community"]["seeds"][1]["direct_edges"], 1);
        assert_eq!(merged["graph"]["community"]["shared_nodes"][0]["id"], "paper:shared");
        assert_eq!(
            merged["graph"]["community"]["shared_nodes"][0]["seed_ids"],
            json!(seeds)
        );
        assert_eq!(merged["execution"]["sparql"], Value::Null);
    }

    #[test]
    fn search_language_is_explicit_bounded_and_defaults_to_english() {
        let mut headers = HeaderMap::new();
        assert_eq!(search_language(&headers).ok(), Some("en"));
        headers.insert("x-atlas-lang", "de".parse().unwrap());
        assert_eq!(search_language(&headers).ok(), Some("de"));
        headers.insert("x-atlas-lang", "invalid".parse().unwrap());
        assert!(search_language(&headers).is_err());
    }

    #[test]
    fn actionable_metadata_keeps_source_status_and_official_action_destination() {
        use atlas_core::graph::{Initiative, OfficialAction, OrgKind, Organisation};
        let (_, graph) = fixture();
        let mut data = graph.data().clone();
        data.orgs.push(Organisation {
            id: "org:fixture".into(),
            name: "Synthetic organisation".into(),
            kind: OrgKind::PatientGroup,
            url: Some("https://example.invalid/generic".into()),
            contact_url: None,
            country: None,
            country_basis: None,
            description: None,
            languages: vec![],
            verified_on: None,
            channels: vec![],
            records: vec![0],
        });
        data.initiatives.push(Initiative {
            id: "org:fixture".into(),
            verified: true,
            records: vec![0],
            actions: vec![OfficialAction {
                action: "View official registration".into(),
                url: "https://example.invalid/actual-action".into(),
                audience: "families".into(),
                records: vec![0],
                ..Default::default()
            }],
            ..Default::default()
        });
        let graph = Graph::new(data);
        let mut study = json!({"id":"NCT00000002"});
        enrich_result(&graph, &mut study);
        assert_eq!(study["status"], "COMPLETED");
        assert!(
            study["facts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["key"] == "country" && f["value"] == "France")
        );
        assert_eq!(study["metadata_evidence"][0]["record"], "fixture.json#L1");
        let mut org = json!({"id":"org:fixture","url":"https://example.invalid/generic"});
        enrich_result(&graph, &mut org);
        assert_eq!(org["url"], "https://example.invalid/actual-action");
        assert_eq!(org["official_action"]["url"], org["url"]);
        let mut person = json!({"id":"person:fixture"});
        enrich_result(&graph, &mut person);
        assert!(person["facts"].as_array().unwrap().is_empty());
        assert!(person.get("role").is_none());
        let mut unknown = json!({"id":"person:unavailable"});
        enrich_result(&graph, &mut unknown);
        assert!(unknown.get("facts").is_none());
    }

    #[test]
    fn canonical_aggregate_preserves_rows_and_input_without_inventing_graph_results() {
        let state = crate::test_support::state();
        let linked = vec![available(&state.atlas, &state.graph, "HGNC:11444").unwrap()];
        let execution = json!({"answer":{"results":[{"data":{"results":{"bindings":[{"count":{"type":"literal","value":"0"}}]}}}]}});
        let outcome = crate::explore_query::QueryOutcome {
            query_execution: execution.clone(),
            result_ids: vec![],
            triples: vec![],
            backend: "nrese".into(),
            sparql: Some("SELECT (COUNT(*) AS ?count) WHERE {}".into()),
            elapsed_ms: 1,
            truncated: false,
            model: None,
            connection: None,
            activities: vec![],
            reasoning_proofs: vec![],
            reasoning_warning: None,
        };
        let response = canonical_response(&state, "count STXBP1 studies", &linked, outcome, 20);
        assert_eq!(response["query_execution"], execution);
        assert!(response["results"].as_array().unwrap().is_empty());
        assert!(response["graph"]["edges"].as_array().unwrap().is_empty());
        assert_eq!(response["graph"]["nodes"][0]["label"], "STXBP1");
        assert_eq!(response["graph"]["nodes"][0]["input"], true);
    }

    #[test]
    fn removed_context_identifier_is_rejected_without_rebuilding_the_graph() {
        use atlas_core::withhold::{KeyKind, SuppressionEntry};
        let mut state = crate::test_support::state();
        let data = tempfile::tempdir().unwrap();
        state.withhold = crate::privacy::WithholdState::new(data.path().to_path_buf(), state.graph.clone());
        let id = "NCTTEST0";
        assert!(validated_sparql_context(&state, &[id.into()]).is_ok());
        let salt = atlas_ingest::withhold::salt_from_env();
        atlas_ingest::withhold::append_suppression(data.path(), SuppressionEntry {
            id: "sup_context_fixture".into(), keys: vec![salt.key(KeyKind::Node, id).unwrap()],
            scope: Default::default(), reason: "gdpr_art17_erasure".into(),
            date: "2026-10-04T00:00:00Z".into(), reviewer: "fixture".into(), request: None,
            salt_id: salt.id().into(),
        }).unwrap();
        state.withhold.reload();
        assert!(available(&state.atlas, &state.graph, id).is_some(), "immutable source node is unchanged");
        assert!(validated_sparql_context(&state, &[id.into()]).is_err(), "runtime removal also applies to query context");
    }

    #[test]
    fn sparql_semantic_context_is_bounded_known_and_distinct_from_execution_seeds() {
        let state = crate::test_support::state();
        let request: SparqlRequest = serde_json::from_value(json!({
            "sparql":"SELECT * WHERE {}", "focus":["HGNC:11444"],
            "semantic_focus":["MONDO:9999999"], "limit":160, "reasoning":false,
            "query":"reviewed-file.txt"
        }))
        .unwrap();
        let execution = validated_sparql_context(&state, &request.focus)
            .ok()
            .expect("known execution focus");
        let semantic = validated_sparql_context(&state, request.semantic_focus.as_deref().unwrap())
            .ok()
            .expect("known semantic focus");
        assert_eq!(execution[0].id, "HGNC:11444");
        assert_eq!(semantic[0].id, "MONDO:9999999");
        assert_eq!(request.sparql, "SELECT * WHERE {}");
        assert_eq!(request.query, "reviewed-file.txt");
        for invalid in [
            vec!["HGNC:unknown".into()],
            vec!["STXBP1".into()],
            vec![" HGNC:11444 ".into()],
            vec!["HGNC:11444".into(); 2],
            vec![String::new()],
            vec!["x".repeat(257)],
            vec!["HGNC:11444".into(); 17],
        ] {
            assert!(validated_sparql_context(&state, &invalid).is_err());
        }
        let legacy: SparqlRequest = serde_json::from_value(json!({"sparql":"SELECT * WHERE {}"})).unwrap();
        assert!(legacy.semantic_focus.is_none());
        assert!(
            serde_json::from_value::<SparqlRequest>(
                json!({"sparql":"SELECT * WHERE {}", "semantic_focus":[{"id":"MONDO:9999999","label":"Untrusted"}]})
            )
            .is_err()
        );
    }

    #[test]
    fn canonical_proof_keeps_exact_source_fields_and_missing_source_dates() {
        let iri = explore_sparql::node_iri("HGNC:11444");
        let execution = json!({"answer":{"results":[{"provenance":[{
            "item":{"value":iri},"record":{"value":"urn:fixture:record"},
            "url":{"value":"https://example.invalid/source"},"hash":{"value":"actual-record-hash"}
        }]}]}});
        let evidence = canonical_evidence(&execution, "HGNC:11444", false);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0]["source"], "urn:fixture:record");
        assert_eq!(evidence[0]["sha256"], "actual-record-hash");
        assert!(evidence[0].get("retrieved_at").is_none());
        assert!(canonical_evidence(&execution, "HGNC:other", false).is_empty());
    }
    use atlas_core::DiseaseIdentity;
    use atlas_core::disease::Disease;
    use atlas_core::evidence::GeneLink;
    use atlas_core::graph::{
        EntityLicence, GraphData, LicenceClass, LinkLevel, Paper, Person, PersonSource, RecordHash, SourceRecord,
        Study, StudyKind,
    };
    use atlas_core::node::EdgeKind;
    use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, Provenance, SourceEntity};

    fn fixture() -> (Atlas, Graph) {
        let entity = SourceEntity {
            id: "source:fixture".into(),
            url: "https://example.org/fixture".into(),
            file: "fixture.json".into(),
            version: Some("synthetic-test-v1".into()),
            retrieved_at: Some("2026-10-04".into()),
            sha256: Some("a".repeat(64)),
            bytes: 100,
            licence: Some("CC0".into()),
        };
        let provenance = Provenance {
            entities: vec![entity],
            activities: vec![],
        };
        let record = RecordRef::line(EntityIdx(0), 1);
        let mut disease = Disease::new("MONDO:0012812", ActivityIdx(0));
        disease.name = "STXBP1 encephalopathy".into();
        disease.derived_from.push(record.clone());
        disease.genes.push(GeneLink {
            symbol: "STXBP1".into(),
            association: "Disease-causing germline mutation(s) in".into(),
            source: "fixture".into(),
            source_disease: disease.id.clone(),
            pmids: vec![],
            assessed: Some(true),
            hgnc: Some("HGNC:11444".into()),
            ncbi_gene: None,
            record,
        });
        let atlas = Atlas::new(vec![], DiseaseIdentity::default(), provenance.clone(), vec![disease]);
        let paper = Paper {
            id: "PMID:123".into(),
            title: "Synthetic STXBP1 paper".into(),
            journal: "Fixture".into(),
            year: Some(2026),
            doi: None,
            review: false,
            records: vec![0],
        };
        let person = Person {
            id: "person:fixture".into(),
            name: "Synthetic Researcher".into(),
            name_variants: vec![],
            orcids: vec![],
            affiliations: vec![],
            source: PersonSource::Resolved,
            genes: vec![],
            communities: vec![],
            cross_community: false,
            matched_by: vec![],
            merge_basis: vec![],
            records: vec![0],
        };
        let study = |id: &str, country: &str, status: &str| Study {
            id: id.into(),
            title: format!("Synthetic study {id}"),
            status: status.into(),
            kind: StudyKind::Trial,
            phases: vec![],
            sponsor: "fixture".into(),
            sponsor_class: "OTHER".into(),
            start: String::new(),
            completion: String::new(),
            enrollment: None,
            countries: vec![country.into()],
            interventions: vec![],
            record: 0,
        };
        let edge = |from: &str, relation: Relation, to: &str| GraphEdge {
            from: from.into(),
            relation,
            to: to.into(),
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: "Synthetic source assertion".into(),
            activity: ActivityIdx(0),
            records: vec![0],
        };
        let graph = Graph::new(GraphData {
            provenance,
            records: vec![SourceRecord {
                entity: EntityIdx(0),
                locator: Locator::Line(1),
                id: "fixture".into(),
                url: Some("https://example.org/fixture/record".into()),
                fetched_at: Some("2026-10-04".into()),
                hash: RecordHash::JsonLine,
                sha256: [1; 32],
            }],
            licences: vec![EntityLicence {
                entity: EntityIdx(0),
                licence: "CC0".into(),
                class: LicenceClass::Open,
            }],
            papers: vec![paper],
            people: vec![person],
            studies: vec![
                study("NCT00000001", "Germany", "RECRUITING"),
                study("NCT00000002", "France", "COMPLETED"),
            ],
            edges: vec![
                edge("PMID:123", Relation::AboutGene, "HGNC:11444"),
                edge("person:fixture", Relation::AuthorOf, "PMID:123"),
                edge("NCT00000001", Relation::NamesGene, "HGNC:11444"),
                edge("NCT00000002", Relation::NamesGene, "HGNC:11444"),
            ],
            ..GraphData::default()
        });
        (atlas, graph)
    }

    #[test]
    fn exact_entities_in_questions_and_no_unrelated_fallback() {
        let (atlas, graph) = fixture();
        let index = ConnectedIndex::new(&graph);
        let (entities, intent, _) = local_understanding(&atlas, &graph, &index, "Find researchers studying STXBP1");
        assert!(entities.iter().any(|n| n.id == "HGNC:11444"));
        assert!(
            local_understanding(&atlas, &graph, &index, "Meine Tochter hat eine STXBP1-Mutation")
                .0
                .iter()
                .any(|n| n.id == "HGNC:11444")
        );
        assert_eq!(intent, Intent::Researchers);
        assert!(
            local_understanding(&atlas, &graph, &index, "zzzzzzzunresolvable9999999")
                .0
                .is_empty()
        );
    }

    #[test]
    fn prose_stopwords_do_not_become_gene_or_disease_acronyms() {
        let (atlas, graph) = fixture();
        let mut diseases = atlas.diseases().to_vec();
        let mut alias = Disease::new("MONDO:0000001", ActivityIdx(0));
        alias.name = "Synthetic acronym condition".into();
        alias.synonyms = vec![
            atlas_core::disease::Name {
                text: "CAN".into(),
                kind: Some("ABBREVIATION".into()),
            },
            atlas_core::disease::Name {
                text: "HAS".into(),
                kind: Some("ABBREVIATION".into()),
            },
        ];
        diseases.push(alias);
        let atlas = Atlas::new(vec![], DiseaseIdentity::default(), atlas.provenance.clone(), diseases);
        let index = ConnectedIndex::new(&graph);
        let (entities, _, _) = local_understanding(
            &atlas,
            &graph,
            &index,
            "My child has STXBP1. Which communities can we contact for support?",
        );
        assert!(entities.iter().any(|n| n.id == "HGNC:11444"));
        assert!(!entities.iter().any(|n| n.id == "MONDO:0000001"));
        assert!(
            local_understanding(&atlas, &graph, &index, "CAN")
                .0
                .iter()
                .any(|n| n.id == "MONDO:0000001"),
            "a deliberate acronym-only search remains usable"
        );
    }

    #[test]
    fn rank_prose_verb_does_not_anchor_gene_but_explicit_rank_identity_still_resolves() {
        let (atlas, graph) = fixture();
        let mut diseases = atlas.diseases().to_vec();
        let mut disease = diseases[0].clone();
        disease.id = "MONDO:9999998".into();
        disease.name = "Synthetic RANK gene fixture".into();
        disease.genes[0].symbol = "TNFRSF11A".into();
        disease.genes[0].hgnc = Some("HGNC:11908".into());
        diseases.push(disease);
        let atlas = Atlas::new(vec![], DiseaseIdentity::default(), atlas.provenance.clone(), diseases);
        let mut data = graph.data().clone();
        data.gene_aliases.push(atlas_core::graph::GeneAlias {
            hgnc: "HGNC:11908".into(),
            symbol: "TNFRSF11A".into(),
            name: "Synthetic RANK alias fixture".into(),
            aliases: vec!["RANK".into()],
            previous: vec![],
            record: 0,
        });
        let graph = Graph::new(data);
        let index = ConnectedIndex::new(&graph);
        let entities = local_understanding(
            &atlas,
            &graph,
            &index,
            "Rank the research opportunities involving toxic protein accumulation",
        )
        .0;
        assert!(!entities.iter().any(|n| n.id == "HGNC:11908"));
        for explicit in ["RANK", "HGNC:11908", "TNFRSF11A"] {
            assert!(
                local_understanding(&atlas, &graph, &index, explicit)
                    .0
                    .iter()
                    .any(|n| n.id == "HGNC:11908"),
                "{explicit}"
            );
        }
    }

    #[test]
    fn longest_condition_name_suppresses_nested_type_alias_and_parent_name() {
        let (atlas, graph) = fixture();
        let mut diseases = atlas.diseases().to_vec();
        let mut gene_source = diseases[0].clone();
        gene_source.id = "MONDO:0000001".into();
        gene_source.name = "Synthetic gamma-sarcoglycan condition".into();
        gene_source.genes[0].symbol = "SGCG".into();
        gene_source.genes[0].hgnc = Some("HGNC:10809".into());
        diseases.push(gene_source);
        for (id, name) in [
            ("MONDO:0000002", "Synthetic disease"),
            ("MONDO:0000003", "Synthetic disease type C"),
        ] {
            let mut d = Disease::new(id, ActivityIdx(0));
            d.name = name.into();
            diseases.push(d);
        }
        let atlas = Atlas::new(vec![], DiseaseIdentity::default(), atlas.provenance.clone(), diseases);
        let mut data = graph.data().clone();
        data.gene_aliases.push(atlas_core::graph::GeneAlias {
            hgnc: "HGNC:10809".into(),
            symbol: "SGCG".into(),
            name: "Synthetic gene alias fixture".into(),
            aliases: vec!["TYPE".into()],
            previous: vec![],
            record: 0,
        });
        let graph = Graph::new(data);
        let index = ConnectedIndex::new(&graph);
        let (entities, _, _) = local_understanding(
            &atlas,
            &graph,
            &index,
            "Which studies already exist for Synthetic disease type C and how can we participate?",
        );
        assert_eq!(
            entities.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["MONDO:0000003"]
        );
        assert!(
            local_understanding(&atlas, &graph, &index, "TYPE")
                .0
                .iter()
                .any(|n| n.id == "HGNC:10809")
        );
    }

    #[test]
    fn researcher_results_follow_real_two_hop_assertions_and_provenance() {
        let (atlas, graph) = fixture();
        let plan = Plan {
            focus: vec!["HGNC:11444".into()],
            intent: Intent::Researchers,
            filters: Filters::default(),
        };
        let result = retrieve(&atlas, &graph, &plan, 20, None);
        assert_eq!(result["results"][0]["id"], "person:fixture");
        assert_eq!(result["results"][0]["edge_ids"].as_array().unwrap().len(), 2);
        assert!(
            result["results"][0]["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["sha256"].as_str().unwrap().len() == 64)
        );
        let nodes: BTreeSet<&str> = result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        for edge in result["graph"]["edges"].as_array().unwrap() {
            assert!(nodes.contains(edge["source"].as_str().unwrap()));
            assert!(nodes.contains(edge["target"].as_str().unwrap()));
        }
    }

    #[test]
    fn explicit_filters_are_applied_and_edited_focus_is_validated() {
        let (atlas, graph) = fixture();
        let mut plan = Plan {
            focus: vec!["HGNC:11444".into()],
            intent: Intent::Studies,
            filters: Filters {
                country: Some("Germany".into()),
                recruiting: Some(true),
                kind: Some("trial".into()),
            },
        };
        assert!(validate_plan(&atlas, &graph, &plan).is_ok());
        let result = retrieve(&atlas, &graph, &plan, 20, None);
        assert_eq!(result["results"].as_array().unwrap().len(), 1);
        assert_eq!(result["results"][0]["id"], "NCT00000001");
        plan.focus.push("HGNC:invented".into());
        assert!(validate_plan(&atlas, &graph, &plan).is_err());
        plan.focus.pop();
        plan.intent = Intent::Researchers;
        assert!(validate_plan(&atlas, &graph, &plan).is_err());
    }

    #[test]
    fn configured_store_results_control_retrieval_and_unknown_assertions_are_not_fabricated() {
        let (atlas, graph) = fixture();
        let plan = Plan {
            focus: vec!["HGNC:11444".into()],
            intent: Intent::Studies,
            filters: Filters::default(),
        };
        let triples = vec![
            explore_sparql::SparqlTriple {
                from: "NCT00000001".into(),
                relation: "names_gene".into(),
                to: "HGNC:11444".into(),
            },
            explore_sparql::SparqlTriple {
                from: "NCT99999999".into(),
                relation: "names_gene".into(),
                to: "HGNC:11444".into(),
            },
        ];
        let result = retrieve(&atlas, &graph, &plan, 20, Some(&triples));
        assert_eq!(result["results"].as_array().unwrap().len(), 1);
        assert_eq!(result["results"][0]["id"], "NCT00000001");
        assert_eq!(result["graph"]["edges"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn schema_accepts_supported_jobs_and_relations_compile_read_only() {
        for intent in [
            Intent::Conditions,
            Intent::Researchers,
            Intent::Studies,
            Intent::Papers,
            Intent::Funding,
            Intent::Models,
            Intent::Therapies,
            Intent::Resources,
            Intent::Groups,
            Intent::Outcomes,
        ] {
            let query = explore_sparql::compile_query(&["HGNC:11444".into()], &relation_scope(intent), 160).unwrap();
            assert!(query.starts_with("PREFIX ra:"));
            assert!(query.contains("VALUES"));
            assert!(!query.contains("SERVICE"));
        }
        assert!(atlas_llm::check_json(&understanding_schema().schema,r#"{"terms":["STXBP1"],"excluded_terms":[],"intent":"models","filters":{"country":null,"recruiting":null,"kind":"cell_line"}}"#).is_ok());
    }

    #[test]
    fn local_country_and_recruiting_filters_survive_provider_unavailability() {
        let (_, graph) = fixture();
        let index = ConnectedIndex::new(&graph);
        let filters = local_filters(&index, "recruiting STXBP1 studies in Germany");
        assert_eq!(filters.country.as_deref(), Some("Germany"));
        assert_eq!(filters.recruiting, Some(true));
    }

    #[test]
    #[ignore = "requires existing local atlas and matching graph snapshots"]
    fn real_snapshot_specific_question_linking() {
        let data = atlas_ingest::data_dir();
        let atlas = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))
            .expect("read existing atlas snapshot")
            .0;
        let graph = atlas_core::snapshot::load_graph(&atlas_ingest::graph::snapshot_path(&data))
            .expect("read existing graph snapshot")
            .0;
        // Match search()'s first clinical pass: the connected label index is used
        // only when this pass finds no clinical entity. Generic asset labels
        // (e.g. "Experimental") do not contaminate a known disease's question.
        let clinical_index = ConnectedIndex {
            labels: vec![],
            countries: BTreeSet::new(),
        };
        let family_question = "My child has been diagnosed with KIF1A-associated neurological disorder. Which patient organizations can we contact, and which related communities could offer support if there is no local group?";
        let linked = local_understanding(&atlas, &graph, &clinical_index, family_question).0;
        assert!(linked.iter().any(|n| n.id == "HGNC:888"));
        assert!(
            linked
                .iter()
                .all(|n| !["HGNC:8064", "MONDO:0012833", "MONDO:0019395"].contains(&n.id.as_str())),
            "ordinary words must not become unrelated alias targets"
        );
        let npc = "Which patient registries, natural history studies, experimental models, and biomarkers already exist for Niemann–Pick disease type C? Could related disease communities reuse them, and what would need validation first?";
        let linked = local_understanding(&atlas, &graph, &clinical_index, npc).0;
        assert_eq!(
            linked.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["MONDO:0018982"],
            "prefer specific disease name over nested TYPE alias and broader disease name"
        );
    }

    #[test]
    #[ignore = "requires existing local atlas and matching graph snapshots"]
    fn real_snapshot_human_gene_focus_and_source_backed_research_paths() {
        let data = atlas_ingest::data_dir();
        let atlas = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))
            .expect("read existing atlas snapshot")
            .0;
        let graph = atlas_core::snapshot::load_graph(&atlas_ingest::graph::snapshot_path(&data))
            .expect("read existing graph snapshot")
            .0;
        let index = ConnectedIndex::new(&graph);
        let entities = link(&atlas, &graph, &index, "STXBP1");
        assert_eq!(
            entities.len(),
            1,
            "human gene must not be ambiguous with organism labels"
        );
        assert_eq!(entities[0].id, "HGNC:11444");
        let mut plan = Plan {
            focus: vec![entities[0].id.clone()],
            intent: Intent::All,
            filters: Filters::default(),
        };
        let result = retrieve(&atlas, &graph, &plan, 20, None);
        assert!(
            result["results"]
                .as_array()
                .unwrap()
                .iter()
                .take(4)
                .any(|node| node["id"] == "MONDO:0012812"),
            "causal single-gene DEE4 must be near the top"
        );
        plan.intent = Intent::Researchers;
        let result = retrieve(&atlas, &graph, &plan, 20, None);
        let results = result["results"].as_array().unwrap();
        assert!(
            !results.is_empty(),
            "existing real research community must remain reachable"
        );
        assert!(
            results
                .iter()
                .all(|node| node["kind"] == "person" && !node["evidence"].as_array().unwrap().is_empty())
        );
        assert!(result["graph"]["nodes"].as_array().unwrap().len() <= MAX_NODES);
        assert!(result["graph"]["edges"].as_array().unwrap().len() <= MAX_EDGES);
    }
}
