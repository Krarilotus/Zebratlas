//! Router, shared state and the atlas handlers. Journey handlers live in `journeys`; JSON shapes
//! in `views`, `prov`, `cards`, `resolve`, `checks`.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use atlas_analytics::{Matcher, Scorer};
use atlas_core::{Atlas, Graph};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::cors::CorsLayer;

use crate::llm::LlmState;
use crate::{checks, journeys, views};

#[derive(Clone)]
pub struct AppState {
    pub matcher: Arc<Matcher>,
    /// The same atlas the matcher holds, shared with the ask and contribute routers.
    pub atlas: Arc<Atlas>,
    /// Connected layer: studies, grants, papers, people, organisations.
    pub graph: Arc<Graph>,
    pub search: Arc<atlas_core::search::domain::Index>,
    pub query_suggestions: Arc<atlas_core::query_graph::SuggestionIndex>,
    /// Data root (verification re-reads source files under it).
    pub data: Arc<PathBuf>,
    pub llm: LlmState,
    pub query_engine: Arc<OnceLock<Result<Option<Arc<atlas_ask::query::QueryEngine>>, String>>>,
    /// Integrity report, computed on first request (the data is immutable while serving).
    pub integrity: Arc<OnceLock<Value>>,
    /// Similarity index of the mechanism layer, built in the background after start.
    pub related: Arc<OnceLock<Result<atlas_analytics::SimilarityIndex, String>>>,
    /// Bounded shared research-question work/results for this immutable atlas and graph.
    pub questions: Arc<crate::questions::QuestionCache>,
    /// Clusters of the DEE slice (mechanism layer), computed after the index.
    pub clusters: Arc<OnceLock<atlas_analytics::ClusterReport>>,
    /// Direct pathway row evidence, read once without altering graph snapshots.
    pub units: Arc<OnceLock<Result<atlas_core::units::PathwayData, String>>>,
    pub explore_index: Arc<OnceLock<crate::explore::ConnectedIndex>>,
    /// Runtime suppression/quarantine filter over every response (D43).
    pub withhold: crate::privacy::SharedWithhold,
}

impl AppState {
    pub fn atlas(&self) -> &Atlas {
        self.matcher.atlas()
    }
}

/// FastAPI-style error body: `{"detail": ...}`.
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub Value);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let value = if let Some(key) = self.1.as_str().and_then(|s| s.strip_prefix("search.")) {
            atlas_core::search::messages::Message::new(key, json!({})).rendered("en")
        } else {
            self.1
        };
        let message = value.is_object().then(|| value.clone());
        let detail = message
            .as_ref()
            .map_or_else(|| value.clone(), |m| m["fallback"].clone());
        let mut body = json!({"detail": detail, "detail_msg": message});
        if value["code"].is_string() { body["code"] = value["code"].clone(); }
        if value["key"] == "find.error.filter" || value["key"].as_str().is_some_and(|k| k.starts_with("search.")) {
            body["msg"] = value;
        }
        (self.0, Json(body)).into_response()
    }
}

pub fn not_found(what: impl Into<Value>) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, what.into())
}

pub fn internal(_e: impl std::fmt::Display) -> ApiError {
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::copy_extra::msg("api.error.internal", json!({})),
    )
}

pub type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(state: AppState) -> Router {
    let accounts = crate::account_wiring::mounted(&state);
    let privacy = privacy_router(&state);
    let config = crate::connector::Config::from_env(&state)
        .map_err(|e| {
            eprintln!("connector unavailable: {e}");
        })
        .ok();
    assembled(state, accounts, privacy, config)
}

