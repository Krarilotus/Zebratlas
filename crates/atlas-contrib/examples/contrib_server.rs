//! Standalone contribution API without the graph (identity checks then report "not found"), for UI
//! work and demos. The atlas server mounts the same router with the real graph lookup.
//!
//! ```sh
//! RARE_ATLAS_REVIEW_TOKENS=demo:<a key of 16+ characters> cargo run -p atlas-contrib --example contrib_server -- 127.0.0.1:8001
//! ```

use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8001".into());
    let config = atlas_contrib::ContribConfig::from_env();
    eprintln!("database: {:?}; overlay: {:?}", config.database, config.overlay_path);
    let state = atlas_contrib::Contrib::new(config, Arc::new(atlas_contrib::NoGraph))?.into_state();
    let app = atlas_contrib::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("serving http://{addr}/api/contribute");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        tokio::signal::ctrl_c().await.ok();
    })
    .await?;
    Ok(())
}
