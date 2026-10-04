//! Contributions to the rare-disease atlas (D19, requirement F3).
//!
//! Patient groups, researchers and their AI assistants can suggest a connection (patient group,
//! organisation, registry, study or person ↔ condition or gene), correct one, report missing
//! evidence, or flag an outdated contact. Every contribution is user-asserted and kept apart from
//! the curated graph until a reviewer accepts it:
//!
//! ```text
//! submitted ──auto-checks──▶ auto_checked ──review──▶ accepted | rejected (with a reason)
//! ```
//!
//! - [`model`]: submissions (cleaned, length-limited), check reports, reviews.
//! - [`checks`]: URL resolves, quote on the page, identity via the graph search, duplicate and
//!   conflict detection against the curated graph and other contributions.
//! - [`state`]: the state machine as a pure function.
//! - [`prov`]: a PROV-O activity for every state change (agent = contributor, software, reviewer).
//! - [`overlay`]: accepted contributions as `user_asserted` nodes, edges and annotations.
//! - [`store`]: SQLite (`data/app/contrib.sqlite`), optimistic versioning per change.
//! - [`routes`]: `/api/contribute*` and `/api/review*`; mount with [`router`].
//!
//! Wiring into the server:
//!
//! ```ignore
//! let contrib = atlas_contrib::Contrib::new(atlas_contrib::ContribConfig::from_env(), lookup)?
//!     .with_user_resolver(resolver)   // optional: signed-in contributors and reviewers
//!     .into_state();
//! let app = app.merge(atlas_contrib::router(contrib));
//! ```

pub mod auth;
pub mod checks;
pub mod config;
pub mod copy;
#[cfg(feature = "core")]
pub mod core_lookup;
pub mod datasource;
pub mod error;
pub mod fetch;
pub mod graph;
pub mod model;
pub mod overlay;
pub mod privacy;
pub mod prov;
mod routes;
pub mod schema;
mod service;
pub mod state;
pub mod store;
pub mod text;
mod util;

pub use auth::{UserRef, UserResolver};
pub use config::{ContribConfig, Limit, ReviewerToken};
#[cfg(feature = "core")]
pub use core_lookup::CoreLookup;
pub use error::ContribError;
pub use fetch::{FetchPolicy, Fetched, Fetcher, HttpFetcher};
pub use graph::{GraphEdgeRef, GraphLookup, MemoryGraph, NoGraph};
pub use model::{Contribution, ContributionKind, State, Submission};
pub use overlay::Overlay;
pub use routes::router;
pub use service::{Contrib, ContribState};
pub use store::{Database, ListFilter};

/// Software agent name in PROV records.
pub const AGENT: &str = concat!("atlas-contrib@", env!("CARGO_PKG_VERSION"));
