use std::path::PathBuf;

use atlas_core::CoreError;

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {source}")]
    Csv { path: PathBuf, source: csv::Error },
    #[error("{path}: {source}")]
    Xml { path: PathBuf, source: quick_xml::Error },
    #[error("{path}: missing column {column}")]
    MissingColumn { path: PathBuf, column: &'static str },
    #[error("{path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
    #[error("{path}: schema {found}, expected {expected}")]
    Schema {
        path: PathBuf,
        found: String,
        expected: String,
    },
    /// A withholding list (suppression/quarantine) is unreadable or invalid (fail-closed).
    #[error("withholding list: {0}")]
    Withhold(String),
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl IngestError {
    pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}
