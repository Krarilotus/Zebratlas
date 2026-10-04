//! Per-device queues, one bounded call, no replay after disconnect/cancellation.
use crate::{
    now,
    protocol::{Capability, Frame},
    random_token,
    store::Store,
};
use async_trait::async_trait;
use atlas_llm::{CompletionRequest, LlmError, Result};
use atlas_llm::{
    connector::{ConnectorProvider, ConnectorTransport},
    registry::Connection,
    request::ProviderOutput,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};

type Reply = oneshot::Sender<Result<ProviderOutput>>;
pub struct Session {
    pub generation: String,
    pub user: String,
    pub capabilities: Vec<Capability>,
    pub tx: mpsc::Sender<Frame>,
    pub closed: watch::Sender<bool>,
    gate: Arc<Semaphore>,
    pending: Mutex<HashMap<String, Reply>>,
}
pub struct Broker {
    pub store: Arc<Store>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}
impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectorBroker")
    }
}
impl Broker {
    pub fn new(store: Arc<Store>) -> Arc<Self> {
        Arc::new(Self {
            store,
            sessions: Mutex::new(HashMap::new()),
        })
    }
    pub fn attach(
        &self,
        id: &str,
        user: String,
        capabilities: Vec<Capability>,
        tx: mpsc::Sender<Frame>,
    ) -> anyhow::Result<Arc<Session>> {
        anyhow::ensure!(self.store.active(id, &user)?, "device revoked");
        anyhow::ensure!(capabilities.len() <= 16, "too many connections");
        let mut seen = std::collections::HashSet::new();
        for c in &capabilities {
            anyhow::ensure!(
                !c.connection.is_empty()
                    && c.connection.len() <= 80
                    && seen.insert(&c.connection)
                    && c.models.len() <= 64
                    && c.models.iter().all(|m| m.len() <= 160)
                    && c.default_model.as_ref().is_none_or(|m| m.len() <= 160)
                    && c.kind != atlas_llm::ProviderKind::Connector,
                "invalid capability"
            );
        }
        let mut sessions = self.sessions.lock().unwrap();
        anyhow::ensure!(!sessions.contains_key(id), "device already connected");
        let (closed, _) = watch::channel(false);
        let session = Arc::new(Session {
            generation: random_token()?,
            user,
            capabilities,
            tx,
            closed,
            gate: Arc::new(Semaphore::new(1)),
            pending: Mutex::new(HashMap::new()),
        });
        sessions.insert(id.into(), session.clone());
        Ok(session)
    }
    pub fn detach(&self, id: &str, generation: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.get(id).is_some_and(|s| s.generation == generation)
            && let Some(s) = sessions.remove(id)
        {
            s.closed.send_replace(true);
            s.pending.lock().unwrap().clear(); // Receivers fail with unavailable.
        }
    }
    pub fn revoke(&self, id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().remove(id) {
            s.closed.send_replace(true);
            s.pending.lock().unwrap().clear();
            // Reader wakes on channel close; socket owner also checks active on heartbeat.
        }
    }
    pub fn settle(&self, id: &str, generation: &str, request: &str, output: Result<ProviderOutput>) {
        let reply = self
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .filter(|s| s.generation == generation)
            .and_then(|s| s.pending.lock().unwrap().remove(request));
        if let Some(reply) = reply {
            let _ = reply.send(output);
        }
    }
    pub fn connections(self: &Arc<Self>, user: &str) -> Vec<Connection> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, s)| s.user == user && self.store.active(id, user).unwrap_or(false))
            .flat_map(|(id, s)| {
                s.capabilities.iter().map(move |c| {
                    let mut connection = ConnectorProvider::connection(
                        format!("connector:{id}:{}", c.connection),
                        c.default_model.clone(),
                        Arc::new(Remote {
                            broker: self.clone(),
                            device: id.clone(),
                            user: user.into(),
                            connection: c.connection.clone(),
                        }),
                    );
                    connection.config.models = c.models.clone();
                    connection
                })
            })
            .collect()
    }
    fn session(&self, id: &str, user: &str) -> Result<Arc<Session>> {
        if !self.store.active(id, user).unwrap_or(false) {
            return Err(LlmError::Unavailable("connector revoked".into()));
        }
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .filter(|s| s.user == user)
            .cloned()
            .ok_or_else(|| LlmError::Unavailable("connector offline".into()))
    }
}
#[derive(Debug)]
struct Remote {
    broker: Arc<Broker>,
    device: String,
    user: String,
    connection: String,
}

// Drop guards remove pending requests on timeout AND when the caller drops its future.
struct Pending {
    session: Arc<Session>,
    id: String,
}
impl Drop for Pending {
    fn drop(&mut self) {
        let removed = self.session.pending.lock().unwrap().remove(&self.id).is_some();
        if removed && self.session.tx.try_send(Frame::Cancel { id: self.id.clone() }).is_err() {
            // A full queue must not silently lose cancellation. Closing the session
            // causes the connector to abort local work instead.
            self.session.closed.send_replace(true);
        }
    }
}
#[async_trait]
impl ConnectorTransport for Remote {
    async fn available(&self) -> bool {
        self.broker.session(&self.device, &self.user).is_ok()
    }
    async fn dispatch(&self, request: CompletionRequest) -> Result<ProviderOutput> {
        if request.deadline.is_zero() || request.deadline > Duration::from_secs(crate::protocol::MAX_DEADLINE_SECS) {
            return Err(LlmError::InvalidRequest(
                "connector deadline must be 1..120000 ms".into(),
            ));
        }
        if serde_json::to_vec(&request)?.len() > 64 * 1024 {
            return Err(LlmError::InvalidRequest("connector prompt too large".into()));
        }
        let session = self.broker.session(&self.device, &self.user)?;
        let _permit = session
            .gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| LlmError::RateLimited("connector busy".into()))?;
        let id = random_token().map_err(|_| LlmError::Unavailable("randomness unavailable".into()))?;
        let (tx, rx) = oneshot::channel();
        session.pending.lock().unwrap().insert(id.clone(), tx);
        let _pending = Pending {
            session: session.clone(),
            id: id.clone(),
        };
        let deadline = request.deadline;
        session
            .tx
            .try_send(Frame::Complete {
                id,
                connection: self.connection.clone(),
                expires_at: now() + deadline.as_secs() as i64 + 1,
                request,
            })
            .map_err(|_| LlmError::Unavailable("connector outbound queue unavailable".into()))?;
        tokio::time::timeout(deadline, rx)
            .await
            .map_err(|_| LlmError::Timeout(deadline))?
            .map_err(|_| LlmError::Unavailable("connector disconnected; outcome unknown, not replayed".into()))?
    }
}
