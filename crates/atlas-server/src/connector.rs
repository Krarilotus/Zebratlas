//! Production personal-model boundary. Shared registries and the hosted default are immutable.
use crate::routes::{AppState, model_routes};
use atlas_accounts::{AccountsConfig, Store as Accounts, presented_token};
use atlas_connector::{
    graph::{GraphReader, HttpGraph},
    protocol::GraphQuery,
    server::Gateway,
    store::{ModelChoice, Store},
};
use atlas_llm::{Cache, CacheMode, Llm, Registry};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;

pub(crate) struct Config {
    pub accounts: AccountsConfig,
    pub devices: Arc<Store>,
    pub conversations: Option<Arc<dyn atlas_ask::ConversationStore>>,
    pub public_origin: String,
}
impl Config {
    pub fn from_env(state: &AppState) -> anyhow::Result<Self> {
        let accounts = AccountsConfig::from_env();
        // Refuse separate ephemeral account stores: they cannot authenticate the account router.
        anyhow::ensure!(
            !matches!(accounts.database, atlas_accounts::Database::InMemory),
            "connector needs a shared accounts database"
        );
        let path = std::env::var_os("ATLAS_CONNECTOR_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| state.data.join("cache/connector/devices.sqlite"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conversations = atlas_ask::AccountsStore::open(&accounts)
            .ok()
            .map(|s| Arc::new(s) as Arc<dyn atlas_ask::ConversationStore>);
        Ok(Self {
            accounts,
            devices: Arc::new(Store::open(&path)?),
            conversations,
            public_origin: std::env::var("ATLAS_PUBLIC_ORIGIN").unwrap_or_else(|_| "https://zebratlas.org".into()),
        })
    }
}

/// The same closed query set as MCP, using the actual privacy-filtered HTTP handlers.
struct RouterGraph {
    api: Router,
    urls: HttpGraph,
}
#[async_trait::async_trait]
impl GraphReader for RouterGraph {
    async fn read(&self, query: GraphQuery) -> anyhow::Result<Value> {
        let url = self.urls.url(&query)?;
        let uri = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().into(),
        };
        let request = Request::builder().uri(uri).body(Body::empty())?;
        let response = self.api.clone().oneshot(request).await?;
        anyhow::ensure!(response.status().is_success(), "graph read unavailable");
        let bytes = to_bytes(response.into_body(), 256 * 1024).await?;
        atlas_connector::graph::response_entity(url.as_str(), &bytes)
    }
}

#[derive(Clone)]
struct Scope {
    gateway: Gateway,
    state: AppState,
    conversations: Option<Arc<dyn atlas_ask::ConversationStore>>,
}