/// One assembly path for production and the full-router connector test.
pub(crate) fn assembled(
    state: AppState,
    accounts: Router,
    privacy: Router,
    config: Option<crate::connector::Config>,
) -> Router {
    let conversations = match &config {
        Some(c) => c.conversations.clone(),
        None => atlas_ask::AccountsStore::from_env()
            .ok()
            .map(|s| Arc::new(s) as Arc<dyn atlas_ask::ConversationStore>),
    };
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/explore/schema", get(crate::explore::schema))
        .route("/api/explore/sparql", post(crate::explore::sparql))
        .route("/api/explore/lookup", post(crate::explore::lookup))
        .route("/api/community", get(crate::explore::community))
        .route("/api/stats", get(stats))
        .route("/api/suggest", get(crate::find::suggest))
        .route("/api/graph/view", get(crate::find::graph_view))
        .route("/api/query-graph/suggestions", get(crate::query_graph::suggestions))
        .route("/api/query-graph/preview", post(crate::query_graph::preview))
        .route("/api/query-graph/index", get(crate::query_graph::index))
        .route("/api/disease/{id}", get(disease))
        .route("/api/condition/{id}", get(disease))
        .route("/api/gene/{id}", get(crate::initiatives::gene))
        .route("/api/initiatives", get(crate::initiatives::partners))
        .route("/api/provenance/{id}", get(provenance))
        .route("/api/match", post(rank))
        .route("/api/integrity", get(integrity))
        .route("/api/verify/{id}", get(verify))
        .route("/api/funding/{id}/evidence", get(crate::funding_evidence::get))
        .route("/api/export.ttl", get(journeys::export))
        .route("/api/units", get(crate::units::list))
        .route("/api/units.ttl", get(crate::units::rdf))
        .route("/api/condition/{id}/gaps", get(journeys::gaps))
        .route("/api/condition/{id}/jobs", get(crate::jobs::condition))
        .route("/api/gene/{id}/jobs", get(crate::jobs::gene))
        .route("/api/condition/{id}/related", get(journeys::related))
        .route(
            "/api/condition/{id}/questions",
            get(crate::questions::condition_questions),
        )
        .route("/api/questions/{qid}", get(crate::questions::question_by_id))
        .route("/api/cluster/{id}", get(journeys::cluster))
        .layer(public_cors())
        .with_state(state.clone())
        .merge(model_routes(&state, conversations))
        .merge(accounts);
    let api = crate::connector::mount(api, &state, config)
        // D43: every response above passes the suppression filter (fail-closed).
        .layer(axum::middleware::from_fn_with_state(
            state.withhold.clone(),
            crate::privacy::filter_responses,
        ))
        // Privacy requests stay outside the filter: the reviewer must see the request itself.
        .merge(privacy);
    crate::account_wiring::deletion_gate(secure_routes(api))
}

fn intake_router() -> Router<AppState> {
    Router::new()
        .route("/api/intake", post(crate::intake::intake))
        .route("/api/intake/terms", post(crate::intake::edited))
        .route("/api/intake/processor", get(crate::intake::processor))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024 + 64 * 1024))
        .layer(axum::middleware::map_response(private_text))
}

fn search_router() -> Router<AppState> {
    Router::new()
        .route("/api/search", get(crate::find::search).post(crate::find::search_text))
        .route("/api/search/lookup", post(crate::find::lookup).layer(DefaultBodyLimit::max(1024)))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024 + 64 * 1024))
        .layer(axum::middleware::map_response(private_text))
}

async fn private_text(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// Applies to every route. Intake and submitted search override the body limit.
fn secure_routes(api: Router) -> Router {
    api.layer(axum::middleware::from_fn(atlas_accounts::csrf::guard))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn(request_limits))
}

/// `/api/privacy/*` (D43); skipped (logged) if its database cannot open.
fn privacy_router(s: &AppState) -> Router {
    use atlas_contrib::privacy::{Privacy, PrivacyConfig, router};
    match Privacy::new(atlas_contrib::ContribConfig::from_env(), PrivacyConfig::default()) {
        Ok(p) => router(
            p.with_actions(Arc::new(crate::privacy::ServerActions(s.withhold.clone())))
                .into_state(),
        ),
        Err(e) => {
            eprintln!("privacy requests unavailable: {e}");
            Router::new()
        }
    }
}

/// Bound URI work before query parsing on every API route.
async fn request_limits(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if request.uri().to_string().len() > 4096 {
        return ApiError(
            StatusCode::URI_TOO_LONG,
            crate::copy_extra::msg("api.error.uri_too_long", json!({})),
        )
        .into_response();
    }
    next.run(request).await
}

fn public_cors() -> CorsLayer {
    let origins = ["http://localhost:3000", "http://127.0.0.1:3000"].map(HeaderValue::from_static);
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-llm-key"),
            header::HeaderName::from_static("x-llm-connection"),
        ])
}

/// The same journey, intake and Ask handlers for hosted and request-scoped LLMs.
pub(crate) fn model_routes(s: &AppState, conversations: Option<Arc<dyn atlas_ask::ConversationStore>>) -> Router {
    let api = Router::new()
        .route("/api/resolve", get(crate::find::resolve))
        .route("/api/explore", post(crate::explore::search))
        .route("/api/explore/overview", post(crate::explore_overview::overview))
        .route("/api/condition/{id}/connections", get(journeys::connections))
        .route("/api/condition/{id}/summary", get(journeys::summary))
        .route("/api/message", post(journeys::message))
        .route("/api/llm/connections", get(journeys::llm_connections))
        .merge(intake_router())
        .merge(search_router())
        .merge(crate::explore_document::router())
        .with_state(s.clone());
    let api = match &s.llm.llm {
        Some(llm) => {
            let mut ask = atlas_ask::AskState::new(s.atlas.clone(), s.graph.clone(), llm.clone());
            if let Ok(Some(engine)) = s
                .query_engine
                .get_or_init(|| atlas_ask::query::QueryEngine::from_env(s.graph.clone()).map(|e| e.map(Arc::new)))
            {
                ask.query_engine = Some(engine.clone());
            }
            ask.store = conversations;
            api.merge(atlas_ask::router(ask))
        }
        None => api,
    };
    api.layer(public_cors())
}

