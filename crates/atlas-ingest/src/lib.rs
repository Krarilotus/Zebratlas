//! Parsers for the source files and the graph build; caches the result as `data/cache/atlas.snapshot`.

pub mod build;
pub mod error;
pub mod g2p;
pub mod graph;
pub mod hpoa;
pub mod identity_projection;
pub mod mechanism;
pub mod obo;
pub mod orphanet;
pub mod semantic_units;
pub mod sources;
pub mod trace;
pub mod withhold;

use std::path::{Path, PathBuf};

use atlas_core::{Atlas, snapshot};

pub use build::{build, build_with};
pub use error::IngestError;

/// Data root: `$RARE_ATLAS_DATA`, else the nearest `data/` with a `raw/` above the working
/// directory, else the repository's `data/`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RARE_ATLAS_DATA") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            let data = dir.join("data");
            if data.join("raw").is_dir() {
                return data;
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

pub fn raw_dir(data: &Path) -> PathBuf {
    data.join("raw")
}

pub fn snapshot_path(data: &Path) -> PathBuf {
    std::env::var_os("RARE_ATLAS_ATLAS_SNAPSHOT")
        .filter(|p| !p.is_empty())
        .or_else(|| std::env::var_os("RARE_ATLAS_SNAPSHOT").filter(|p| !p.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| snapshot_dir(data).join("atlas.snapshot"))
}

/// Operator-owned private cache for a server reading a shared source corpus.
pub fn snapshot_dir(data: &Path) -> PathBuf {
    std::env::var_os("RARE_ATLAS_SNAPSHOTS")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("cache"))
}

/// How [`load_or_build`] obtained the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Snapshot,
    Built,
}

/// Load the snapshot if it matches the current source files, else build and save it.
pub fn load_or_build(data: &Path, rebuild: bool) -> Result<(Atlas, Origin), IngestError> {
    let raw = raw_dir(data);
    let path = snapshot_path(data);
    let projection = identity_projection::load(data)?;
    let sig = format!(
        "{};identity:{}:{}:{}:{}",
        sources::signature(&raw)?,
        atlas_core::identity_policy::RULE,
        atlas_core::identity_policy::code_sha256(),
        projection.accepted.manifest_sha256,
        identity_projection::digest(include_str!("build.rs").replace("\r\n", "\n").as_bytes())
    );
    if !rebuild && path.exists() && snapshot::signature(&path).is_ok_and(|s| s == sig) {
        let (atlas, _) = snapshot::load(&path)?;
        return Ok((atlas, Origin::Snapshot));
    }
    let atlas = build(&raw)?;
    snapshot::save(&path, &atlas, &sig)?;
    Ok((atlas, Origin::Built))
}
