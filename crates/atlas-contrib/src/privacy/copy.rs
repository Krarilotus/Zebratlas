//! Privacy API messages (D26): stable keys/parameters and an English fallback.
use crate::error::ContribError;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub(super) struct Msg {
    pub key: &'static str,
    pub params: Value,
    pub fallback: String,
}

pub(super) fn received(reference: &str) -> Msg {
    Msg {
        key: "privacy.request.received",
        params: json!({"reference": reference}),
        fallback: format!("Your request has been received. Your reference is {reference}."),
    }
}

pub(super) fn respond_by(date: &str) -> Msg {
    Msg {
        key: "privacy.request.respond_by",
        params: json!({"date": date}),
        fallback: format!("We will respond by {date}."),
    }
}

pub(super) fn error(error: &ContribError) -> Msg {
    let (key, params, fallback) = match error {
        ContribError::RateLimited { retry_after_secs } => (
            "privacy.request.rate_limited",
            json!({"seconds": retry_after_secs}),
            format!("Too many requests. Try again in {retry_after_secs} seconds."),
        ),
        ContribError::Invalid(_) => (
            "privacy.request.invalid",
            json!({"detail": error.to_string()}),
            error.to_string(),
        ),
        ContribError::NotFound => ("privacy.request.not_found", json!({}), "Request not found.".into()),
        ContribError::Unauthorized => (
            "privacy.request.reviewer_required",
            json!({}),
            "Sign in as an authorised reviewer.".into(),
        ),
        ContribError::Storage(_) | ContribError::Internal(_) => (
            "privacy.request.internal",
            json!({}),
            "Your request could not be processed. Please try again later.".into(),
        ),
        _ => (
            "privacy.request.blocked",
            json!({"detail": error.to_string()}),
            error.to_string(),
        ),
    };
    Msg { key, params, fallback }
}
