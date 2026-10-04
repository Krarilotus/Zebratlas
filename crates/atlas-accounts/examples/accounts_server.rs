//! Serves only `/api/account/*`, for working on the account screens without the full atlas server.
//!
//!   cargo run -j 2 -p atlas-accounts --example accounts_server -- 127.0.0.1:8787
//!
//! Uses `AccountsConfig::from_env()` (RARE_ATLAS_ACCOUNTS_DB, RARE_ATLAS_DATA, RARE_ATLAS_INSECURE_COOKIES).

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8787".into());
    let app = atlas_accounts::router(atlas_accounts::AccountsConfig::from_env());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("accounts API on http://{addr}/api/account");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await
}
