//! Pairing, outbound transport and read-only MCP in the Rust core.
pub mod broker;
pub mod client;
pub mod graph;
pub mod mcp;
pub mod protocol;
pub mod server;
pub mod store;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn random_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("secure randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn hash(value: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value))
}
