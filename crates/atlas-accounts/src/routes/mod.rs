//! `/api/account/*`: router, shared state and extractors. Handlers live in [`identity`] and [`content`].
//!
//! Auth: a session token in the `__Host-atlas_session` cookie (plain HTTP: `atlas_session`) (`HttpOnly`, `SameSite=Lax`, `Secure` unless
//! configured off) or in `Authorization: Bearer <token>` for API clients. The token never appears in a
//! response body, so browser scripts can't read it. Mutating routes take JSON bodies, which a cross-site
//! HTML form can't send. The shared CSRF guard requires `X-Atlas-CSRF: 1` for cookie mutations.

mod content;
mod data;
mod email;
mod identity;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::auth::{Hasher, RateLimiter, token_hash};
use crate::config::AccountsConfig;
use crate::error::{AccountsError, Result};
use crate::store::{SessionInfo, Store};
use crate::util::now;

pub const SESSION_COOKIE: &str = "__Host-atlas_session";
pub const INSECURE_SESSION_COOKIE: &str = "atlas_session";
const BODY_LIMIT: usize = 512 * 1024;

#[derive(Clone)]
pub(crate) struct AppState(Arc<Inner>);

pub(crate) struct Inner {
    pub store: Store,
    pub hasher: Hasher,
    pub config: AccountsConfig,
    pub login_per_account: RateLimiter,
    pub login_per_client: RateLimiter,
    pub signup_per_client: RateLimiter,
    pub email_per_client: RateLimiter,
    /// Server key for saved documents (`ATLAS_DOC_KEK`); `None` = saving is off.
    pub kek: Option<crate::documents::Kek>,
    /// atlas-contrib's export / anonymise hooks, wired by the host.
    pub hooks: crate::documents::Hooks,
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// The accounts API. Never fails: if the database can't be opened, the error is printed once and every
/// `/api/account/*` route answers 503, so the free atlas keeps working (H1).
pub fn router(config: AccountsConfig) -> Router {
    match try_router(config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("atlas-accounts: accounts disabled: {e}");
            unavailable_router()
        }
    }
}

/// The accounts API, or the error that prevents it (database can't be opened or migrated).
pub fn try_router(config: AccountsConfig) -> Result<Router> {
    try_router_with(config, crate::documents::AccountsExtras::from_env(None))
}

/// [`router`] with atlas-contrib's account hooks (export, anonymise on deletion; D42).
pub fn router_with(config: AccountsConfig, extras: crate::documents::AccountsExtras) -> Router {
    match try_router_with(config, extras) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("atlas-accounts: accounts disabled: {e}");
            unavailable_router()
        }
    }
}

pub fn try_router_with(config: AccountsConfig, extras: crate::documents::AccountsExtras) -> Result<Router> {
    Ok(router_from_state(build_state(config, extras)?))
}

fn build_state(config: AccountsConfig, extras: crate::documents::AccountsExtras) -> Result<AppState> {
    let store = Store::open(&config.database)?;
    let state = AppState(Arc::new(Inner {
        store,
        hasher: Hasher::new(config.password_cost)?,
        login_per_account: RateLimiter::new(config.login_per_account),
        login_per_client: RateLimiter::new(config.login_per_client),
        signup_per_client: RateLimiter::new(config.signup_per_client),
        email_per_client: RateLimiter::new(config.email_per_client),
        kek: extras.kek,
        hooks: extras.hooks,
        config,
    }));
    Ok(state)
}

fn router_from_state(state: AppState) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/signup", post(identity::signup))
        .route("/login", post(identity::login))
        .route("/verify-email", post(email::verify))
        .route("/resend-verification", post(email::resend))
        .route("/forgot-password", post(email::forgot))
        .route("/reset-password", post(email::reset))
        .route("/logout", post(identity::logout))
        .route("/logout-all", post(identity::logout_all))
        .route(
            "/me",
            get(identity::me).patch(identity::update_me).delete(identity::delete_me),
        )
        .route("/password", post(identity::change_password))
        .route("/export", get(identity::export))
        .route("/data", get(data::view))
        .route("/documents", get(data::list_documents))
        .route("/documents/{id}", get(data::get_document).delete(data::delete_document))
        .route("/orgs", get(identity::list_orgs).post(identity::create_org))
        .route("/saved", get(content::list_saved).post(content::create_saved))
        .route(
            "/saved/{id}",
            get(content::get_saved)
                .patch(content::update_saved)
                .delete(content::delete_saved),
        )
        .route(
            "/conversations",
            get(content::list_conversations).post(content::create_conversation),
        )
        .route(
            "/conversations/{id}",
            get(content::get_conversation)
                .patch(content::rename_conversation)
                .delete(content::delete_conversation),
        )
        .route("/conversations/{id}/messages", post(content::append_messages))
        .fallback(|| async { AccountsError::NotFound })
        .with_state(state);
    Router::new().nest("/api/account", finish(api))
}

