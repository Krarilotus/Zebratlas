//! Typed errors. HTTP bodies are `{"detail": ...}` like the rest of the API; they never echo secrets.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::model::State;

#[derive(Debug, thiserror::Error)]
pub enum ContribError {
    /// The submission or review input breaks a rule; the message says which, in plain words.
    #[error("{0}")]
    Invalid(String),
    #[error("contribution not found")]
    NotFound,
    /// A state change the review workflow does not allow (e.g. accepting before the auto-checks ran).
    #[error("cannot {action} a contribution that is {from}")]
    Transition { from: State, action: &'static str },
    /// Accepting is blocked by a failed auto-check (duplicate of a curated edge, missing target, ...).
    #[error("cannot accept: {0}")]
    Blocked(String),
    /// Someone else changed the contribution between read and write (optimistic version check).
    #[error("the contribution changed meanwhile; reload and try again")]
    Stale,
    #[error("sign-in as a reviewer is needed")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("too many contributions from this connection; try again in {retry_after_secs} s")]
    RateLimited { retry_after_secs: u64 },
    #[error("storage unavailable")]
    Storage(#[from] rusqlite::Error),
    #[error("internal error")]
    Internal(String),
}

impl ContribError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Transition { .. } | Self::Blocked(_) | Self::Stale => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Storage(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub type Result<T> = std::result::Result<T, ContribError>;

impl IntoResponse for ContribError {
    fn into_response(self) -> Response {
        let status = self.status();
        let (key, params) = match &self {
            Self::Invalid(_) => ("invalid", json!({})),
            Self::NotFound => ("not_found", json!({})),
            Self::Transition { from, action } => ("transition", json!({"from": from, "action": action})),
            Self::Blocked(_) => ("blocked", json!({})),
            Self::Stale => ("stale", json!({})),
            Self::Unauthorized => ("unauthorized", json!({})),
            Self::Forbidden(_) => ("forbidden", json!({})),
            Self::RateLimited { retry_after_secs } => ("rate_limited", json!({"seconds": retry_after_secs})),
            Self::Storage(_) | Self::Internal(_) => ("internal", json!({})),
        };
        let msg = crate::copy::msg(&format!("contribute.error.{key}"), params);
        // Storage and internal details stay in the server log, not in the response.
        let detail = match &self {
            Self::Storage(_) | Self::Internal(_) => {
                eprintln!("atlas-contrib: {self}");
                "internal error".to_string()
            }
            other => other.to_string(),
        };
        let mut res = (
            status,
            Json(json!({ "detail": detail, "detail_msg": msg, "code": key })),
        )
            .into_response();
        if let Self::RateLimited { retry_after_secs } = self
            && let Ok(v) = HeaderValue::from_str(&retry_after_secs.to_string())
        {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        res
    }
}
