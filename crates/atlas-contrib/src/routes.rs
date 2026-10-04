//! HTTP API.
//!
//! Public:
//! - `POST /api/contribute`: submit (202, `{id, state, contribution}`); checks start in the background;
//! - `GET  /api/contribute?node=&edge=&state=&limit=`: contributions about a node or edge (public view);
//! - `GET  /api/contribute/stats`: counts per state;
//! - `GET  /api/contribute/overlay`: accepted contributions as the `user_asserted` overlay;
//! - `GET  /api/contribute/{id}`: one contribution (state, checks, decision; never the contact);
//! - `GET  /api/contribute/{id}/prov`: its PROV-O history (JSON-LD).
//!
//! Reviewers (`Authorization: Bearer <token>` or a listed signed-in account):
//! - `GET  /api/review?state=`: the queue, with contributor contacts;
//! - `GET  /api/review/me`: `{reviewer: bool}`;
//! - `GET  /api/review/{id}`, `POST /api/review/{id}` `{decision, reason}`, `POST /api/review/{id}/recheck`.

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{Extensions, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{client_key, reviewer};
use crate::error::{ContribError, Result};
use crate::model::{Agent, ReviewInput, State as ReviewState, Submission};
use crate::service::ContribState;
use crate::store::ListFilter;

/// Routes under `/api/contribute` and `/api/review`; merge into the server's router.
pub fn router(state: ContribState) -> Router {
    Router::new()
        .route("/api/contribute", post(submit).get(list_public))
        .route("/api/contribute/schema", get(schema))
        .route("/api/contribute/stats", get(stats))
        .route("/api/contribute/overlay", get(overlay))
        .route("/api/contribute/discovery-candidates", get(discovery_candidates))
        .route("/api/contribute/{id}", get(get_public))
        .route("/api/contribute/{id}/prov", get(prov))
        .route("/api/review", get(queue))
        .route("/api/review/me", get(me))
        .route("/api/review/{id}", get(get_full).post(decide))
        .route("/api/review/{id}/recheck", post(recheck))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-store"),
                );
                response
            },
        ))
        .with_state(state)
}

type ApiResult = Result<Json<Value>>;

async fn schema() -> Json<Value> {
    Json(crate::schema::document())
}

async fn submit(
    State(s): State<ContribState>,
    headers: HeaderMap,
    extensions: Extensions,
    body: std::result::Result<Json<Submission>, axum::extract::rejection::JsonRejection>,
) -> Result<impl IntoResponse> {
    let Json(sub) =
        body.map_err(|e| ContribError::invalid(format!("the form could not be read: {}", e.body_text())))?;
    s.limiter
        .hit(&client_key(s.config.trust_forwarded_for, &headers, &extensions))?;
    let user = s.users.as_ref().and_then(|r| r(&headers));
    let c = s.submit(sub, user)?;
    if s.config.check_on_submit {
        let state = s.clone();
        let id = c.id.clone();
        tokio::spawn(async move {
            if let Err(e) = state.run_checks(&id).await {
                eprintln!("atlas-contrib: checks for {id}: {e}");
            }
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "id": c.id, "state": c.state, "contribution": c.public() })),
    ))
}

#[derive(Deserialize)]
struct ListQuery {
    node: Option<String>,
    edge: Option<String>,
    /// Comma-separated states.
    state: Option<String>,
    limit: Option<u32>,
}

fn filter(q: ListQuery, default_limit: u32) -> Result<ListFilter> {
    let states = match q.state.as_deref() {
        None | Some("") => Vec::new(),
        Some(s) => s
            .split(',')
            .map(|x| ReviewState::parse(x.trim()).ok_or_else(|| ContribError::invalid(format!("unknown state {x}"))))
            .collect::<Result<_>>()?,
    };
    Ok(ListFilter {
        states,
        node: q.node.filter(|s| !s.is_empty()),
        edge: q.edge.filter(|s| !s.is_empty()),
        limit: Some(q.limit.unwrap_or(default_limit).clamp(1, 500)),
    })
}

async fn list_public(State(s): State<ContribState>, Query(q): Query<ListQuery>) -> ApiResult {
    let items: Vec<Value> = s.list(&filter(q, 50)?)?.iter().map(|c| c.public()).collect();
    Ok(Json(json!({ "items": items })))
}

async fn stats(State(s): State<ContribState>) -> ApiResult {
    Ok(Json(json!({ "counts": s.counts()? })))
}

async fn discovery_candidates(State(s): State<ContribState>) -> ApiResult {
    Ok(Json(s.discovery_candidates()?))
}

async fn overlay(State(s): State<ContribState>) -> ApiResult {
    let o = s.overlay()?;
    Ok(Json(
        serde_json::to_value(o).map_err(|e| ContribError::Internal(e.to_string()))?,
    ))
}

async fn get_public(State(s): State<ContribState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.get(&id)?.public()))
}

async fn prov(State(s): State<ContribState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.prov_jsonld(&id)?))
}

fn require_reviewer(s: &ContribState, headers: &HeaderMap) -> Result<Agent> {
    reviewer(&s.config, s.users.as_ref(), headers)
}

async fn me(State(s): State<ContribState>, headers: HeaderMap) -> ApiResult {
    let agent = require_reviewer(&s, &headers).ok();
    Ok(Json(
        json!({ "reviewer": agent.is_some(), "agent": agent.map(|a| a.id) }),
    ))
}

async fn queue(State(s): State<ContribState>, headers: HeaderMap, Query(q): Query<ListQuery>) -> ApiResult {
    require_reviewer(&s, &headers)?;
    let mut f = filter(q, 200)?;
    if f.states.is_empty() {
        f.states = vec![ReviewState::Submitted, ReviewState::AutoChecked];
    }
    let items = s.list(&f)?;
    Ok(Json(json!({ "items": items, "counts": s.counts()? })))
}

async fn get_full(State(s): State<ContribState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    require_reviewer(&s, &headers)?;
    let c = s.get(&id)?;
    let prov = s.prov_events(&id)?;
    Ok(Json(json!({ "contribution": c, "history": prov })))
}

async fn decide(
    State(s): State<ContribState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: std::result::Result<Json<ReviewInput>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let agent = require_reviewer(&s, &headers)?;
    let Json(input) =
        body.map_err(|e| ContribError::invalid(format!("the decision could not be read: {}", e.body_text())))?;
    let c = s.review(&id, &input, agent)?;
    Ok(Json(json!({ "contribution": c })))
}

async fn recheck(State(s): State<ContribState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    require_reviewer(&s, &headers)?;
    let c = s.run_checks(&id).await?;
    Ok(Json(json!({ "contribution": c })))
}