fn unavailable_router() -> Router {
    let api = Router::new()
        .route("/status", get(|| async { Json(json!({ "enabled": false })) }))
        .fallback(|| async { AccountsError::Unavailable });
    Router::new().nest("/api/account", finish(api))
}

/// Body limit and `Cache-Control: no-store` on every account response (personal data).
fn finish(r: Router) -> Router {
    r.layer(axum::middleware::from_fn(crate::csrf::guard))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::middleware::map_response(|mut res: Response| async move {
            res.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            res
        }))
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "enabled": true,
        "email_verification_required": state.config.require_email_verification,
        "email_delivery_available": state.config.email.is_some(),
        "sign_in_methods": ["password"],
        "plans": ["free", "organisation", "enterprise"],
    }))
}

// ---------------------------------------------------------------------------------------------
// Extractors
// ---------------------------------------------------------------------------------------------

/// The signed-in user's session; 401 without a valid one.
pub(crate) struct Session {
    pub info: SessionInfo,
}

impl FromRequestParts<AppState> for Session {
    type Rejection = AccountsError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let token = presented_token(&parts.headers, state.config.secure_cookies).ok_or(AccountsError::Unauthorized)?;
        let info = state
            .store
            .session(&token_hash(&token), now())?
            .ok_or(AccountsError::Unauthorized)?;
        Ok(Session { info })
    }
}

/// Rate-limit key for the caller: first `X-Forwarded-For` hop when trusted, else the socket address.
pub(crate) struct ClientKey(pub String);

impl FromRequestParts<AppState> for ClientKey {
    type Rejection = AccountsError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let forwarded = state
            .config
            .trust_forwarded_for
            .then(|| {
                parts
                    .headers
                    .get("x-forwarded-for")?
                    .to_str()
                    .ok()?
                    .split(',')
                    .next()
                    .map(str::trim)
                    .map(str::to_string)
            })
            .flatten()
            .filter(|s| !s.is_empty() && s.len() <= 64);
        let socket = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string());
        Ok(ClientKey(forwarded.or(socket).unwrap_or_else(|| "unknown".into())))
    }
}

/// JSON body whose rejection is a JSON error like every other response here.
pub(crate) struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = AccountsError;

    async fn from_request(req: Request, state: &S) -> Result<Self> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Body(v)),
            Err(rej) => Err(AccountsError::invalid(rej.body_text())),
        }
    }
}

/// The token from `Authorization: Bearer` or the session cookie, if it looks like one of ours.
pub fn presented_token(headers: &HeaderMap, secure: bool) -> Option<String> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let cookie = || {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|kv| kv.trim().split_once('='))
            .find(|(k, _)| {
                *k == if secure {
                    SESSION_COOKIE
                } else {
                    INSECURE_SESSION_COOKIE
                }
            })
            .map(|(_, v)| v.trim())
    };
    let token = bearer.or_else(cookie)?;
    let plausible = token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    plausible.then(|| token.to_string())
}

pub(crate) fn session_cookie(token: &str, max_age_secs: u64, secure: bool) -> HeaderValue {
    let name = if secure {
        SESSION_COOKIE
    } else {
        INSECURE_SESSION_COOKIE
    };
    let secure = if secure { "; Secure" } else { "" };
    let v = format!("{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}");
    HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub(crate) fn clear_cookie(secure: bool) -> HeaderValue {
    session_cookie("", 0, secure)
}

pub(crate) fn with_cookie(mut res: Response, cookie: HeaderValue) -> Response {
    res.headers_mut().append(header::SET_COOKIE, cookie);
    res
}

pub(crate) fn json_response<T: serde::Serialize>(status: axum::http::StatusCode, body: &T) -> Response {
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tokens_in_bearer_or_cookie_and_rejects_junk() {
        let tok = "a".repeat(43);
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("lang=de; {SESSION_COOKIE}={tok}")).unwrap(),
        );
        assert_eq!(presented_token(&h, true).as_deref(), Some(tok.as_str()));
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {tok}")).unwrap(),
        );
        assert_eq!(presented_token(&h, true).as_deref(), Some(tok.as_str()));
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("atlas_session=short"));
        assert!(presented_token(&h, true).is_none());
    }

    #[test]
    fn cookies_are_http_only_and_lax() {
        let c = session_cookie(&"a".repeat(43), 60, true);
        let c = c.to_str().unwrap();
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Lax") && c.ends_with("Secure"));
        assert!(!session_cookie("x", 60, false).to_str().unwrap().contains("Secure"));
    }
}
