//! Unit-level graph API. Construction and raw-file IO run off the async executor.
use crate::copy_extra;
use crate::routes::{ApiError, AppState, internal, not_found};

struct CurrentWithhold<'a> {
    graph: &'a atlas_core::Graph,
    current: atlas_core::withhold::Withhold,
}
impl atlas_core::graph::RecordWithhold for CurrentWithhold<'_> {
    fn records_withheld(&self, records: &[u32]) -> Option<&str> {
        self.current
            .records(self.graph.data(), records)
            .map(|h| h.reason)
            .or_else(|| atlas_core::graph::RecordWithhold::records_withheld(self.graph, records))
    }
    fn node_withheld(&self, key: atlas_core::node::NodeKey) -> Option<&str> {
        self.current
            .node(self.graph, key)
            .map(|h| h.reason)
            .or_else(|| atlas_core::graph::RecordWithhold::node_withheld(self.graph, key))
    }
}
use atlas_core::units::{self, PathwayData, UnitCollection};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Debug, Deserialize)]
pub struct UnitQuery {
    #[serde(default)]
    pub focus: String,
    pub unit: Option<String>,
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

async fn collection(state: AppState, focus: String) -> Result<UnitCollection, ApiError> {
    if focus.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            copy_extra::msg("api.error.focus_required", json!({})),
        ));
    }
    tokio::task::spawn_blocking(move || {
        let pathways = state
            .units
            .get_or_init(|| atlas_ingest::semantic_units::read(&state.data, state.atlas()).map_err(|e| e.to_string()));
        // Missing source is represented as unavailable; unreadable source is an explicit error.
        let pathways: &PathwayData = pathways.as_ref().map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                copy_extra::msg("api.error.internal", json!({})),
            )
        })?;
        let filter = CurrentWithhold {
            graph: &state.graph,
            current: atlas_ingest::withhold::load_or_closed(&state.data, atlas_ingest::withhold::salt_from_env()),
        };
        units::build_with_filter(state.atlas(), &state.graph, pathways, &focus, &filter)
            .ok_or_else(|| not_found(copy_extra::msg("api.error.item_unknown", json!({"id": focus}))))
    })
    .await
    .map_err(internal)?
}

pub async fn list(
    State(state): State<AppState>,
    query: Result<Query<UnitQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let q = parsed_query(query)?;
    let limit = q.limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            copy_extra::msg("api.error.unit_limit", json!({})),
        ));
    }
    let mut c = collection(state, q.focus.clone()).await?;
    let total = c.units.len();
    let single = q.unit.is_some();
    if let Some(id) = q.unit {
        c.units.retain(|u| u.id == id);
        if c.units.is_empty() {
            return Err(not_found(copy_extra::msg(
                "api.error.item_unknown",
                json!({"id": id, "focus": q.focus}),
            )));
        }
    } else {
        c.units = c.units.into_iter().skip(q.offset).take(limit).collect();
    }
    let next = (!single && q.offset.saturating_add(limit) < total).then(|| q.offset.saturating_add(limit));
    let mut value = serde_json::to_value(c).map_err(internal)?;
    value["pagination"] = json!({"total": total, "offset": q.offset, "limit": limit, "next_offset": next});
    Ok(Json(value))
}

pub async fn rdf(
    State(state): State<AppState>,
    query: Result<Query<UnitQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let q = parsed_query(query)?;
    let c = rdf_selection(collection(state, q.focus).await?, q.unit)?;
    // RDF always exports the entire selected view, without JSON pagination.
    let text = tokio::task::spawn_blocking(move || units::rdf::turtle(&c))
        .await
        .map_err(internal)?;
    Ok(([(header::CONTENT_TYPE, "text/turtle; charset=utf-8")], text).into_response())
}

fn parsed_query(query: Result<Query<UnitQuery>, QueryRejection>) -> Result<UnitQuery, ApiError> {
    query.map(|Query(q)| q).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            crate::find::msg(
                "find.error.query",
                json!({"field": "filters"}),
                "Invalid search request.",
            ),
        )
    })
}

