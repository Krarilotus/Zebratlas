//! Local connector: lets a *hosted* atlas UI use the subscription CLIs (and local model servers)
//! on the user's own machine. The browser calls `http://127.0.0.1:<port>`; the hosted server is
//! never involved and never sees the user's login.
//!
//! ```text
//! cargo run -p atlas-llm --example connector -- --origin https://atlas.example.org [--port 8765] [--max-parallel 1]
//! ```
//!
//! - binds 127.0.0.1 only; CORS allows exactly `--origin`;
//! - a pairing token is printed once at start; every request needs `Authorization: Bearer <token>`;
//! - only connections that run on this machine (CLIs, localhost servers) are exposed;
//! - same restrictions and probes as in-process (it is the same `atlas-llm` code);
//! - at most `--max-parallel` (default 1) calls at a time; others get 429, never queued silently;
//! - a call that times out or is cancelled is not replayed.
//!
//! API: `GET /v1/connections` → `[ConnectionInfo]` (probed);
//! `POST /v1/complete` `{ "connection": "...", "request": CompletionRequest, "inputs": [..] }` →
//! `Completion` (response + PROV-O record).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;

use atlas_llm::{Call, CompletionRequest, Llm, LlmError};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;

struct App {
    llm: Llm,
    token: String,
    gate: Semaphore,
}

#[derive(Deserialize)]
struct CompleteBody {
    connection: String,
    request: CompletionRequest,
    #[serde(default)]
    inputs: Vec<String>,
}

/// 256 bits from the OS-seeded std hasher keys (no extra dependency).
fn pairing_token() -> String {
    (0..4)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn authorised(app: &App, headers: &HeaderMap) -> bool {
    let Some(got) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let got = got.strip_prefix("Bearer ").unwrap_or_default().as_bytes();
    let want = app.token.as_bytes();
    // Constant-time compare.
    got.len() == want.len() && got.iter().zip(want).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn local_only(c: &atlas_llm::ConnectionInfo) -> bool {
    c.kind.is_cli()
        || c.base_url.as_deref().is_some_and(|u| {
            u.starts_with("http://localhost") || u.starts_with("http://127.0.0.1") || u.starts_with("http://[::1]")
        })
}

fn error(status: StatusCode, e: &str) -> Response {
    (status, Json(json!({ "error": e }))).into_response()
}

async fn connections(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorised(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "pairing token required");
    }
    let list: Vec<_> = app
        .llm
        .list_connections(true)
        .await
        .into_iter()
        .filter(local_only)
        .collect();
    Json(list).into_response()
}

async fn complete(State(app): State<Arc<App>>, headers: HeaderMap, Json(body): Json<CompleteBody>) -> Response {
    if !authorised(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "pairing token required");
    }
    let Ok(conn) = app.llm.registry().get(&body.connection) else {
        return error(StatusCode::NOT_FOUND, "unknown connection");
    };
    if !local_only(&conn.info(None)) {
        return error(
            StatusCode::FORBIDDEN,
            "the connector only runs connections on this machine",
        );
    }
    let Ok(_permit) = app.gate.try_acquire() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "a call is already running");
    };
    let call = Call::new(body.connection).with_inputs(body.inputs);
    match app.llm.complete(&call, body.request).await {
        Ok(done) => Json(done).into_response(),
        Err(e) => {
            let status = match e {
                LlmError::InvalidRequest(_) | LlmError::InvalidSchema(_) => StatusCode::BAD_REQUEST,
                LlmError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
                LlmError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
                LlmError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_GATEWAY,
            };
            error(status, &e.to_string())
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut origin = None;
    let mut port: u16 = 8765;
    let mut max_parallel = 1usize;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--origin" => origin = args.next(),
            "--port" => port = args.next().unwrap_or_default().parse()?,
            "--max-parallel" => max_parallel = args.next().unwrap_or_default().parse()?,
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let origin = origin.ok_or_else(|| anyhow::anyhow!("--origin <atlas web origin> is required"))?;
    let cors = CorsLayer::new()
        .allow_origin(HeaderValue::from_str(&origin)?)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    let app = Arc::new(App {
        llm: Llm::from_env()?,
        token: pairing_token(),
        gate: Semaphore::new(max_parallel.max(1)),
    });
    println!("atlas connector on http://127.0.0.1:{port} for {origin}");
    println!("pairing token (paste into the atlas settings): {}", app.token);
    let router = Router::new()
        .route("/v1/connections", get(connections))
        .route("/v1/complete", post(complete))
        .layer(cors)
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
