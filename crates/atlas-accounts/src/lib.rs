//! Optional accounts for the rare-disease atlas (requirement H2).
//!
//! The atlas is free first: every journey works without an account. Signing in only adds a place to
//! keep things: conditions a person follows, connection cards and maps, message drafts, and their
//! conversations with the atlas (questions, answers and the citations behind them).
//!
//! The data model already anticipates the paid tier (H3) without building billing: organisations with
//! members and roles, a plan flag (`free` / `organisation` / `enterprise`), and an optional organisation
//! scope on saved items and conversations (future team workspaces).
//!
//! Layout:
//! - [`config`]: where the SQLite file lives, cookie and rate-limit settings.
//! - [`store`]: schema, migrations and every query (rusqlite, one connection behind a mutex).
//! - [`auth`]: argon2id password hashing, session tokens (only their SHA-256 is stored), rate limits.
//! - [`routes`]: the axum handlers under `/api/account/*`.
//!
//! Sign-in methods are rows in `credentials` (`kind` = `password` today; `passkey` and `magic_link`
//! are reserved), and sessions record the `auth_method` that created them, so passkeys (WebAuthn) or
//! magic links add a credential kind and two routes without touching sessions or saved data.
//!
//! Secrets are never logged: passwords and tokens live in [`auth::Secret`], whose `Debug` is redacted,
//! and errors never echo request input.

pub mod auth;
pub mod config;
pub mod copy;
pub mod csrf;
pub mod documents;
pub mod email;
pub mod error;
pub mod model;
mod routes;
pub mod store;
mod util;

pub use config::{AccountsConfig, Database, Limit, PasswordCost};
pub use documents::{
    AccountsExtras, AnonymiseReport, ContributionHooks, DocumentContent, DocumentMeta, DocumentVault, Kek,
    SavedDocument,
};
pub use error::AccountsError;
pub use routes::{
    INSECURE_SESSION_COOKIE, SESSION_COOKIE, presented_token, router, router_with, try_router, try_router_with,
};
pub use store::Store;