fn rdf_selection(mut c: UnitCollection, id: Option<String>) -> Result<UnitCollection, ApiError> {
    if let Some(id) = id {
        c.units.retain(|u| u.id == id);
        if c.units.is_empty() {
            return Err(not_found(copy_extra::msg("api.error.item_unknown", json!({"id": id}))));
        }
        // A detail export must not carry unrelated links with unresolved endpoints/proofs.
        c.links.clear();
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::provenance::{ActivityIdx, EntityIdx, RecordRef, SourceEntity};
    use atlas_core::{Atlas, Disease, DiseaseIdentity, Graph, Provenance};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use axum::{Router, routing::get};
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    fn state(root: std::path::PathBuf) -> AppState {
        let mut disease = Disease::new("MONDO:PLACEHOLDER", ActivityIdx(0));
        disease.name = "Placeholder condition".into();
        disease.derived_from.push(RecordRef::line(EntityIdx(0), 1));
        let atlas = Arc::new(Atlas::new(
            Vec::new(),
            DiseaseIdentity::default(),
            Provenance {
                entities: vec![SourceEntity {
                    id: "source:fixture".into(),
                    file: "fixture.tsv".into(),
                    url: "https://example.org/fixture".into(),
                    sha256: Some("a".repeat(64)),
                    ..SourceEntity::default()
                }],
                activities: Vec::new(),
            },
            vec![disease],
        ));
        let graph = Arc::new(Graph::default());
        AppState {
            search: Arc::new(atlas_core::search::domain::Index::build(&atlas, &graph)),
            questions: Arc::new(Default::default()),
            matcher: Arc::new(atlas_analytics::Matcher::new(atlas.clone(), Default::default())),
            atlas: atlas.clone(),
            withhold: crate::privacy::WithholdState::new(root.clone(), graph.clone()),
            graph: graph.clone(),
            data: Arc::new(root),
            llm: crate::llm::LlmState {
                llm: None,
                runtime: Arc::new(Default::default()),
            },
            integrity: Arc::new(OnceLock::new()),
            related: Arc::new(OnceLock::new()),
            clusters: Arc::new(OnceLock::new()),
            units: Arc::new(OnceLock::new()),
            query_engine: Arc::new(OnceLock::new()),
            query_suggestions: Arc::new(atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph)),
            explore_index: Arc::new(OnceLock::new()),
        }
    }

    #[tokio::test]
    async fn http_contract_pagination_detail_errors_and_turtle() {
        let root = std::env::temp_dir().join(format!("atlas-unit-api-fixture-{}", std::process::id()));
        let app = Router::new()
            .route("/api/units", get(list))
            .route("/api/units.ttl", get(rdf))
            .with_state(state(root));
        async fn request(app: Router, uri: &str) -> (StatusCode, String, String) {
            let res = app
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = res.status();
            let content_type = res.headers()[header::CONTENT_TYPE].to_str().unwrap().to_owned();
            let text = String::from_utf8(to_bytes(res.into_body(), 10_000_000).await.unwrap().to_vec()).unwrap();
            (status, content_type, text)
        }
        let (status, _, text) = request(app.clone(), "/api/units?focus=MONDO:PLACEHOLDER&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["schema"], units::SCHEMA);
        assert_eq!(body["pagination"]["total"], 1);
        assert_eq!(body["units"].as_array().unwrap().len(), 1);
        assert_eq!(body["root_summaries"][0]["id"], body["roots"][0]);
        assert_eq!(body["pathways_available"], false);
        let id = body["units"][0]["id"].as_str().unwrap();
        assert_eq!(
            request(app.clone(), &format!("/api/units?focus=MONDO:PLACEHOLDER&unit={id}"))
                .await
                .0,
            StatusCode::OK
        );
        let (_, _, empty) = request(app.clone(), "/api/units?focus=MONDO:PLACEHOLDER&offset=9999").await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&empty).unwrap()["units"],
            json!([])
        );
        for (uri, expected) in [
            ("/api/units", StatusCode::BAD_REQUEST),
            ("/api/units?focus=missing", StatusCode::NOT_FOUND),
            ("/api/units?focus=MONDO:PLACEHOLDER&unit=missing", StatusCode::NOT_FOUND),
            ("/api/units?focus=MONDO:PLACEHOLDER&limit=0", StatusCode::BAD_REQUEST),
            ("/api/units?focus=MONDO:PLACEHOLDER&limit=501", StatusCode::BAD_REQUEST),
            (
                "/api/units?focus=MONDO:PLACEHOLDER&limit=invalid",
                StatusCode::BAD_REQUEST,
            ),
            (
                "/api/units.ttl?focus=MONDO:PLACEHOLDER&offset=invalid",
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let (status, _, body) = request(app.clone(), uri).await;
            assert_eq!(status, expected, "{uri}");
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            let expected_key = if uri == "/api/units" {
                "api.error.focus_required"
            } else if uri.contains("=invalid") {
                "find.error.query"
            } else if expected == StatusCode::NOT_FOUND {
                "api.error.item_unknown"
            } else {
                "api.error.unit_limit"
            };
            assert_eq!(body["detail_msg"]["key"], expected_key, "{uri}");
            assert_eq!(body["detail"], body["detail_msg"]["fallback"]);
            assert!(crate::language_contract::offenders(&body).is_empty());
        }
        let (status, ctype, text) = request(app, "/api/units.ttl?focus=MONDO:PLACEHOLDER").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ctype.starts_with("text/turtle"));
        assert!(text.contains("ra:CommunityUnit"));
        assert!(text.contains("prov:wasGeneratedBy"));
    }

    #[tokio::test]
    async fn standalone_rdf_omits_unrelated_links() {
        let root = std::env::temp_dir().join(format!("atlas-unit-api-detail-{}", std::process::id()));
        let s = state(root);
        let mut c = collection(s, "MONDO:PLACEHOLDER".into())
            .await
            .map_err(|e| e.1)
            .unwrap();
        let id = c.units[0].id.clone();
        c.links.push(atlas_core::units::UnitLink {
            id: "unit:link:PLACEHOLDER".into(),
            from: id.clone(),
            to: "unit:community:OTHER_PLACEHOLDER".into(),
            relation: "shares_pathway_group".into(),
            status: atlas_core::units::AssertionStatus::Inferred,
            support: vec![],
            evidence: vec![],
            generated_by: c.activity.id.clone(),
            sha256: String::new(),
        });
        let c = rdf_selection(c, Some(id)).map_err(|e| e.1).unwrap();
        let body = units::rdf::turtle(&c);
        assert!(body.contains("ra:CommunityUnit"));
        assert!(!body.contains("ra:UnitLink"));
    }

    #[tokio::test]
    async fn unreadable_source_is_explicit_without_leaking_paths() {
        let root = std::env::temp_dir().join(format!("atlas-unit-api-invalid-{}", std::process::id()));
        let raw = root.join("raw");
        std::fs::create_dir_all(&raw).unwrap();
        let path = raw.join("NCBI2Reactome.txt");
        std::fs::write(&path, [0xff]).unwrap();
        let response = list(
            State(state(root.clone())),
            Ok(Query(UnitQuery {
                focus: "MONDO:PLACEHOLDER".into(),
                unit: None,
                offset: 0,
                limit: None,
            })),
        )
        .await
        .err()
        .unwrap()
        .into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = String::from_utf8(to_bytes(response.into_body(), 10000).await.unwrap().to_vec()).unwrap();
        assert!(!body.contains(&root.to_string_lossy().to_string()));
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["detail_msg"]["key"], "api.error.internal");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(raw).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
