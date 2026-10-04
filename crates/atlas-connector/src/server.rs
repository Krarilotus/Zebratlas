//! Merge router() into atlas-server. Its account Store must use the same database
//! as atlas-accounts. GraphReader must serve the existing privacy-filtered graph API.
use crate::{
    broker::Broker,
    graph::GraphReader,
    now,
    protocol::{Failure, Frame, MAX_FRAME, VERSION},
    store::{ModelChoice, Poll, Store},
};
use atlas_accounts::{AccountsConfig, Store as Accounts, auth::token_hash, presented_token};
use atlas_llm::{Cache, CacheMode, Call, CompletionRequest, Llm, Registry};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::SinkExt;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Gateway(Arc<Inner>, Option<Arc<Llm>>);
struct Inner {
    pub broker: Arc<Broker>,
    accounts: Arc<Accounts>,
    secure: bool,
    graph: Arc<dyn GraphReader>,
    rate: Mutex<HashMap<String, (i64, u32)>>,
}
#[expect(
    clippy::result_large_err,
    reason = "HTTP helpers return complete responses for direct propagation by handlers."
)]
impl Gateway {
    pub fn new(store: Arc<Store>, accounts: Arc<Accounts>, secure: bool, graph: Arc<dyn GraphReader>) -> Self {
        Self(
            Arc::new(Inner {
                broker: Broker::new(store),
                accounts,
                secure,
                graph,
                rate: Mutex::new(HashMap::new()),
            }),
            None,
        )
    }
    pub fn from_config(
        store: Arc<Store>,
        config: &AccountsConfig,
        graph: Arc<dyn GraphReader>,
    ) -> anyhow::Result<Self> {
        Ok(Self::new(
            store,
            Arc::new(Accounts::open(&config.database)?),
            config.secure_cookies,
            graph,
        ))
    }
    pub fn broker(&self) -> &Arc<Broker> {
        &self.0.broker
    }
    pub fn secure_cookies(&self) -> bool {
        self.0.secure
    }
    pub fn with_hosted(mut self, hosted: Arc<Llm>) -> Self {
        self.1 = Some(hosted);
        self
    }
    /// A request-scoped LLM. Authenticate headers first; callers cannot nominate an account.
    /// Extend the hosted registry rather than changing the hosted default (D48).
    pub fn llm_for(&self, headers: &HeaderMap, hosted: &Llm) -> Result<Llm, Response> {
        let user = self.user(headers)?;
        let mut registry = hosted.registry().clone();
        for connection in self.0.broker.connections(&user) {
            registry.insert(connection);
        }
        Ok(hosted.with_registry(registry))
    }
    pub fn user(&self, headers: &HeaderMap) -> Result<String, Response> {
        let token = presented_token(headers, self.0.secure)
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "sign_in_required"))?;
        self.0
            .accounts
            .session(&token_hash(&token), now())
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "accounts_unavailable"))?
            .map(|s| s.user.id)
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "sign_in_required"))
    }
    fn valid_device(&self, id: &str, user: &str) -> bool {
        self.0.broker.store.active(id, user).unwrap_or(false) && self.0.accounts.user(user).ok().flatten().is_some()
    }
    fn allow(&self, key: &str, max: u32) -> bool {
        let mut rates = self.0.rate.lock().unwrap();
        let at = now();
        rates.retain(|_, (time, _)| at - *time < 60);
        if rates.len() >= 2000 && !rates.contains_key(key) {
            return false;
        }
        let item = rates.entry(key.into()).or_insert((at, 0));
        item.1 += 1;
        item.1 <= max
    }
    pub fn choice(&self, user: &str) -> Result<Option<ModelChoice>, Response> {
        self.0
            .broker
            .store
            .choice(user)
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "store_unavailable"))
    }
    /// Validate against the account-scoped registry, including the device's allowlist.
    pub fn validate_choice(&self, llm: &Llm, choice: &ModelChoice) -> Result<(), Response> {
        let c = llm
            .registry()
            .get(&choice.connection)
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "connection_unavailable"))?;
        if choice.model.as_ref().is_some_and(|m| {
            m.is_empty()
                || m.len() > 160
                || m.chars().any(char::is_control)
                || (c.config.default_model.as_ref() != Some(m) && !c.config.models.contains(m))
        }) {
            return Err(error(StatusCode::BAD_REQUEST, "model_not_advertised"));
        }
        Ok(())
    }
}
pub fn router(gateway: Gateway) -> Router {
    Router::new()
        .route("/api/connector/pair", post(pair))
        .route("/api/connector/poll", post(poll))
        .route("/api/account/connectors/approve", post(approve))
        .route("/api/account/connectors/models", get(models))
        .route(
            "/api/account/connectors/model",
            get(model).put(choose_model).delete(reset_model),
        )
        .route("/api/account/connectors", get(devices))
        .route("/api/account/connectors/{id}", axum::routing::delete(revoke))
        .route("/api/connector/connect", get(connect))
        .route("/api/connector/connections", get(connections))
        .route("/api/connector/complete", post(complete))
        .with_state(gateway)
        .layer(axum::middleware::from_fn(atlas_accounts::csrf::guard))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::map_response(|mut r: Response| async move {
            r.headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            r
        }))
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ModelListingQuery {
    connection: Option<String>,
}
async fn models(State(g): State<Gateway>, headers: HeaderMap, Query(query): Query<ModelListingQuery>) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let Some(hosted) = &g.1 else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "models_unavailable");
    };
    let llm = match g.llm_for(&headers, hosted) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if let Some(name) = query.connection {
        if name.is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
            return error(StatusCode::BAD_REQUEST, "invalid_connection");
        }
        if !g.allow(&format!("model-check:{user}"), 12) {
            return error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
        }
        let connection = match llm.registry().get(&name) {
            Ok(connection) => connection,
            Err(_) => return error(StatusCode::NOT_FOUND, "connection_unavailable"),
        };
        // A server's installed CLI/login is not the signed-in visitor's machine.
        let mut availability = if connection.kind.is_cli() {
            atlas_llm::Availability::not_ready("native-connector-required")
        } else {
            match tokio::time::timeout(Duration::from_secs(4), connection.provider.probe()).await {
                Ok(availability) => availability,
                Err(_) => atlas_llm::Availability::not_ready("probe-timeout"),
            }
        };
        let connected = availability.available;
        let mut models = connection.config.models.clone();
        if let Some(default) = &connection.config.default_model {
            models.push(default.clone());
        }
        models.sort();
        models.dedup();
        if !connected {
            models.clear();
        } else if connection.kind != atlas_llm::ProviderKind::Connector {
            models.retain(|model| availability.models.contains(model));
        }
        // Return only this account's allowlisted catalogue, never key metadata or
        // the provider's broader catalogue. Free-tier readiness cannot override a failed probe.
        availability.models.clear();
        return Json(json!({"connection":name,"connected":connected,"models":models,"availability":availability}))
            .into_response();
    }
    let choice = match g.choice(&user) {
        Ok(c) => c,
        Err(e) => return e,
    };
    Json(json!({"default":hosted.default_connection(false),"selected":choice,
        "connections":llm.list_connections(false).await}))
    .into_response()
}
async fn model(State(g): State<Gateway>, headers: HeaderMap) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match g.choice(&user) {
        Ok(c) => Json(json!({"selected":c})).into_response(),
        Err(e) => e,
    }
}
async fn choose_model(State(g): State<Gateway>, headers: HeaderMap, Json(choice): Json<ModelChoice>) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let Some(hosted) = &g.1 else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "models_unavailable");
    };
    let llm = match g.llm_for(&headers, hosted) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if let Err(e) = g.validate_choice(&llm, &choice) {
        return e;
    }
    match g.0.broker.store.set_choice(&user, &choice) {
        Ok(()) => Json(json!({"selected":choice})).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "store_unavailable"),
    }
}
async fn reset_model(State(g): State<Gateway>, headers: HeaderMap) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match g.0.broker.store.clear_choice(&user) {
        Ok(()) => Json(json!({"selected":null})).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "store_unavailable"),
    }
}
fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    label: String,
}
async fn pair(State(g): State<Gateway>, Json(b): Json<Label>) -> Response {
    // Bounded global acquisition limit, independent of untrusted forwarded headers.
    if !g.allow("pair-global", 30) {
        return error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    match g.0.broker.store.pair(&b.label, now()) {
        Ok(p) => Json(p).into_response(),
        Err(_) => error(StatusCode::BAD_REQUEST, "pairing_unavailable"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceCode {
    device_code: atlas_accounts::auth::Secret,
}
async fn poll(State(g): State<Gateway>, Json(b): Json<DeviceCode>) -> Response {
    if !g.allow("poll-global", 240) {
        return error(StatusCode::TOO_MANY_REQUESTS, "slow_down");
    }
    match g.0.broker.store.poll(b.device_code.expose(), now()) {
        Ok(Poll::Pending) => Json(json!({"status":"authorization_pending"})).into_response(),
        Ok(Poll::Ready { device_id }) => Json(json!({"status":"approved","device_id":device_id})).into_response(),
        Ok(Poll::Invalid) => error(StatusCode::BAD_REQUEST, "expired_or_consumed"),
        Err(_) => error(StatusCode::TOO_MANY_REQUESTS, "slow_down"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    user_code: String,
}
async fn approve(State(g): State<Gateway>, headers: HeaderMap, Json(b): Json<Approval>) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !g.allow(&format!("approve:{user}"), 10) {
        return error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    match g.0.broker.store.approve(&b.user_code, &user, now()) {
        Ok(true) => Json(json!({"approved":true})).into_response(),
        _ => error(StatusCode::BAD_REQUEST, "invalid_or_expired_code"),
    }
}
async fn devices(State(g): State<Gateway>, headers: HeaderMap) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match g.0.broker.store.list(&user) {
        Ok(list) => Json(list).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "store_unavailable"),
    }
}
async fn revoke(State(g): State<Gateway>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match g.0.broker.store.revoke(&id, &user, now()) {
        Ok(true) => {
            g.0.broker.revoke(&id);
            Json(json!({"revoked":true})).into_response()
        }
        _ => error(StatusCode::NOT_FOUND, "device_not_found"),
    }
}
async fn connections(State(g): State<Gateway>, headers: HeaderMap) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    Json(
        g.0.broker
            .connections(&user)
            .iter()
            .map(|c| c.info(None))
            .collect::<Vec<_>>(),
    )
    .into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    connection: String,
    request: CompletionRequest,
    #[serde(default)]
    inputs: Vec<String>,
}
async fn complete(State(g): State<Gateway>, headers: HeaderMap, Json(b): Json<Complete>) -> Response {
    let user = match g.user(&headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let mut r = Registry::default();
    for c in g.0.broker.connections(&user) {
        r.insert(c);
    }
    let llm = Llm::new(r, Cache::new("unused", CacheMode::Off));
    match llm
        .complete(&Call::new(b.connection).with_private().with_inputs(b.inputs), b.request)
        .await
    {
        Ok(c) => Json(c).into_response(),
        Err(_) => error(StatusCode::BAD_GATEWAY, "connector_call_failed_not_replayed"),
    }
}
async fn connect(State(g): State<Gateway>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    // Native connector supplies a bearer header; browser origins are never accepted.
    if headers.contains_key(header::ORIGIN) {
        return error(StatusCode::FORBIDDEN, "native_connector_required");
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(d) = token.and_then(|t| g.0.broker.store.authenticate(t).ok().flatten()) else {
        return error(StatusCode::UNAUTHORIZED, "device_required");
    };
    if !g.valid_device(&d.id, &d.user_id) {
        return error(StatusCode::UNAUTHORIZED, "device_revoked");
    }
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| session(g, d, socket))
        .into_response()
}
async fn session(g: Gateway, d: crate::store::Device, mut socket: WebSocket) {
    let hello = tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(text)))) = hello else {
        return;
    };
    let Ok(Frame::Hello {
        version: VERSION,
        capabilities,
    }) = serde_json::from_str(&text)
    else {
        return;
    };
    let (tx, mut rx) = mpsc::channel(8);
    let Ok(s) = g.0.broker.attach(&d.id, d.user_id.clone(), capabilities, tx) else {
        return;
    };
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut last_seen = tokio::time::Instant::now();
    let mut graph_job: Option<tokio::task::JoinHandle<()>> = None;
    let mut closed = s.closed.subscribe();
    loop {
        if *closed.borrow() {
            break;
        }
        tokio::select! {
            _=closed.changed()=>break,
            _=heartbeat.tick()=>{
                if !g.valid_device(&d.id,&d.user_id) || last_seen.elapsed()>Duration::from_secs(45) {break;}
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {break;}
            }
            Some(frame)=rx.recv()=>{
                if !g.valid_device(&d.id,&d.user_id) {break;}
                let Ok(text)=serde_json::to_string(&frame) else {break;};
                if text.len()>MAX_FRAME || socket.send(Message::Text(text.into())).await.is_err() {break;}
            }
            incoming=socket.recv()=>{
                last_seen=tokio::time::Instant::now();
                if !g.valid_device(&d.id,&d.user_id) {break;}
                match incoming {
                    Some(Ok(Message::Pong(_)))=>{},
                    Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break;}},
                    Some(Ok(Message::Text(text)))=>{
                        match serde_json::from_str::<Frame>(&text) {
                            Ok(Frame::Completed {id,output})=>g.0.broker.settle(&d.id,&s.generation,&id,Ok(output)),
                            Ok(Frame::Failed {id,..})=>g.0.broker.settle(&d.id,&s.generation,&id,Err(atlas_llm::LlmError::Unavailable("local provider failed; not replayed".into()))),
                            Ok(Frame::Graph {id,query})=>{
                                if graph_job.as_ref().is_some_and(|j|!j.is_finished()) {
                                    let _=s.tx.try_send(Frame::Failed {id,code:Failure::Busy}); continue;
                                }
                                let tx=s.tx.clone(); let graph=g.0.graph.clone();
                                graph_job=Some(tokio::spawn(async move {
                                    let frame=match tokio::time::timeout(Duration::from_secs(20),graph.read(query)).await {
                                        Ok(Ok(result))=>Frame::GraphResult {id,result},
                                        _=>Frame::Failed {id,code:Failure::Unavailable}
                                    };
                                    let _=tx.try_send(frame);
                                }));
                            },
                            _=>break
                        }
                    }
                    _=>break
                }
            }
        }
    }
    if let Some(j) = graph_job {
        j.abort();
    }
    g.0.broker.detach(&d.id, &s.generation);
    let _ = socket.close().await;
}
