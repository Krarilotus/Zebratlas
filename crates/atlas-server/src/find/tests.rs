use super::*;
use crate::test_support::{call, state};
use axum::routing::get;
use std::sync::Arc;

#[tokio::test]
async fn current_search_and_jobs_keep_subject_scoped_coverage() {
    for (failed, expected) in [(0, "cached_evidence"), (1, "outside_verified_neighbourhood")] {
        let mut s = state();
        let mut gd = s.graph.data().clone();
        gd.coverage.push(atlas_core::graph::Coverage {
            source: "reporter".into(),
            status: "loaded".into(),
            genes: vec!["STXBP1".into()],
            header_checksums_failed: failed,
            ..Default::default()
        });
        s.graph = Arc::new(Graph::new(gd));
        let app = axum::Router::new()
            .route("/search", get(search))
            .route("/jobs/{id}", get(crate::jobs::condition))
            .with_state(s);
        let (status, b) = call(app.clone(), "GET", "/search?q=STXBP1&country=DE", None).await;
        assert_eq!(status, StatusCode::OK);
        for field in ["results", "items"] {
            let condition = b[field]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["node"]["id"] == "MONDO:9999999")
                .unwrap();
            assert_eq!(condition["coverage"]["status"], expected);
            assert_eq!(condition["coverage"]["discovery_routes_checked"], false);
        }
        let (status, b) = call(app, "GET", "/jobs/MONDO:9999999", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(b["coverage"]["status"], expected);
        assert_eq!(b["coverage"]["discovery_routes_checked"], false);
    }
}

