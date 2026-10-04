//! Typed errors and their HTTP form: `{"detail": "...", "code": "..."}` (same `detail` key as the
//! rest of the API). Messages never echo request input, so they can't leak a password or token.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AccountsError {
    #[error("not signed in")]
    Unauthorized,
    #[error("email or password is wrong")]
    BadCredentials,
    #[error("{0}")]
    Invalid(String),
    #[error("not found")]
    NotFound,
    #[error("not allowed")]
    Forbidden,
    #[error("{0}")]
    Conflict(String),
    #[error("too many attempts, try again later")]
    RateLimited { retry_after_secs: u64 },
    #[error("accounts are unavailable on this server")]
    Unavailable,
    #[error("account email request failed")]
    EmailFlow {
        code: crate::email::EmailErrorCode,
        locale: crate::email::EmailLocale,
    },
    #[error("storage error")]
    Storage(#[from] rusqlite::Error),
    #[error("internal error")]
    Internal(String),
}

pub type Result<T, E = AccountsError> = std::result::Result<T, E>;

impl AccountsError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized | Self::BadCredentials => StatusCode::UNAUTHORIZED,
            Self::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::EmailFlow { code, .. } => match code {
                crate::email::EmailErrorCode::Unverified => StatusCode::FORBIDDEN,
                crate::email::EmailErrorCode::InvalidToken => StatusCode::UNPROCESSABLE_ENTITY,
                crate::email::EmailErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            },
            Self::Storage(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code; the web UI maps it to a translated message.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::BadCredentials => "bad_credentials",
            Self::Invalid(_) => "invalid",
            Self::NotFound => "not_found",
            Self::Forbidden => "forbidden",
            Self::Conflict(_) => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::EmailFlow { code, .. } => code.code(),
            Self::Storage(_) | Self::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for AccountsError {
    fn into_response(self) -> Response {
        match &self {
            // Log the cause server-side; SQLite messages name tables and constraints, never values.
            Self::Storage(e) => eprintln!("atlas-accounts: storage error: {e}"),
            Self::Internal(e) => eprintln!("atlas-accounts: internal error: {e}"),
            _ => {}
        }
        let message = if let Self::EmailFlow { code, locale } = &self {
            crate::email::message(code.code(), *locale)
        } else {
            crate::copy::msg(&format!("account.error.{}", self.code()), json!({}))
        };
        let detail = if matches!(self, Self::EmailFlow { .. }) {
            message["fallback"]
                .as_str()
                .unwrap_or("Account email request failed.")
                .to_owned()
        } else {
            self.to_string()
        };
        let mut res = (
            self.status(),
            Json(json!({ "detail": detail, "code": self.code(), "detail_msg": message })),
        )
            .into_response();
        if let Self::RateLimited { retry_after_secs } = self
            && let Ok(v) = HeaderValue::from_str(&retry_after_secs.max(1).to_string())
        {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        res
    }
}
