//! Search-document preview: reuse the intake parser/redactor without storing uploads or calling a model.
use std::time::Duration;

use atlas_intake::{Input, Limits};
use axum::{
    Json, Router,
    body::Bytes,
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};

use crate::routes::AppState;

#[derive(Debug)]
struct DocumentError(StatusCode, Value);
impl IntoResponse for DocumentError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "detail": self.1["fallback"], "msg": self.1 }))).into_response()
    }
}

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_QUERY_BYTES: usize = 32_000;
static GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/explore/document", post(document))
        .layer(DefaultBodyLimit::max(MAX_BYTES))
        .layer(axum::middleware::map_response(|mut r: Response| async move {
            r.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
            r
        }))
}

fn preview(bytes: &[u8]) -> Result<Value, DocumentError> {
    let limits = Limits {
        max_model_chars: MAX_QUERY_BYTES,
        ..Limits::default()
    };
    let mut prepared = atlas_intake::prepare(Input::File(bytes), &limits).map_err(|e| {
        DocumentError(
            StatusCode::from_u16(e.status()).unwrap_or(StatusCode::UNPROCESSABLE_ENTITY),
            crate::find::msg(e.key(), json!({}), e.to_string()),
        )
    })?;
    if prepared.sent.len() > MAX_QUERY_BYTES {
        let mut end = MAX_QUERY_BYTES;
        while !prepared.sent.is_char_boundary(end) {
            end -= 1;
        }
        prepared.sent.truncate(end);
        prepared.truncated = true;
    }
    Ok(json!({
        "text": prepared.sent, "format": prepared.format.as_str(), "pages": prepared.pages,
        "chars": prepared.chars, "truncated": prepared.truncated, "redactions": prepared.redacted.total(),
    }))
}

async fn document(bytes: Bytes) -> Result<Json<Value>, DocumentError> {
    let permit = GATE.try_acquire().map_err(|_| {
        DocumentError(
            StatusCode::TOO_MANY_REQUESTS,
            crate::copy_extra::msg("api.error.internal", json!({})),
        )
    })?;
    // The permit remains in the worker even if the HTTP timeout elapses: no unbounded parser backlog.
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        std::panic::catch_unwind(|| preview(&bytes)).unwrap_or_else(|_| {
            Err(DocumentError(
                StatusCode::UNPROCESSABLE_ENTITY,
                crate::find::msg("intake.error.unreadable", json!({}), "The document could not be read."),
            ))
        })
    });
    let value = tokio::time::timeout(Duration::from_secs(30), work)
        .await
        .map_err(|_| {
            DocumentError(
                StatusCode::REQUEST_TIMEOUT,
                crate::copy_extra::msg("api.error.internal", json!({})),
            )
        })?
        .map_err(|_| {
            DocumentError(
                StatusCode::UNPROCESSABLE_ENTITY,
                crate::find::msg("intake.error.unreadable", json!({}), "The document could not be read."),
            )
        })??;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_preview_redacts_and_preserves_gene_terms() {
        let value = preview(b"Patient name: Jane Smith\nEmail: jane@example.org\nSTXBP1 developmental epilepsy")
            .unwrap_or_else(|error| panic!("{}", error.1));
        let text = value["text"].as_str().unwrap();
        assert!(text.contains("STXBP1"));
        assert!(!text.contains("jane@example.org"));
        assert!(!text.contains("Jane Smith"));
        assert!(value["redactions"].as_u64().unwrap() >= 2);
    }

    #[test]
    fn preview_respects_query_utf8_byte_limit() {
        let value =
            preview(format!("STXBP1 {}", "ü".repeat(20_000)).as_bytes()).unwrap_or_else(|error| panic!("{}", error.1));
        assert!(value["text"].as_str().unwrap().len() <= MAX_QUERY_BYTES);
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn unsupported_image_is_rejected() {
        assert_eq!(
            preview(b"\x89PNG\r\n\x1a\n").unwrap_err().0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
}