pub(crate) fn mount(api: Router, state: &AppState, config: Option<Config>) -> Router {
    let gateway =
        config
            .as_ref()
            .map(|c| -> anyhow::Result<Gateway> {
                let graph = api.clone().layer(middleware::from_fn_with_state(
                    state.withhold.clone(),
                    crate::privacy::filter_responses,
                ));
                let reader = Arc::new(RouterGraph {
                    api: graph,
                    urls: HttpGraph::new(&c.public_origin)?,
                });
                Ok(Gateway::new(
                    c.devices.clone(),
                    Arc::new(Accounts::open(&c.accounts.database)?),
                    c.accounts.secure_cookies,
                    reader,
                )
                .with_hosted(
                    state.llm.llm.clone().unwrap_or_else(|| {
                        Arc::new(Llm::new(Registry::default(), Cache::new("unused", CacheMode::Off)))
                    }),
                ))
            })
            .transpose();
    match (gateway, config) {
        // Scope before the inner router matches paths. Re-dispatching a matched request
        // would append path parameters twice (e.g. summary's Path<String>).
        (Ok(Some(gateway)), Some(c)) => Router::new()
            .fallback_service(api.merge(atlas_connector::server::router(gateway.clone())))
            .layer(middleware::from_fn_with_state(
                Scope {
                    gateway,
                    state: state.clone(),
                    conversations: c.conversations,
                },
                scoped,
            )),
        (result, _) => {
            if let Err(e) = result {
                eprintln!("connector unavailable: {e}");
            }
            api.route("/api/connector/{*path}", axum::routing::any(unavailable))
                .route("/api/account/connectors", axum::routing::any(unavailable))
                .route("/api/account/connectors/{*path}", axum::routing::any(unavailable))
        }
    }
}
async fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "connector_unavailable")
}
fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
fn model_path(path: &str) -> bool {
    matches!(
        path,
        "/api/resolve"
            | "/api/search"
            | "/api/explore"
            | "/api/message"
            | "/api/llm/connections"
            | "/api/ask"
            | "/api/ask/connections"
            | "/api/intake"
            | "/api/intake/processor"
    ) || (path.starts_with("/api/condition/") && (path.ends_with("/summary") || path.ends_with("/connections")))
}
fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn header_text(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

async fn scoped(State(scope): State<Scope>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let understand = matches!(path.as_str(), "/api/search" | "/api/resolve" | "/api/intake");
    if !model_path(&path) {
        return next.run(request).await;
    }
    // Deterministic graph reads remain usable when a saved personal model is offline.
    #[derive(serde::Deserialize, Default)]
    struct ModelOptions {
        llm: Option<u8>,
        explain: Option<u8>,
    }
    let options = axum::extract::Query::<ModelOptions>::try_from_uri(request.uri())
        .map(|q| q.0)
        .unwrap_or_default();
    if (path.starts_with("/api/condition/") && path.ends_with("/connections") && options.explain.unwrap_or(0) == 0)
        || (matches!(path.as_str(), "/api/resolve" | "/api/search") && options.llm == Some(0))
    {
        return next.run(request).await;
    }
    let listing = path.ends_with("/connections") && !path.starts_with("/api/condition/");
    let mut named = header_text(&request, "x-llm-connection");
    let mut has_key = header_text(&request, "x-llm-key").is_some();
    let mut ask = None;
    if matches!(path.as_str(), "/api/ask" | "/api/explore") && request.method() == axum::http::Method::POST {
        let (parts, body) = request.into_parts();
        let bytes = match to_bytes(body, 64 * 1024).await {
            Ok(b) => b,
            Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"),
        };
        let body = serde_json::from_slice::<Value>(&bytes).ok().filter(Value::is_object);
        // Explore plans are already validated by the read-only search handler. An
        // edited plan must not depend on a saved model being connected. Rebuild
        // the original bytes unchanged: explore has a closed request schema and
        // must never receive the ask-only connection/model fields below.
        let deterministic =
            path == "/api/explore" && body.as_ref().and_then(|b| b.get("plan")).is_some_and(|v| !v.is_null());
        if path == "/api/ask" {
            ask = body;
        }
        if let Some(b) = &ask {
            named = text(b.get("connection")).or(named);
            has_key |= text(b.get("key")).is_some();
        }
        request = Request::from_parts(parts, Body::from(bytes));
        if deterministic {
            return next.run(request).await;
        }
    }
    let personal = named.as_ref().is_some_and(|n| n.starts_with("connector:"));
    if ask.as_ref().and_then(|b| b.get("no_llm")).and_then(Value::as_bool) == Some(true) {
        return next.run(request).await;
    }
    if presented_token(request.headers(), scope.gateway.secure_cookies()).is_none() {
        return if personal {
            error(StatusCode::UNAUTHORIZED, "sign_in_required")
        } else {
            next.run(request).await
        };
    }
    let user = match scope.gateway.user(request.headers()) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let hosted = scope
        .state
        .llm
        .llm
        .as_ref()
        .map(|l| l.as_ref())
        .cloned()
        .unwrap_or_else(|| Llm::new(Registry::default(), Cache::new("unused", CacheMode::Off)));
    let mut llm = match scope.gateway.llm_for(request.headers(), &hosted) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let choice = if named.is_none() && !has_key && !listing {
        match scope.gateway.choice(&user) {
            Ok(c) => c,
            Err(e) => return e,
        }
    } else {
        None
    };
    if let Some(choice) = &choice {
        if let Err(e) = scope.gateway.validate_choice(&llm, choice) {
            // Preserve lexical choices when a saved device is unavailable. The explicit
            // connection header still prevents any hosted model fallback.
            if !understand || e.status() != StatusCode::SERVICE_UNAVAILABLE {
                return e;
            }
        }
        named = Some(choice.connection.clone());
        if let Some(model) = &choice.model
            && let Ok(connection) = llm.registry().get(&choice.connection)
        {
            let mut registry = llm.registry().clone();
            let mut c = connection.clone();
            c.config.default_model = Some(model.clone());
            registry.insert(c);
            llm = llm.with_registry(registry);
        }
    }
    if let Some(name) = &named {
        if name.starts_with("connector:") {
            if has_key {
                return error(StatusCode::BAD_REQUEST, "keys_stay_on_device");
            }
            let choice = ModelChoice {
                connection: name.clone(),
                model: ask.as_ref().and_then(|b| text(b.get("model"))),
            };
            if let Err(e) = scope.gateway.validate_choice(&llm, &choice)
                && (!understand || e.status() != StatusCode::SERVICE_UNAVAILABLE)
            {
                return e;
            }
        }
        let value = match name.parse() {
            Ok(v) => v,
            Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_connection"),
        };
        request.headers_mut().insert("x-llm-connection", value);
        if let Some(b) = &mut ask {
            b["connection"] = json!(name);
            if let Some(model) = choice.as_ref().and_then(|c| c.model.as_ref())
                && text(b.get("model")).is_none()
            {
                b["model"] = json!(model);
            }
            let (mut parts, _) = request.into_parts();
            // The body changed; do not retain its old length.
            parts.headers.remove(header::CONTENT_LENGTH);
            request = Request::from_parts(parts, Body::from(serde_json::to_vec(b).unwrap()));
        }
    }
    let mut state = scope.state.clone();
    state.llm.llm = Some(Arc::new(llm));
    let mut response = model_routes(&state, scope.conversations.clone())
        .oneshot(request)
        .await
        .unwrap();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
#[cfg(test)]
#[path = "connector_tests.rs"]
mod tests;
