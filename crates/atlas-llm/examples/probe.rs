//! Print every connection with its probe result (no model calls):
//! `cargo run -p atlas-llm --example probe`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env"));
    let llm = atlas_llm::Llm::from_env()?;
    let list = llm.list_connections(true).await;
    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
}