pub async fn serve(addr: &str, state: AppState) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("serving http://{addr}/api");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        tokio::signal::ctrl_c().await.ok();
    })
    .await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn stats(State(s): State<AppState>) -> ApiResult {
    let mut v = views::stats(s.atlas());
    // D33: the live release is (git commit, data manifest hash); the deploy sets both (docs/ops/RUNBOOK.md).
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    v["release"] = json!({ "commit": env("ATLAS_RELEASE_COMMIT"), "data": env("ATLAS_DATA_MANIFEST") });
    Ok(Json(v))
}

async fn disease(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let atlas = s.atlas();
    let idx = atlas
        .disease_idx(s.graph.canonical_id(&id))
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.condition_unknown", json!({"id": id}))))?;
    let mut body = views::disease(atlas, idx);
    let (a2, data) = (s.atlas.clone(), s.data.clone());
    let (codes, gap) = tokio::task::spawn_blocking(move || {
        let codes = crate::codes::load(&a2, &data);
        crate::codes::for_condition(&a2, codes, idx)
    })
    .await
    .map_err(internal)?;
    body["codes"] = json!(codes);
    body["icd_gap"] = gap;
    body["already_working_on_this"] = json!(crate::initiatives::for_condition(atlas, &s.graph, idx));
    Ok(Json(body))
}

/// PROV chain of a connected-layer edge or node, an atlas node or edge, or an LLM call activity.
async fn provenance(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let atlas = s.atlas();
    checks::chain(atlas, &s.graph, &id)
        .or_else(|| s.llm.runtime.lookup(&id))
        .map(Json)
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.item_unknown", json!({"id": id}))))
}

async fn integrity(State(s): State<AppState>) -> ApiResult {
    if let Some(v) = s.integrity.get() {
        return Ok(Json(v.clone()));
    }
    let s2 = s.clone();
    let v = tokio::task::spawn_blocking(move || {
        let t = std::time::Instant::now();
        let mut v = checks::integrity(s2.atlas(), &s2.graph);
        v["checked_in_ms"] = json!(t.elapsed().as_millis() as u64);
        v
    })
    .await
    .map_err(internal)?;
    Ok(Json(s.integrity.get_or_init(|| v).clone()))
}

async fn verify(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let s2 = s.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || checks::verify(&s2.data, s2.atlas(), &s2.graph, id2.trim()))
        .await
        .map_err(internal)?
        .map(Json)
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.item_unknown", json!({"id": id}))))
}

#[derive(Deserialize)]
pub struct MatchRequest {
    pub present: Vec<String>,
    #[serde(default)]
    pub excluded: Vec<String>,
    #[serde(default = "default_top")]
    pub top: usize,
    /// `fusion` (default, D30.2), `atlas` (the scorer before D30.2) or `resnik`; also `?scorer=`.
    #[serde(default)]
    pub scorer: Scorer,
}

#[derive(Deserialize)]
pub struct MatchQuery {
    scorer: Option<String>,
}

fn default_top() -> usize {
    10
}

async fn rank(State(s): State<AppState>, Query(q): Query<MatchQuery>, Json(mut req): Json<MatchRequest>) -> ApiResult {
    validate_match(&req)?;
    static MATCH_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let permit = MATCH_GATE.try_acquire().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            crate::copy_extra::msg("api.error.matcher_busy", json!({})),
        )
    })?;
    if let Some(name) = q.scorer {
        req.scorer = Scorer::parse(&name).ok_or_else(|| {
            ApiError(
                StatusCode::BAD_REQUEST,
                crate::copy_extra::msg("api.error.scorer_unknown", json!({"name": name})),
            )
        })?;
    }
    let matcher = s.matcher.clone();
    let body = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        views::matches(&matcher, &req)
    })
    .await
    .map_err(internal)?;
    Ok(Json(body))
}

