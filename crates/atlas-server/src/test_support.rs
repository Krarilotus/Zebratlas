//! Synthetic in-memory atlas, never an assertion about real patients or studies.
use crate::routes::AppState;
use atlas_core::graph::{
    Coverage, GraphData, GraphEdge, LinkLevel, RecordHash, Relation, SourceRecord, Study, StudyKind,
};
use atlas_core::node::EdgeKind;
use atlas_core::{
    Atlas, Disease, Graph,
    evidence::GeneLink,
    identity::DiseaseIdentity,
    provenance::{Activity, ActivityIdx, Provenance, RecordRef, SourceEntity},
    term::Term,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;

pub fn state() -> AppState {
    let mut prov = Provenance::default();
    let e = prov.add_entity(SourceEntity {
        id: "source:synthetic-mondo".into(),
        file: "mondo.obo".into(),
        version: Some("synthetic-v1".into()),
        url: "https://example.invalid/mondo".into(),
        sha256: Some("00".repeat(32)),
        ..Default::default()
    });
    prov.add_activity(Activity {
        id: "activity:ingest-synthetic".into(),
        used: vec![e],
        ..Default::default()
    });
    let mut d = Disease::new("MONDO:9999999", ActivityIdx(0));
    d.name = "Synthetic STXBP1 condition".into();
    d.derived_from.push(RecordRef::record(e, "test-condition"));
    d.genes.push(GeneLink {
        symbol: "STXBP1".into(),
        association: "MENDELIAN".into(),
        source: "synthetic".into(),
        source_disease: d.id.clone(),
        pmids: vec![],
        assessed: None,
        hgnc: Some("HGNC:11444".into()),
        ncbi_gene: None,
        record: RecordRef::record(e, "test-gene"),
    });
    let hp = Term {
        id: "HP:0001250".into(),
        name: "Seizure".into(),
        ..Default::default()
    };
    let atlas = Arc::new(Atlas::new(
        vec![hp],
        DiseaseIdentity::new(&[], std::iter::empty()),
        prov,
        vec![d],
    ));
    let mut gd = GraphData::default();
    let e = gd.provenance.add_entity(SourceEntity {
        id: "source:synthetic-ctgov".into(),
        file: "cache/trials/studies.jsonl".into(),
        version: Some("test-v1".into()),
        url: "https://example.invalid/studies".into(),
        ..Default::default()
    });
    gd.provenance.add_activity(Activity {
        id: "activity:synthetic-trials".into(),
        used: vec![e],
        ..Default::default()
    });
    for (i, country) in ["Germany", "France"].iter().enumerate() {
        gd.records.push(SourceRecord {
            entity: e,
            locator: atlas_core::provenance::Locator::Line(i as u32 + 1),
            id: format!("NCTTEST{i}"),
            url: None,
            fetched_at: None,
            hash: RecordHash::JsonLine,
            sha256: [0; 32],
        });
        gd.studies.push(Study {
            id: format!("NCTTEST{i}"),
            title: format!("Synthetic STXBP1 study {i}"),
            status: if i == 0 { "RECRUITING" } else { "COMPLETED" }.into(),
            kind: StudyKind::Trial,
            phases: vec![],
            sponsor: String::new(),
            sponsor_class: String::new(),
            start: String::new(),
            completion: String::new(),
            enrollment: None,
            countries: vec![country.to_string()],
            interventions: vec![],
            record: i as u32,
        });
        gd.edges.push(GraphEdge {
            from: format!("NCTTEST{i}"),
            to: "MONDO:9999999".into(),
            relation: Relation::StudiesCondition,
            kind: EdgeKind::Observed,
            level: LinkLevel::Exact,
            reason: "synthetic fixture".into(),
            activity: ActivityIdx(0),
            records: vec![i as u32],
        });
    }
    gd.coverage.push(Coverage {
        source: "ctgov".into(),
        status: "loaded".into(),
        files: vec!["cache/trials/studies.jsonl".into()],
        ..Default::default()
    });
    let matcher = Arc::new(atlas_analytics::Matcher::new(atlas.clone(), Default::default()));
    let graph = Arc::new(Graph::new(gd));
    let withhold = crate::privacy::WithholdState::new("unused-test-data".into(), graph.clone());
    let search = Arc::new(atlas_core::search::domain::Index::build(&atlas, &graph));
    AppState {
        search,
        matcher,
        atlas: atlas.clone(),
        graph: graph.clone(),
        withhold,
        data: Arc::new("unused-test-data".into()),
        llm: crate::llm::LlmState {
            llm: None,
            runtime: Arc::new(Default::default()),
        },
        integrity: Arc::new(OnceLock::new()),
        questions: Arc::new(crate::questions::QuestionCache::default()),
        related: Arc::new(OnceLock::new()),
        clusters: Arc::new(OnceLock::new()),
        units: Arc::new(OnceLock::new()),
        query_engine: Arc::new(OnceLock::new()),
        query_suggestions: Arc::new(atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph)),
        explore_index: Arc::new(OnceLock::new()),
    }
}

pub async fn call(app: Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(body.map(|v| Body::from(v.to_string())).unwrap_or(Body::empty()))
        .unwrap();
    response(app, req).await
}
pub async fn response(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let r = app.oneshot(req).await.unwrap();
    let status = r.status();
    let b = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&b).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&b).into())),
    )
}
