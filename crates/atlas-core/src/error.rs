use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("snapshot {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("snapshot {path}: {source}")]
    Codec { path: PathBuf, source: bincode::Error },
    #[error("snapshot {path}: format {found}, expected {expected} (rebuild it)")]
    Format { path: PathBuf, found: u32, expected: u32 },
}
