//! D43 in the server: the runtime withholding filter over every `/api` response, the privacy
//! request host actions (trace + suppression write) and the `trace` CLI.
//!
//! The graph build already drops suppressed people (atlas-ingest). Between an approval and the
//! next build, the runtime filter withholds them from every JSON and Turtle response: an object
//! that holds a withheld node id, contact name or e-mail as a value is removed, as is an array
//! element or edge id (`a|relation|b`) naming one. A response that is itself withheld becomes 404.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use atlas_contrib::privacy::{PrivacyActions, SuppressOrder, TraceParams};
use atlas_core::Graph;
use atlas_core::withhold::{KeyKind, Salt, SuppressionEntry, Withhold};
use atlas_ingest::trace::{TraceOptions, TraceQuery, TraceReport, trace};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

/// Values withheld from responses (clear text, in memory only, never logged).
#[derive(Debug, Default)]
struct Tokens {
    set: HashSet<String>,
    closed: bool,
}

pub struct WithholdState {
    data: PathBuf,
    salt: Salt,
    graph: Arc<Graph>,
    tokens: RwLock<Arc<Tokens>>,
    /// Serialises appends to `data/suppression.json`.
    write: std::sync::Mutex<()>,
}

pub type SharedWithhold = Arc<WithholdState>;

fn tokens_for(graph: &Graph, w: &Withhold) -> Tokens {
    let mut t = Tokens {
        closed: w.closed_reason().is_some(),
        ..Tokens::default()
    };
    for kind in [
        atlas_core::node::NodeKind::Study,
        atlas_core::node::NodeKind::Grant,
        atlas_core::node::NodeKind::Paper,
        atlas_core::node::NodeKind::Person,
        atlas_core::node::NodeKind::Organisation,
        atlas_core::node::NodeKind::Asset,
    ] {
        for idx in 0..graph.node_count(kind) as u32 {
            let key = atlas_core::node::NodeKey { kind, idx };
            if w.node(graph, key).is_some() {
                let id = graph.node_ref(key).id;
                t.set
                    .insert(crate::export::iri(&id).trim_matches(['<', '>']).to_owned());
                t.set.insert(id);
                if kind == atlas_core::node::NodeKind::Person {
                    let person = graph.person(idx);
                    t.set.insert(person.name.clone());
                    t.set.extend(person.name_variants.iter().cloned());
                    t.set.extend(person.orcids.iter().cloned());
                }
            }
        }
    }
    for (idx, edge) in graph.edges().iter().enumerate() {
        if w.edge(graph, idx as u32).is_some() {
            t.set.insert(edge.id());
        }
    }
    // An action can be quarantined after startup while its initiative remains
    // valid. Hide that URL until the next graph rebuild refreshes its metadata.
    for initiative in &graph.data().initiatives {
        for action in &initiative.actions {
            if w.records(graph.data(), &action.records).is_some() {
                t.set.insert(action.url.clone());
            }
        }
        for action in &initiative.reported_actions {
            if w.records(graph.data(), &action.records).is_some() {
                t.set.insert(action.url.clone());
            }
        }
    }
    for c in &graph.data().contacts {
        for x in &c.central {
            if w.contact(&x.name, "", x.email.as_deref()).is_some() {
                t.set.insert(x.name.clone());
                t.set.extend(x.email.clone());
            }
        }
        for x in &c.officials {
            if w.contact(&x.name, &x.affiliation, None).is_some() {
                t.set.insert(x.name.clone());
            }
        }
    }
    t.set.retain(|s| !s.trim().is_empty());
    if t.closed {
        t.set.extend(closed_tokens(graph));
    }
    t
}

impl WithholdState {
    /// Hold one coherent visibility snapshot while calculating query counts and candidates.
    /// Reuse the same token matcher as the response filter; never count removed rows first.
    pub(crate) fn with_query_visibility<T>(&self, run: impl FnOnce(&dyn Fn(&str) -> bool) -> T) -> T {
        let tokens = self.tokens.read().unwrap_or_else(|p| p.into_inner());
        let closed = tokens.closed.then(|| closed_tokens(&self.graph));
        run(&|id| !withheld_str(id, &tokens.set) && !closed.as_ref().is_some_and(|set| withheld_str(id, set)))
    }

    pub fn new(data: PathBuf, graph: Arc<Graph>) -> SharedWithhold {
        let salt = atlas_ingest::withhold::salt_from_env();
        if salt.is_dev() {
            eprintln!(
                "withhold: {} unset, using the documented development salt",
                atlas_core::withhold::SALT_ENV
            );
        }
        let s = Arc::new(Self {
            data,
            salt,
            graph,
            tokens: RwLock::new(Arc::new(Tokens::default())),
            write: std::sync::Mutex::new(()),
        });
        s.reload();
        s
    }