fn validate_match(req: &MatchRequest) -> Result<(), ApiError> {
    if req.present.len() + req.excluded.len() > 128 || req.present.iter().chain(&req.excluded).any(|id| id.len() > 64) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            crate::copy_extra::msg("api.error.phenotype_bounds", json!({})),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[tokio::test]
    async fn intake_limit_override_keeps_global_csrf_body_and_uri_limits() {
        use axum::{
            body::{Body, Bytes},
            http::Request,
        };
        use tower::ServiceExt;
        let app = secure_routes(
            Router::new()
                .route("/ordinary", post(|_: Bytes| async { StatusCode::OK }))
                .merge(intake_router())
                .merge(search_router())
                .with_state(crate::test_support::state()),
        );
        let text = format!("STXBP1 {}", " ".repeat(70 * 1024));
        let payload = json!({"text": text}).to_string();
        for (path, csrf, expected) in [
            ("/api/intake", true, StatusCode::OK),
            ("/api/search", true, StatusCode::OK),
            ("/ordinary", true, StatusCode::PAYLOAD_TOO_LARGE),
            ("/api/intake", false, StatusCode::FORBIDDEN),
            ("/api/search", false, StatusCode::FORBIDDEN),
        ] {
            let mut request = Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, "lang=en");
            if csrf {
                request = request.header("x-atlas-csrf", "1");
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::from(payload.clone())).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}, csrf={csrf}");
        }
        let response = app
            .oneshot(
                Request::get(format!("/api/intake/processor?q={}", "x".repeat(4096)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::URI_TOO_LONG);
    }

    #[tokio::test]
    async fn lookup_post_preserves_csrf_limits_and_no_store() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let app = secure_routes(search_router().with_state(crate::test_support::state()));
        for (csrf, bytes, expected) in [(true, 0, StatusCode::OK), (false, 0, StatusCode::FORBIDDEN), (true, 2048, StatusCode::PAYLOAD_TOO_LARGE)] {
            // Unknown fields are rejected; use whitespace padding to exercise the body cap.
            let body = if bytes == 0 { json!({"q":"STXBP1"}).to_string() } else { format!("{}{}", json!({"q":"STXBP1"}), " ".repeat(bytes)) };
            let mut req = Request::post("/api/search/lookup").header(header::CONTENT_TYPE,"application/json").header(header::COOKIE,"fixture=1");
            if csrf { req = req.header("x-atlas-csrf","1"); }
            let response = app.clone().oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
            assert_eq!(response.status(), expected);
            if csrf { assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store"); }
        }
        assert_eq!(app.oneshot(Request::get("/api/search/lookup").body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn phenotype_work_is_bounded_before_ranking() {
        let mut req: MatchRequest = serde_json::from_value(json!({"present": ["HP:0001250"]})).unwrap();
        assert!(validate_match(&req).is_ok());
        req.excluded = vec!["HP:0001250".into(); 128];
        assert!(validate_match(&req).is_err());
        req.excluded = vec!["x".repeat(65)];
        assert!(validate_match(&req).is_err());
    }
    #[test]
    fn internal_errors_do_not_expose_paths() {
        let error = internal("secret/path/database.sqlite");
        assert_eq!(error.1["key"], "api.error.internal");
        assert!(!error.1.to_string().contains("secret/path"));
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;
    #[tokio::test]
    async fn search_error_keys_render_without_breaking_legacy_details() {
        let response = ApiError(StatusCode::BAD_REQUEST, "search.error.too_long".into()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["detail_msg"]["key"], "search.error.too_long");
        assert_eq!(body["detail"], body["detail_msg"]["fallback"]);
        assert_eq!(body["msg"], body["detail_msg"]);
        assert!(crate::language_contract::offenders(&body).is_empty());
    }
    #[tokio::test]
    async fn legacy_text_errors_keep_their_detail_and_status() {
        for error in [
            not_found("source record absent"),
            ApiError(
                StatusCode::CONFLICT,
                "source record changed; rebuild and reverify".into(),
            ),
        ] {
            let status = error.0;
            let expected = error.1.clone();
            let response = error.into_response();
            assert_eq!(response.status(), status);
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["detail"], expected);
            assert!(body["detail_msg"].is_null());
        }
    }
    #[tokio::test]
    async fn error_response_has_a_key_and_keeps_the_identifier_as_data() {
        let response = not_found(crate::copy_extra::msg(
            "api.error.item_unknown",
            json!({"id": "MONDO:missing"}),
        ))
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["detail_msg"]["key"], "api.error.item_unknown");
        assert_eq!(body["detail_msg"]["params"]["id"], "MONDO:missing");
        assert_eq!(body["detail"], body["detail_msg"]["fallback"]);
        assert!(crate::language_contract::offenders(&body).is_empty());
    }
}
