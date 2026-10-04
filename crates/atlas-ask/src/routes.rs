//! HTTP: `GET /api/ask/intents`, `GET /api/ask/connections`, `POST /api/ask` (JSON, or SSE with
//! `Accept: text/event-stream` / `?stream=1`).

use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::AskState;
use crate::converse::{AskRequest, AskResponse, Event};
use crate::intent::INTENTS;
use crate::store::session_token;

const MAX_QUESTION: usize = 2000;

pub fn router(state: AskState) -> Router {
    Router::new()
        .route("/api/ask", post(ask))
        .route("/api/ask/intents", get(intents))
        .route("/api/ask/connections", get(connections))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn(atlas_accounts::csrf::guard))
        .with_state(state)
}

fn bad_request(msg: serde_json::Value) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "detail": msg["fallback"], "detail_msg": msg })),
    )
        .into_response()
}

async fn intents() -> Json<serde_json::Value> {
    let list: Vec<serde_json::Value> = INTENTS
        .iter()
        .map(|i| {
            let mut v = i.tool();
            v["slots"] = json!(i.slots);
            if !i.one_of.is_empty() {
                v["one_of"] = json!(i.one_of);
            }
            v
        })
        .collect();
    Json(json!({ "intents": list }))
}

#[derive(Deserialize)]
struct ConnParams {
    #[serde(default)]
    probe: Option<String>,
}

/// The connections the provider picker offers (same data as `/api/llm/connections`).
async fn connections(State(s): State<AskState>, Query(p): Query<ConnParams>) -> Json<serde_json::Value> {
    let probe = matches!(p.probe.as_deref(), Some("1" | "true"));
    let list = s.asker.llm.list_connections(probe).await;
    let default = crate::converse::default_connection_for(&s.asker.llm);
    let default_model_label = list
        .iter()
        .find(|c| c.name == default)
        .and_then(|c| c.model_label.clone());
    Json(json!({ "default": default, "default_model_label": default_model_label, "connections": list }))
}

#[derive(Deserialize)]
struct AskParams {
    #[serde(default)]
    stream: Option<String>,
}

async fn ask(
    State(s): State<AskState>,
    Query(p): Query<AskParams>,
    headers: HeaderMap,
    extensions: axum::http::Extensions,
    Json(mut req): Json<AskRequest>,
) -> Response {
    if req.history.len() > 32
        || req.history.iter().any(|t| t.text.len() > 8000)
        || req.chips.as_ref().is_some_and(|c| c.len() > crate::converse::MAX_CHIPS)
    {
        return bad_request(crate::copy::msg("ask.error.context_too_large", json!({})));
    }
    let has_chips = req.chips.as_ref().is_some_and(|c| !c.is_empty());
    if req.question.trim().is_empty() && !has_chips {
        return bad_request(crate::copy::msg("ask.error.empty_question", json!({})));
    }
    if req.question.chars().count() > MAX_QUESTION {
        return bad_request(crate::copy::msg(
            "ask.error.question_too_long",
            json!({"limit": MAX_QUESTION}),
        ));
    }
    let user = s
        .store
        .as_ref()
        .zip(session_token(&headers))
        .and_then(|(st, t)| st.user_for_token(&t));
    let forwarded = atlas_accounts::AccountsConfig::from_env()
        .trust_forwarded_for
        .then(|| {
            headers
                .get("x-forwarded-for")?
                .to_str()
                .ok()?
                .split(',')
                .next()?
                .trim()
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .flatten();
    let socket = extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip());
    let identity = user
        .as_ref()
        .map(|u| format!("user:{u}"))
        .or_else(|| forwarded.or(socket).map(|ip| format!("ip:{ip}")))
        .unwrap_or_else(|| "unknown".into());
    req.visitor = Some(atlas_llm::visitor_key(&identity));
    let stream = matches!(p.stream.as_deref(), Some("1" | "true"))
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"));
    if !stream {
        let mut out = s.asker.ask(&req, None).await;
        save(&s, user.as_deref(), &req, &mut out);
        return Json(out).into_response();
    }
    let (tx, rx) = mpsc::channel::<Event>(16);
    let state = s.clone();
    tokio::spawn(async move {
        let mut out = state.asker.ask(&req, Some(&tx)).await;
        save(&state, user.as_deref(), &req, &mut out);
        let _ = tx
            .send(Event::Answer {
                response: Box::new(out),
            })
            .await;
    });
    let events = ReceiverStream::new(rx).map(|e| {
        let name = match &e {
            Event::Status { .. } => "status",
            Event::Chips { .. } => "chips",
            Event::Results { .. } => "results",
            Event::Answer { .. } => "answer",
            Event::Error { .. } => "error",
        };
        let ev = SseEvent::default()
            .event(name)
            .json_data(&e)
            .unwrap_or_else(|err| SseEvent::default().event("error").data(err.to_string()));
        Ok::<_, Infallible>(ev)
    });
    Sse::new(events).keep_alive(KeepAlive::default()).into_response()
}

/// Save for signed-in users (unless the request opts out or only planned).
fn save(s: &AskState, user: Option<&str>, req: &AskRequest, out: &mut AskResponse) {
    let (Some(store), Some(user)) = (s.store.as_ref(), user) else {
        return;
    };
    if req.save == Some(false) || out.planned_only {
        return;
    }
    match store.save(user, req.conversation_id.as_deref(), out) {
        Ok(id) => {
            out.conversation_id = Some(id);
            out.saved = true;
        }
        Err(_) => {
            let msg = crate::copy::msg("ask.response.save_failed", json!({}));
            out.notes.push(msg["fallback"].as_str().unwrap_or_default().to_owned());
            out.notes_msg.push(msg);
        }
    }
}
