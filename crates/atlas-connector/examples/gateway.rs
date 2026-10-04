//! Local verification gateway, separate from the production atlas-server mount.
//! No shared graph store: reads the already running filtered API.
use atlas_accounts::AccountsConfig;
use atlas_connector::{
    graph::HttpGraph,
    server::{Gateway, router},
    store::Store,
};
use clap::Parser;
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8021")]
    listen: String,
    #[arg(long, default_value = "http://127.0.0.1:8010")]
    graph: String,
    #[arg(long, default_value = "data/cache/connector/devices.sqlite")]
    database: PathBuf,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let address: std::net::SocketAddr = args.listen.parse()?;
    anyhow::ensure!(address.ip().is_loopback(), "verification gateway must bind loopback");
    if let Some(parent) = args.database.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let accounts = AccountsConfig::from_env();
    let gateway = Gateway::from_config(
        Arc::new(Store::open(&args.database)?),
        &accounts,
        Arc::new(HttpGraph::new(&args.graph)?),
    )?;
    let router = router(gateway).merge(atlas_accounts::router(accounts));
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!("Verification gateway on {address}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
