//! Verification (D15): re-read a source record from the cache and re-hash it, or re-hash a whole
//! source file, and compare with the checksum stored at build time.

use std::path::{Path, PathBuf};

use atlas_core::Graph;
use atlas_core::graph::{RecIdx, RecordHash, hex};
use atlas_core::provenance::SourceEntity;
use serde::Serialize;

use super::cache;
use crate::sources;

/// Records checked per node or edge (the rest are listed as not checked).
pub const MAX_RECORDS: usize = 10;

#[derive(Clone, Debug, Serialize)]
pub struct RecordVerification {
    pub record_id: String,
    pub source: String,
    pub file: String,
    pub locator: String,
    pub hash_form: RecordHash,
    /// Upstream page anyone can open to check the record live.
    pub url: Option<String>,
    pub fetched_at: Option<String>,
    pub stored_sha256: String,
    pub computed_sha256: Option<String>,
    pub id_at_locator: Option<String>,
    pub matches: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileVerification {
    pub source: String,
    pub file: String,
    pub url: String,
    pub stored_sha256: Option<String>,
    pub computed_sha256: Option<String>,
    pub matches: bool,
    pub error: Option<String>,
}

/// Absolute path of an entity file: graph entities are data-relative (`cache/…`, `raw/…`), atlas
/// entities name a file in `raw/`.
pub fn entity_path(data: &Path, file: &str) -> PathBuf {
    if file.starts_with("cache/") || file.starts_with("raw/") {
        data.join(file)
    } else {
        data.join("raw").join(file)
    }
}

pub fn record(data: &Path, graph: &Graph, r: RecIdx) -> RecordVerification {
    let rec = graph.record(r);
    let entity = graph.provenance().entity(rec.entity);
    let check = cache::rehash(&entity_path(data, &entity.file), &rec.locator, rec.hash);
    let stored = hex(&rec.sha256);
    let matches = check.computed_sha256.as_deref() == Some(stored.as_str())
        && check.found_id.as_deref().is_none_or(|id| id == rec.id);
    RecordVerification {
        record_id: rec.id.clone(),
        source: entity.id.clone(),
        file: entity.file.clone(),
        locator: rec.locator.to_string(),
        hash_form: rec.hash,
        url: rec.url.clone(),
        fetched_at: rec.fetched_at.clone(),
        stored_sha256: stored,
        computed_sha256: check.computed_sha256,
        id_at_locator: check.found_id,
        matches,
        error: check.error,
    }
}

/// Re-hash a raw source file (byte checksum, as recorded by the atlas build).
pub fn file(data: &Path, entity: &SourceEntity) -> FileVerification {
    let path = entity_path(data, &entity.file);
    let (computed, error) = match sources::sha256(&path) {
        Ok(h) => (Some(h), None),
        Err(e) => (None, Some(e.to_string())),
    };
    FileVerification {
        source: entity.id.clone(),
        file: entity.file.clone(),
        url: entity.url.clone(),
        matches: computed.is_some() && computed == entity.sha256,
        stored_sha256: entity.sha256.clone(),
        computed_sha256: computed,
        error,
    }
}