    /// Re-read both lists (after an approval, or on start).
    pub fn reload(&self) {
        let w = atlas_ingest::withhold::load_or_closed(&self.data, self.salt.clone());
        let t = tokens_for(&self.graph, &w);
        eprintln!(
            "withhold: {} suppression entries, {} items withheld at runtime{}",
            w.suppression_entries().len(),
            t.set.len(),
            if t.closed { " (closed)" } else { "" }
        );
        *self.tokens.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(t);
    }

    fn report(&self, p: &TraceParams) -> TraceReport {
        let q = TraceQuery {
            orcid: p.orcid.clone(),
            name: p.name.clone(),
            affiliation: p.affiliation.clone(),
            email: p.email.clone(),
            node: p.node.clone(),
        };
        if q.is_empty() {
            return TraceReport::default();
        }
        trace(
            &self.data,
            &self.graph,
            &q,
            &self.salt,
            &TraceOptions::for_data(&self.data),
        )
    }
}

/// Host actions for atlas-contrib's privacy requests.
pub struct ServerActions(pub SharedWithhold);

impl PrivacyActions for ServerActions {
    fn trace(&self, p: &TraceParams) -> Value {
        let r = self.0.report(p);
        json!({ "counts": r.counts(), "report": r })
    }

    fn suppress(&self, o: &SuppressOrder<'_>) -> Result<String, String> {
        let s = &self.0;
        let mut keys = s.report(o.trace).keys;
        for id in o.nodes {
            match s.graph.node(id) {
                Some(k) if k.kind == atlas_core::node::NodeKind::Person => {
                    keys.extend(s.salt.person_keys(s.graph.person(k.idx)))
                }
                _ => keys.extend(s.salt.key(KeyKind::Node, id)),
            }
        }
        keys.sort();
        keys.dedup();
        if keys.is_empty() {
            return Err(
                "the trace found no identifiers to suppress; give an ORCID, name + affiliation, e-mail or node id"
                    .into(),
            );
        }
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).map_err(|e| e.to_string())?;
        let entry = SuppressionEntry {
            id: format!("sup_{}", rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            keys,
            scope: Default::default(),
            reason: match o.kind {
                atlas_contrib::privacy::RequestType::Remove => "gdpr_art17_erasure",
                atlas_contrib::privacy::RequestType::Object => "gdpr_art21_objection",
                atlas_contrib::privacy::RequestType::Correct => "gdpr_art16_rectification_pending",
            }
            .into(),
            date: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
            reviewer: o.reviewer.to_owned(),
            request: Some(o.reference.to_owned()),
            salt_id: s.salt.id().to_owned(),
        };
        let id = entry.id.clone();
        {
            let _g = s.write.lock().unwrap_or_else(|p| p.into_inner());
            atlas_ingest::withhold::append_suppression(&s.data, entry).map_err(|e| e.to_string())?;
        }
        s.reload();
        Ok(id)
    }

    fn email_hash(&self, email: &str) -> String {
        self.0
            .salt
            .key(KeyKind::Email, email)
            .unwrap_or_else(|| "invalid".into())
    }
}

fn withheld_str(s: &str, t: &HashSet<String>) -> bool {
    t.contains(s) || (s.contains('|') && s.split('|').any(|part| t.contains(part)))
}

/// `None` = the value itself is withheld.
fn filter_value(v: Value, t: &HashSet<String>) -> Option<Value> {
    match v {
        Value::String(s) => (!withheld_str(&s, t)).then_some(Value::String(s)),
        Value::Array(a) => Some(Value::Array(a.into_iter().filter_map(|x| filter_value(x, t)).collect())),
        Value::Object(m) => {
            if m.values().any(|x| x.as_str().is_some_and(|s| withheld_str(s, t))) {
                return None;
            }
            // Search items hold identity under `node`; discard the item as a unit rather
            // than leave a result without its canonical node after runtime suppression.
            if m.get("node")
                .and_then(Value::as_object)
                .is_some_and(|n| n.values().any(|x| x.as_str().is_some_and(|s| withheld_str(s, t))))
            {
                return None;
            }
            Some(Value::Object(
                m.into_iter()
                    .filter(|(k, _)| !withheld_str(k, t))
                    .filter_map(|(k, x)| match x {
                        Value::String(_) => Some((k, x)),
                        other => filter_value(other, t).map(|x| (k, x)),
                    })
                    .collect(),
            ))
        }
        other => Some(other),
    }
}

