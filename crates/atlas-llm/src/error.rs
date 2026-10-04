//! Typed errors. Messages never contain keys; provider bodies are truncated.

use std::time::Duration;

/// Everything a provider call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Unknown connection name in the registry.
    #[error("unknown connection '{0}'")]
    UnknownConnection(String),

    /// The connection needs a key and none was given (per request or env).
    #[error("connection '{connection}' needs an API key ({hint})")]
    MissingKey { connection: String, hint: String },

    /// The request is not valid for this connection (e.g. a setting it cannot enforce).
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The CLI or server is not installed / not reachable / not signed in.
    #[error("provider unavailable: {0}")]
    Unavailable(String),

    /// The provider rejected the credentials (401/403 or CLI sign-in expired).
    #[error("authentication failed: {0}")]
    Auth(String),

    /// 429 or provider-side quota.
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// The hosted free tier (D23) refused: a limit, the daily budget or the kill switch.
    /// Show it as is; the UI offers "bring your own key or try later".
    #[error("free quota reached: {reason}; bring your own key or try later")]
    FreeQuota {
        reason: crate::free_tier::QuotaReason,
        /// Seconds until a retry can succeed, when known.
        retry_after_secs: Option<u64>,
    },

    /// The deadline passed; the process was killed / the request dropped.
    /// The call may still have reached the provider; it is never replayed automatically.
    #[error("deadline of {0:?} exceeded (not replayed)")]
    Timeout(Duration),

    /// Non-success HTTP status or a provider error payload.
    #[error("provider error (status {status:?}): {message}")]
    Provider { status: Option<u16>, message: String },

    /// The provider answered, but not in a shape we can use.
    #[error("unexpected provider output: {0}")]
    BadOutput(String),

    /// The output did not satisfy the JSON schema, also after the one retry.
    #[error("structured output failed validation after retry: {0}")]
    SchemaValidation(String),

    /// The schema itself is not a valid JSON schema.
    #[error("invalid JSON schema: {0}")]
    InvalidSchema(String),

    /// Offline mode and the request is not in the cache.
    #[error("cache miss in offline mode (key {0})")]
    CacheMiss(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = LlmError> = std::result::Result<T, E>;

/// Cut a provider body so errors stay readable (and never carry whole prompts around).
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes)", &s[..end], s.len())
}
