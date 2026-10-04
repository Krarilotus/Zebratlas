//! Host adapter for D42: one contribution service for routes, private export and deletion.
use crate::routes::AppState;
use atlas_accounts::{AccountsConfig, AccountsExtras, AnonymiseReport, ContributionHooks, DocumentVault};
use atlas_contrib::{Contrib, ContribConfig, UserRef};
use axum::{
    Router,
    extract::Request,
    http::Method,
    middleware::{self, Next},
};
use serde_json::Value;
use std::sync::Arc;

struct Core {
    atlas: Arc<atlas_core::Atlas>,
    graph: Arc<atlas_core::Graph>,
}
fn atlas(c: &Core) -> &atlas_core::Atlas {
    &c.atlas
}
fn graph(c: &Core) -> Option<&atlas_core::Graph> {
    Some(&c.graph)
}

pub struct Hooks(pub Arc<Contrib>);
impl ContributionHooks for Hooks {
    fn export_for_user(&self, user: &str) -> Result<Value, String> {
        self.0.export_for_user(user).map_err(|e| e.to_string())
    }
    fn anonymise_for_user(&self, user: &str, keep_credit: bool) -> Result<AnonymiseReport, String> {
        let snapshot = self.export_for_user(user)?;
        let accepted = snapshot["contributions"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter(|c| c["contribution"]["state"] == "accepted")
                    .count()
            })
            .unwrap_or(0);
        let removed = self
            .0
            .anonymise_for_user(user, keep_credit)
            .map_err(|e| e.to_string())?;
        Ok(AnonymiseReport {
            accepted_kept: accepted as u64,
            personal_fields_removed: removed as u64,
        })
    }
}

struct UnavailableHooks;
impl ContributionHooks for UnavailableHooks {
    fn export_for_user(&self, _: &str) -> Result<Value, String> {
        Err("contributions unavailable".into())
    }
    fn anonymise_for_user(&self, _: &str, _: bool) -> Result<AnonymiseReport, String> {
        Err("contributions unavailable".into())
    }
}

pub fn mounted(s: &AppState) -> Router {
    let cfg = AccountsConfig::from_env();
    let extras = AccountsExtras::from_env(None);
    let lookup = Arc::new(atlas_contrib::CoreLookup::new(
        Arc::new(Core {
            atlas: s.atlas.clone(),
            graph: s.graph.clone(),
        }),
        atlas,
        graph,
    ));
    mount_services(cfg, extras, Contrib::new(ContribConfig::from_env(), lookup)).0
}

fn mount_services(
    cfg: AccountsConfig,
    extras: AccountsExtras,
    c: atlas_contrib::error::Result<Contrib>,
) -> (Router, Option<Arc<Contrib>>) {
    let vault = DocumentVault::open_with(&cfg, extras.kek.clone()).ok().map(Arc::new);
    match c {
        Ok(mut c) => {
            if let Some(vault) = vault {
                c = c.with_user_resolver(Arc::new(move |headers| {
                    vault.user(headers).ok().flatten().map(|u| UserRef {
                        id: u.id,
                        name: u.display_name,
                        email: Some(u.email),
                    })
                }));
            }
            let shared = c.into_state();
            let hooks = Arc::new(Hooks(shared.clone()));
            (
                atlas_accounts::router_with(
                    cfg,
                    AccountsExtras {
                        hooks: Some(hooks),
                        ..extras
                    },
                )
                .merge(atlas_contrib::router(shared.clone())),
                Some(shared),
            )
        }
        // Losing the contribution database must not turn account deletion into a partial erase.
        Err(e) => {
            eprintln!("contributions unavailable: {e}");
            (
                atlas_accounts::router_with(
                    cfg,
                    AccountsExtras {
                        hooks: Some(Arc::new(UnavailableHooks)),
                        ..extras
                    },
                ),
                None,
            )
        }
    }
}

#[cfg(test)]
#[path = "account_wiring_tests.rs"]
mod tests;

/// Deletion gets an exclusive gate before any auth extractor runs. Existing submissions and saves
/// finish first; queued requests re-authenticate after deletion invalidates every session. Reads stay
/// available. The contribution service rewrites its overlay/export files in the deletion hook; no
/// contribution overlay is loaded into this server's immutable graph or integrity cache.
pub fn deletion_gate(router: Router) -> Router {
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    router.layer(middleware::from_fn(move |request: Request, next: Next| {
        let gate = gate.clone();
        async move {
            let path = request.uri().path();
            let private = path.starts_with("/api/account/") || path.starts_with("/api/intake");
            let response = if request.method() == Method::DELETE && path == "/api/account/me" {
                let _guard = gate.write().await;
                next.run(request).await
            } else if request.method() != Method::GET
                && (path.starts_with("/api/account/")
                    || path.starts_with("/api/contribute")
                    || path.starts_with("/api/review")
                    || path == "/api/intake")
            {
                let _guard = gate.read().await;
                next.run(request).await
            } else {
                next.run(request).await
            };
            let mut response = response;
            if private {
                response.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-store"),
                );
            }
            response
        }
    }))
}
