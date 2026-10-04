//! Thin HTTP boundary; all exploration/counting is owned by atlas-core::query_graph.
use crate::routes::{ApiError, ApiResult, AppState};
use atlas_core::query_graph::{PreviewRequest, SuggestRequest};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde_json::json;

fn bad_request(_detail: String) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        atlas_core::search::messages::Message::new("error.invalid", json!({})).rendered("en"),
    )
}

pub async fn suggestions(State(s): State<AppState>, Query(request): Query<SuggestRequest>) -> ApiResult {
    if request.node.as_ref().is_some_and(|v| v.len() > 512)
        || request.class.as_ref().is_some_and(|v| v.len() > 128)
        || request.relation.as_ref().is_some_and(|v| v.len() > 128)
        || request.target_class.as_ref().is_some_and(|v| v.len() > 128)
        || request.q.as_ref().is_some_and(|v| v.len() > 128)
        || request.offset.is_some_and(|v| v > 1_000_000)
        || request.limit.is_some_and(|v| v == 0 || v > 5)
    { return Err(bad_request("Suggestion input exceeds bounds".into())); }
    let started = std::time::Instant::now();
    let page = s
        .withhold
        .with_query_visibility(|allowed| s.query_suggestions.suggest(&request, &s.atlas, &s.graph, &allowed))
        .map_err(bad_request)?;
    let mut value = serde_json::to_value(page).map_err(crate::routes::internal)?;
    value["latency_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    value["prov:wasGeneratedBy"] = json!({"@type":"prov:Activity", "version":1, "operation":"index-neighbours", "ended_at":atlas_core::provenance::rfc3339(std::time::SystemTime::now())});
    Ok(Json(value))
}

pub async fn preview(State(s): State<AppState>, Json(request): Json<PreviewRequest>) -> ApiResult {
    let started = std::time::Instant::now();
    let result = s
        .withhold
        .with_query_visibility(|allowed| s.query_suggestions.preview(&request, &allowed))
        .map_err(bad_request)?;
    let mut value = serde_json::to_value(result).map_err(crate::routes::internal)?;
    value["latency_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    value["scope"] = json!("in_memory_asserted_graph");
    Ok(Json(value))
}

pub async fn index(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!({"version":1, "index_sha256":s.query_suggestions.sha256,
        "excluded_unresolved":s.query_suggestions.excluded_unresolved,
        "sources":{"atlas":s.atlas.provenance.entities,"connected":s.graph.provenance().entities},
        "count_unit":"distinct_visible_target_ids", "evidence_sample":"up_to_three_records_for_one_witness_edge",
        "prov:wasGeneratedBy":{"@type":"prov:Activity", "operation":"build-query-suggestions-index", "software_version":env!("CARGO_PKG_VERSION")}})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_analytics::{Matcher, ScoringParams};
    use atlas_core::{Atlas, DiseaseIdentity, Graph, Provenance, graph::GraphData};
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    fn router() -> axum::Router {
        let atlas = Arc::new(Atlas::new(
            vec![],
            DiseaseIdentity::default(),
            Provenance::default(),
            vec![],
        ));
        let graph = Arc::new(Graph::new(GraphData::default()));
        let data = std::env::temp_dir().join(format!("zebratlas-query-graph-readonly-test-{}", std::process::id()));
        let state = AppState {
            questions: Arc::new(crate::questions::QuestionCache::default()),
            matcher: Arc::new(Matcher::new(atlas.clone(), ScoringParams::default())),
            query_suggestions: Arc::new(atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph)),
            withhold: crate::privacy::WithholdState::new(data.clone(), graph.clone()),
            search: Arc::new(atlas_core::search::domain::Index::build(&atlas, &graph)),
            units: Arc::new(OnceLock::new()),
            query_engine: Arc::new(OnceLock::new()),
            explore_index: Arc::new(OnceLock::new()),
            atlas,
            graph,
            data: Arc::new(data),
            llm: crate::llm::LlmState {
                llm: None,
                runtime: Arc::new(crate::llm::Runtime::default()),
            },
            integrity: Arc::new(OnceLock::new()),
            related: Arc::new(OnceLock::new()),
            clusters: Arc::new(OnceLock::new()),
        };
        crate::routes::router(state)
    }

    #[tokio::test]
    async fn public_routes_publish_index_lineage_and_reject_invalid_scope() {
        let app = router();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/query-graph/suggestions?class=paper")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(value["phase"], "properties");
        assert_eq!(value["items"], json!([]));
        assert_eq!(value["index_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(value["prov:wasGeneratedBy"]["@type"], "prov:Activity");
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/query-graph/suggestions?node=unknown&class=paper")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/query-graph/preview")
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"focus":[],"steps":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