#[tokio::test]
async fn scoped_filters_disjunctive_facets_and_checked() {
    let s = state();
    let app = axum::Router::new().route("/search", get(search)).with_state(s);
    let (status, b) = call(app.clone(), "GET", "/search?q=STXBP1&country=DE&recruiting=true", None).await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<_> = b["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"condition") && kinds.contains(&"gene"));
    assert_eq!(kinds.iter().filter(|&&k| k == "study").count(), 1);
    assert_eq!(b["facets"]["country"].as_array().unwrap().len(), 1); // FR is completed: other filter still applies
    let (_, b) = call(app, "GET", "/search?q=STXBP1&kind=study&country=DE", None).await;
    assert_eq!(b["total"], 1);
    assert_eq!(b["facets"]["country"].as_array().unwrap().len(), 2); // country ignores its own filter
    let checked = b["checked"]["sources"].as_array().unwrap();
    let mondo = checked.iter().find(|c| c["source"] == "mondo").unwrap();
    assert_eq!(mondo["status"], "matched");
    assert_eq!(mondo["version"], "synthetic-v1");
    assert_eq!(b["checked"]["summary"]["key"], "checked.summary");
}

#[tokio::test]
async fn invalid_values_and_graph_masks() {
    let s = state();
    let app = axum::Router::new()
        .route("/search", get(search))
        .route("/graph", get(graph_view))
        .with_state(s);
    for filter in ["kind=unknown", "country=ZZ", "language=zz", "recruiting=yes"] {
        let (status, b) = call(app.clone(), "GET", &format!("/search?q=STXBP1&{filter}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(b["msg"]["key"], "find.error.filter");
    }
    let (_, b) = call(app, "GET", "/graph?focus=MONDO:9999999&country=DE", None).await;
    assert_eq!(b["nodes"].as_array().unwrap().len(), 3);
    assert!(
        b["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["node"]["id"] == "NCTTEST1" && n["masked"] == true)
    );
    assert_eq!(b["edges"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn withheld_records_do_not_reappear_in_search_or_masks() {
    let mut s = state();
    let mut gd = s.graph.data().clone();
    gd.quarantine.push(atlas_core::graph::Quarantine {
        record: 0,
        reason: "synthetic hold".into(),
    });
    s.graph = Arc::new(Graph::new(gd));
    let app = axum::Router::new()
        .route("/search", get(search))
        .route("/graph", get(graph_view))
        .with_state(s);
    for path in ["/search?q=STXBP1", "/graph?focus=MONDO:9999999"] {
        let (_, b) = call(app.clone(), "GET", path, None).await;
        assert!(!b.to_string().contains("NCTTEST0"));
    }
}

#[tokio::test]
async fn resolve_keeps_resolution_and_adds_filtered_links() {
    let app = axum::Router::new().route("/resolve", get(resolve)).with_state(state());
    let (status, b) = call(app, "GET", "/resolve?q=MONDO:9999999&llm=0&kind=study&country=DE", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b["target"]["id"], "MONDO:9999999");
    assert_eq!(b["linked"]["total"], 1);
    assert!(b["checked"]["sources"].is_array());
}

#[tokio::test]
async fn resolve_reused_matches_keep_legacy_resolution_and_filtered_coverage() {
    let app = axum::Router::new()
        .route("/resolve", get(resolve))
        .route("/legacy", get(crate::journeys::resolve))
        .with_state(state());
    for q in [
        "STXBP1",
        "My%20daughter%20has%20STXBP1",
        "MONDO:9999999",
        "no-synthetic-match",
    ] {
        let (status, current) = call(app.clone(), "GET", &format!("/resolve?q={q}&llm=0&country=DE"), None).await;
        let (legacy_status, legacy) = call(app.clone(), "GET", &format!("/legacy?q={q}&llm=0"), None).await;
        assert_eq!(status, legacy_status);
        for key in ["status", "target", "choices", "ranked_matches", "reconcile"] {
            assert_eq!(current[key], legacy[key], "{q}: {key}");
        }
        assert!(current["checked"]["sources"].is_array());
    }
    let (status, _) = call(app, "GET", &format!("/resolve?q={}&llm=0", "x".repeat(513)), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[test]
fn group_language_and_country_filters_leave_other_kinds_available() {
    let graph = Graph::new(Default::default());
    let mut group = item(
        &graph,
        NodeRef {
            id: "org:synthetic".into(),
            kind: NodeKind::Organisation,
            label: "Synthetic group".into(),
        },
        None,
        "title",
        None,
    );
    group.country = vec!["DE".into()];
    group.languages = vec!["en".into()];
    let f = Params {
        country: Some("de".into()),
        language: Some("fr".into()),
        recruiting: Some("true".into()),
        ..Default::default()
    }
    .filters()
    .unwrap();
    assert!(!f.accepts(&group, ""));
    assert!(f.accepts(&group, "language"));
    let condition = item(
        &graph,
        NodeRef {
            id: "MONDO:9999999".into(),
            kind: NodeKind::Disease,
            label: "Synthetic condition".into(),
        },
        None,
        "title",
        None,
    );
    assert!(f.accepts(&condition, ""));
    assert_eq!(filters::language("English").as_deref(), Some("en"));
    assert_eq!(filters::language("de-DE").as_deref(), Some("de"));
    assert_eq!(filters::country("Germany").as_deref(), Some("DE"));
}

#[tokio::test]
async fn bounded_post_lookup_is_deterministic_and_keyed() {
    let app = axum::Router::new()
        .route("/lookup", axum::routing::post(lookup))
        .with_state(state());
    let (status, body) = call(app.clone(), "POST", "/lookup", Some(json!({"q":"STXBP1","limit":6}))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["results"].as_array().unwrap().len() <= 6);
    assert!(body.get("query_execution").is_none());
    for q in [String::new(), "Jane\nSTXBP1".into(), "\u{00e9}".repeat(65)] {
        let (status, body) = call(app.clone(), "POST", "/lookup", Some(json!({"q":q}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["detail_msg"]["key"].as_str().is_some());
    }
    for limit in [0, 7] {
        let (status, body) = call(
            app.clone(),
            "POST",
            "/lookup",
            Some(json!({"q":"STXBP1","limit":limit})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["detail_msg"]["key"], "find.error.query");
    }
}
