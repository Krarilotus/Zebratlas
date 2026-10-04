//! Ask the atlas (D19): the user's own model converses, the atlas supplies every fact.
//!
//! - [`intent`]: ten typed query intents with JSON schemas, and [`Chip`]s (intent + slots), the
//!   editable form of a parsed question.
//! - [`exec`]: a deterministic, read-only executor per intent over atlas-core's [`Atlas`] and
//!   [`Graph`]; every fact carries the edge or node id it comes from.
//! - [`converse`]: the loop. Model → intent calls → executors → model → cited answer →
//!   atlas-llm validator → one regeneration → template fallback. Provider-agnostic (JSON tool
//!   protocol over atlas-llm), cached and PROV-O-recorded per call.
//! - [`routes`]: `POST /api/ask` (JSON or SSE), `GET /api/ask/intents`, `GET /api/ask/connections`.
//! - [`store`]: saves conversations for signed-in users through atlas-accounts.
//!
//! Wiring in atlas-server (owned by the graph agent):
//!
//! ```ignore
//! let ask = atlas_ask::AskState::new(atlas.clone(), graph.clone(), Arc::new(atlas_llm::Llm::from_env()?))
//!     .with_accounts_from_env();
//! let app = Router::new() /* existing routes */ .merge(atlas_ask::router(ask)) /* then .layer(cors) */;
//! ```
//!
//! Adapters that should move into library crates (today they mirror atlas-server code, see
//! [`exec::view`]): causal genes and gene-edge ids, the connection collector, the
//! shared-symptom neighbours and the graph path search.

pub mod converse;
pub mod copy;
pub mod exec;
pub mod facts;
pub mod intent;
pub mod query;
pub mod routes;
pub mod rules;
pub mod store;

use std::sync::Arc;

use atlas_core::{Atlas, Graph};
use atlas_llm::Llm;

pub use converse::{AskRequest, AskResponse, Asker, Event, Turn};
pub use facts::AskFact;
pub use intent::{Chip, ChipOrigin, ChipStatus, INTENTS, IntentKind, IntentSpec};
pub use store::{AccountsStore, ConversationStore};

/// State of the ask router.
#[derive(Clone)]
pub struct AskState {
    pub asker: Asker,
    pub store: Option<Arc<dyn ConversationStore>>,
    pub query_engine: Option<Arc<query::QueryEngine>>,
}

impl AskState {
    pub fn new(atlas: Arc<Atlas>, graph: Arc<Graph>, llm: Arc<Llm>) -> Self {
        let query_engine = match query::QueryEngine::from_env(graph.clone()) {
            Ok(engine) => engine.map(Arc::new),
            Err(error) => {
                eprintln!("atlas-ask: query routes disabled: {error}");
                None
            }
        };
        Self {
            asker: Asker::new(atlas, graph, llm),
            store: None,
            query_engine,
        }
    }

    pub fn with_store(mut self, store: Arc<dyn ConversationStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Enable schema, edited-plan and MCP routes after generating a card from the loaded release.
    pub fn with_query_engine(mut self, mut engine: query::QueryEngine) -> Self {
        engine.graph = Some(self.asker.graph.clone());
        self.query_engine = Some(Arc::new(engine));
        self
    }

    /// Save conversations in the accounts database (same settings as `/api/account`). If it can't
    /// be opened, asking still works; nothing is saved.
    pub fn with_accounts_from_env(self) -> Self {
        match AccountsStore::from_env() {
            Ok(s) => self.with_store(Arc::new(s)),
            Err(e) => {
                eprintln!("atlas-ask: conversations will not be saved: {e}");
                self
            }
        }
    }
}

/// `/api/ask`, `/api/ask/intents`, `/api/ask/connections`.
pub fn router(state: AskState) -> axum::Router {
    let query = state.query_engine.clone().map(|engine| {
        query::routes::router(query::routes::QueryState {
            engine,
            asker: state.asker.clone(),
        })
    });
    let app = routes::router(state);
    match query {
        Some(q) => app.merge(q),
        None => app,
    }
}
