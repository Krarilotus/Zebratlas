//! Optional router: server/search-intake owners can merge without changing existing ask contracts.
use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct QueryState {
    pub engine: Arc<QueryEngine>,
    pub asker: crate::Asker,
}
pub fn router(state: QueryState) -> Router {
    Router::new()
        .route("/api/ask/schema", get(schema))
        .route("/api/ask/query", post(query))
        .route("/api/ask/query/suggestions", post(suggestions))
        .route("/api/ask/query/mcp", post(mcp))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn(atlas_accounts::csrf::guard))
        .with_state(state)
}
async fn schema(State(s): State<QueryState>) -> Json<Value> {
    Json(
        json!({"schema":s.engine.schema,"plan_schema":QueryPlan::schema(&[],&s.engine.relations()),"tools":s.engine.tools()}),
    )
}
async fn query(
    State(s): State<QueryState>,
    Json(req): Json<conversation::QueryRequest>,
) -> Result<Json<conversation::QueryAnswer>, (StatusCode, Json<Value>)> {
    // HTTP callers cannot assert that invented IDs are linked. The search owner supplies links
    // directly to understand(); this endpoint accepts edited plans only, with graph-owned IDs.
    if req.plan.is_none() {
        return Err(query_error("edited plans only".into()));
    }
    s.asker
        .understand_query(&s.engine, &req, None)
        .await
        .map(Json)
        .map_err(query_error)
}
fn query_error(diagnostic: String) -> (StatusCode, Json<Value>) {
    let message = crate::copy::msg("ask.error.generic", json!({}));
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"detail":message["fallback"],"detail_msg":message,"diagnostic":diagnostic})),
    )
}
async fn suggestions(
    State(s): State<QueryState>,
    Json(req): Json<suggestions::SuggestionRequest>,
) -> Result<Json<suggestions::Suggestions>, (StatusCode, Json<Value>)> {
    s.asker.query_suggestions(&req).map(Json).map_err(query_error)
}
#[derive(Deserialize)]
struct Rpc {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}
async fn mcp(State(s): State<QueryState>, headers: HeaderMap, Json(r): Json<Rpc>) -> Response {
    // A stateless JSON-only Streamable HTTP subset, pinned to the published 2025-11-25 protocol.
    if let Some(origin) = headers.get("origin") {
        let valid = origin
            .to_str()
            .ok()
            .and_then(|v| reqwest::Url::parse(v).ok())
            .and_then(|u| u.host_str().map(str::to_owned));
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(':').next());
        if valid.as_deref() != host {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    if r.jsonrpc != "2.0" {
        return Json(json!({"jsonrpc":"2.0","id":r.id,"error":{"code":-32600,"message":"Invalid Request"}}))
            .into_response();
    }
    if r.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let result: Result<Value, String> = match r.method.as_str() {
        "initialize" => Ok(
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"atlas-query","version":"0.1.0"}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":s.engine.tools()})),
        "tools/call" => {
            let args = r.params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            Ok(
                match s.engine.dispatch(r.params["name"].as_str().unwrap_or(""), args).await {
                    Ok(v) => {
                        json!({"content":[{"type":"text","text":v.to_string()}],"structuredContent":v,"isError":false})
                    }
                    Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
                },
            )
        }
        _ => Err("Method not found".into()),
    };
    Json(match result {
        Ok(v) => json!({"jsonrpc":"2.0","id":r.id,"result":v}),
        Err(e) => json!({"jsonrpc":"2.0","id":r.id,"error":{"code":-32601,"message":e}}),
    })
    .into_response()
}