/// Person node ids and contact names when the filter is closed (computed on reload).
fn closed_tokens(graph: &Graph) -> HashSet<String> {
    let mut set = HashSet::new();
    for person in &graph.data().people {
        set.insert(person.id.clone());
        set.insert(crate::export::iri(&person.id).trim_matches(['<', '>']).to_owned());
        set.insert(person.name.clone());
        set.extend(person.name_variants.iter().cloned());
        set.extend(person.orcids.iter().cloned());
    }
    for c in &graph.data().contacts {
        set.extend(c.central.iter().map(|x| x.name.clone()));
        set.extend(c.central.iter().filter_map(|x| x.email.clone()));
        set.extend(c.officials.iter().map(|x| x.name.clone()));
    }
    set.retain(|token| !token.trim().is_empty());
    set
}

/// Middleware: filter every response body through the runtime tokens.
pub async fn filter_responses(State(w): State<SharedWithhold>, req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    let tokens = w.tokens.read().unwrap_or_else(|p| p.into_inner()).clone();
    if tokens.set.is_empty() && !tokens.closed {
        return res;
    }
    let set = &tokens.set;
    let ctype = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let is_json = ctype.starts_with("application/json");
    let is_text = ctype.starts_with("text/");
    if !is_json && !is_text {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 256 * 1024 * 1024).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "response too large to filter").into_response();
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    if is_json {
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "detail": "response could not be filtered" })),
            )
                .into_response();
        };
        return match filter_value(v, set) {
            Some(v) => Response::from_parts(parts, Body::from(v.to_string())),
            None => (StatusCode::NOT_FOUND, axum::Json(json!({ "detail": "not found" }))).into_response(),
        };
    }
    let text = String::from_utf8_lossy(&bytes);
    // Turtle is structured across lines: dropping individual lines can retain personal lineage
    // or produce invalid RDF. Withhold the whole export until the graph is rebuilt.
    if set.iter().any(|token| text.contains(token)) {
        return (StatusCode::NOT_FOUND, axum::Json(json!({ "detail": "not found" }))).into_response();
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// Every streamed row is filtered immediately before emission, using the latest lists.
pub fn filter_stream_value(w: &SharedWithhold, value: Value) -> Option<Value> {
    let tokens = w.tokens.read().unwrap_or_else(|p| p.into_inner()).clone();
    filter_value(value, &tokens.set)
}

/// `atlas-server trace`: counts by default, details only with `--show` (reviewer use).
pub fn trace_cli(data: &Path, graph_snapshot: Option<PathBuf>, q: TraceQuery, show: bool) -> anyhow::Result<()> {
    anyhow::ensure!(!q.is_empty(), "give --orcid, --name [--affiliation], --email or --node");
    let path = graph_snapshot.unwrap_or_else(|| atlas_ingest::graph::snapshot_path(data));
    // Read-only: the snapshot is loaded as is, never rebuilt or written.
    let (graph, _) = atlas_core::snapshot::load_graph(&path)?;
    let salt = atlas_ingest::withhold::salt_from_env();
    let r = trace(data, &graph, &q, &salt, &TraceOptions::for_data(data));
    let out = if show {
        json!({ "counts": r.counts(), "report": r })
    } else {
        json!({ "counts": r.counts() })
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_quarantine_withholds_individual_initiative_actions() {
        use atlas_core::graph::{GraphData, Initiative, OfficialAction, RecordHash, SourceRecord};
        use atlas_core::provenance::{EntityIdx, Locator, SourceEntity};
        let mut data = GraphData::default();
        data.provenance.add_entity(SourceEntity {
            file: "cache/ecosystem/initiatives.json".into(),
            ..SourceEntity::default()
        });
        data.records.push(SourceRecord {
            entity: EntityIdx(0),
            locator: Locator::Record("records[0]".into()),
            id: "ecosystem:test".into(),
            url: Some("https://example.org/participate".into()),
            fetched_at: None,
            hash: RecordHash::CanonicalJson,
            sha256: [1; 32],
        });
        data.initiatives.push(Initiative {
            id: "ecosystem:test".into(),
            actions: vec![OfficialAction {
                url: "https://example.org/join".into(),
                records: vec![0],
                ..OfficialAction::default()
            }],
            ..Initiative::default()
        });
        let graph = Graph::new(data);
        let q = json!({"schema":"quarantine","version":1,"entries":[{"file":"cache/ecosystem/responses/page.bin",
            "source_url":"https://example.org/participate","reason":"terminal block"}]});
        let bytes = serde_json::to_vec(&q).unwrap();
        let w = Withhold::from_lists(Salt::new("fixture"), None, Some(&bytes)).unwrap();
        let tokens = tokens_for(&graph, &w);
        assert!(tokens.set.contains("https://example.org/join"));
        let value = json!({"already_working_on_this":[{"id":"ecosystem:test","official_action":{"url":"https://example.org/join"}}]});
        assert!(
            !filter_value(value, &tokens.set)
                .unwrap()
                .to_string()
                .contains("https://example.org/join")
        );
    }

    #[test]
    fn filter_drops_objects_edges_and_array_items() {
        let t: HashSet<String> = ["ORCID:0000-0000-0000-0000".to_string(), "Placeholder Name".to_string()].into();
        let v = json!({
            "people": [
                {"id": "ORCID:0000-0000-0000-0000", "name": "x"},
                {"id": "person:kept", "name": "Kept"}
            ],
            "edges": ["ORCID:0000-0000-0000-0000|author_of|PMID:1", "person:kept|author_of|PMID:1"],
            "contacts": {"central": [{"name": "Placeholder Name", "role": "contact"}]},
            "count": 2
        });
        let f = filter_value(v, &t).unwrap();
        assert_eq!(f["people"].as_array().unwrap().len(), 1);
        assert_eq!(f["edges"].as_array().unwrap().len(), 1);
        assert!(f["contacts"]["central"].as_array().unwrap().is_empty());
        assert!(filter_value(json!({"id": "ORCID:0000-0000-0000-0000"}), &t).is_none());
        let hits = filter_value(
            json!([
                {"node":{"id":"ORCID:0000-0000-0000-0000"},"why":"some reason"},
                {"node":{"id":"person:kept"},"why":"some reason"}
            ]),
            &t,
        )
        .unwrap();
        assert_eq!(hits.as_array().unwrap().len(), 1);
        assert_eq!(hits[0]["node"]["id"], "person:kept");
    }

    #[test]
    fn streamed_rows_use_the_latest_filter_snapshot() {
        let w = Arc::new(WithholdState {
            data: PathBuf::new(),
            salt: Salt::new("synthetic"),
            graph: Arc::new(Graph::default()),
            tokens: RwLock::new(Arc::new(Tokens::default())),
            write: std::sync::Mutex::new(()),
        });
        let row = json!({"event": "explanation", "card_id": "person:fixture", "data": {"text": "fixture"}});
        assert!(filter_stream_value(&w, row.clone()).is_some());
        *w.tokens.write().unwrap() = Arc::new(Tokens {
            set: ["person:fixture".into()].into(),
            closed: false,
        });
        assert!(
            filter_stream_value(&w, row).is_none(),
            "an approval after the first row hides later rows"
        );
    }
    #[tokio::test]
    async fn approval_hook_reloads_json_and_turtle_filters_without_rebuilding() {
        use atlas_core::graph::{GraphData, Person, PersonSource};
        use axum::{Router, middleware, routing::get};
        use tower::ServiceExt;
        let dir = std::env::temp_dir().join(format!("atlas-privacy-hook-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = "ORCID:0000-0000-0000-0000";
        let person = Person {
            id: id.into(),
            name: "Synthetic Testperson".into(),
            name_variants: vec![],
            orcids: vec!["0000-0000-0000-0000".into()],
            affiliations: vec![],
            source: PersonSource::Orcid,
            genes: vec![],
            communities: vec![],
            cross_community: false,
            matched_by: vec![],
            merge_basis: vec![],
            records: vec![],
        };
        let state = WithholdState::new(
            dir.clone(),
            Arc::new(Graph::new(GraphData {
                people: vec![person],
                ..Default::default()
            })),
        );
        let app = Router::new()
            .route("/json", get(|| async { axum::Json(json!({"people":[{"id":"ORCID:0000-0000-0000-0000","name":"Synthetic Testperson"},{"id":"person:kept"}]})) }))
            .route("/turtle", get(|| async { ([(header::CONTENT_TYPE, "text/turtle")], "<https://orcid.org/0000-0000-0000-0000> <https://schema.org/name> \"Synthetic Testperson\" .") }))
            .layer(middleware::from_fn_with_state(state.clone(), filter_responses));
        let before = app
            .clone()
            .oneshot(Request::builder().uri("/turtle").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(before.status(), StatusCode::OK);
        let actions = ServerActions(state.clone());
        let params = TraceParams {
            node: Some(id.into()),
            ..Default::default()
        };
        let entry = actions
            .suppress(&SuppressOrder {
                reference: "pr_fixture",
                kind: atlas_contrib::privacy::RequestType::Remove,
                trace: &params,
                nodes: &[],
                reviewer: "agent:reviewer/fixture",
            })
            .unwrap();
        let persisted = std::fs::read_to_string(dir.join("suppression.json")).unwrap();
        assert!(persisted.contains(&entry));
        assert!(!persisted.contains(id) && !persisted.contains("Synthetic Testperson"));
        let after = app
            .clone()
            .oneshot(Request::builder().uri("/json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(after.into_body(), 4096).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["people"].as_array().unwrap().len(), 1);
        let turtle = app
            .oneshot(Request::builder().uri("/turtle").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(turtle.status(), StatusCode::NOT_FOUND);
        assert!(
            state.graph.node(id).is_some(),
            "runtime removal did not rebuild/mutate the graph"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
