//! HTTP API of privacy requests.
//!
//! Public (no account):
//! - `POST /api/privacy/requests` `{email, concerns, type}` → 202 `{reference, received_at, respond_by}`.
//!
//! Reviewers (same auth as `/api/review`):
//! - `GET  /api/privacy/review?state=received,verifying` → queue (default: open requests);
//! - `GET  /api/privacy/review/{reference}` → request + derived trace parameters + trace;
//! - `POST /api/privacy/review/{reference}/trace` `{orcid?, name?, affiliation?, email?, node?}` → trace;
//! - `POST /api/privacy/review/{reference}` `{decision: verifying|approve|reject, reason?, trace?, nodes?, suppress?}`.
//!
//! Reviewer responses carry personal data: `Cache-Control: no-store`.

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{Extensions, HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::copy;
use super::{DecisionInput, PrivacyInput, PrivacyState, RequestState, TraceParams};
use crate::auth::{client_key, reviewer};
use crate::error::ContribError;
use crate::model::Agent;

struct PrivacyError(ContribError);
type Result<T> = std::result::Result<T, PrivacyError>;

impl From<ContribError> for PrivacyError {
    fn from(error: ContribError) -> Self {
        Self(error)
    }
}

impl IntoResponse for PrivacyError {
    fn into_response(self) -> Response {
        let message = copy::error(&self.0);
        let payload = json!({"detail": message.fallback, "message_key": message.key, "message": message});
        let mut response = self.0.into_response();
        *response.body_mut() = axum::body::Body::from(payload.to_string());
        response.headers_mut().remove(header::CONTENT_LENGTH);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

pub fn router(state: PrivacyState) -> Router {
    Router::new()
        .route("/api/privacy/requests", post(submit))
        .route("/api/privacy/review", get(queue))
        .route("/api/privacy/review/{reference}", get(detail).post(decide))
        .route("/api/privacy/review/{reference}/trace", post(trace))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .with_state(state)
}

fn private(v: Value) -> Response {
    let mut res = Json(v).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn require_reviewer(s: &PrivacyState, headers: &HeaderMap) -> Result<Agent> {
    Ok(reviewer(&s.contrib, s.users.as_ref(), headers)?)
}

async fn submit(
    State(s): State<PrivacyState>,
    headers: HeaderMap,
    extensions: Extensions,
    body: std::result::Result<Json<PrivacyInput>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    s.limiter
        .hit(&client_key(s.contrib.trust_forwarded_for, &headers, &extensions))?;
    let Json(input) =
        body.map_err(|e| ContribError::invalid(format!("the form could not be read: {}", e.body_text())))?;
    let kind = input.kind;
    let ack = match s.submit(input)? {
        Some(r) => json!({
            "reference": r.reference,
            "type": r.kind,
            "received_at": r.created_at,
            "respond_by": r.respond_by,
            "message_key": "privacy.request.received",
            "message": copy::received(&r.reference),
            "respond_by_message": copy::respond_by(&r.respond_by),
        }),
        // Honeypot: same shape, nothing stored.
        None => {
            let reference = format!("pr_{}", "0".repeat(20));
            let now = std::time::SystemTime::now();
            let received_at = humantime::format_rfc3339_seconds(now).to_string();
            let respond_by =
                humantime::format_rfc3339_seconds(now + std::time::Duration::from_secs(30 * 86400)).to_string();
            json!({
                "reference": reference, "type": kind,
                "received_at": received_at, "respond_by": respond_by,
                "message_key": "privacy.request.received",
                "message": copy::received(&reference),
                "respond_by_message": copy::respond_by(&respond_by),
            })
        }
    };
    let mut res = (StatusCode::ACCEPTED, Json(ack)).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Deserialize)]
struct QueueQuery {
    state: Option<String>,
    limit: Option<u32>,
}

async fn queue(State(s): State<PrivacyState>, headers: HeaderMap, Query(q): Query<QueueQuery>) -> Result<Response> {
    require_reviewer(&s, &headers)?;
    let states = match q.state.as_deref() {
        None | Some("") => vec![RequestState::Received, RequestState::Verifying],
        Some("all") => vec![],
        Some(v) => v
            .split(',')
            .map(|x| RequestState::parse(x.trim()).ok_or_else(|| ContribError::invalid(format!("unknown state {x}"))))
            .collect::<crate::error::Result<_>>()?,
    };
    let items = s.queue(&states, q.limit.unwrap_or(200).clamp(1, 500))?;
    Ok(private(json!({ "items": items })))
}

async fn detail(State(s): State<PrivacyState>, headers: HeaderMap, Path(reference): Path<String>) -> Result<Response> {
    require_reviewer(&s, &headers)?;
    Ok(private(s.detail(&reference, None)?))
}

async fn trace(
    State(s): State<PrivacyState>,
    headers: HeaderMap,
    Path(reference): Path<String>,
    body: std::result::Result<Json<TraceParams>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    require_reviewer(&s, &headers)?;
    let Json(p) =
        body.map_err(|e| ContribError::invalid(format!("the trace parameters could not be read: {}", e.body_text())))?;
    Ok(private(s.detail(&reference, Some(p))?))
}

async fn decide(
    State(s): State<PrivacyState>,
    headers: HeaderMap,
    Path(reference): Path<String>,
    body: std::result::Result<Json<DecisionInput>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    let agent = require_reviewer(&s, &headers)?;
    let Json(input) =
        body.map_err(|e| ContribError::invalid(format!("the decision could not be read: {}", e.body_text())))?;
    let r = s.decide(&reference, input, &agent)?;
    Ok(private(json!({ "request": r })))
}
