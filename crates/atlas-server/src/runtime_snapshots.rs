//! Checksum-pinned operational mechanism loading does not depend on transferred file mtimes.
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use atlas_core::mechanism::MechanismData;

fn checked_path(manifest: &serde_json::Value, override_path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = override_path
        .or_else(|| manifest["mechanism_snapshot"].as_str().map(PathBuf::from))
        .context("Missing operational mechanism snapshot path")?;
    let expected = manifest["mechanism_sha256"]
        .as_str()
        .context("Missing operational mechanism checksum")?;
    anyhow::ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid mechanism checksum"
    );
    anyhow::ensure!(
        atlas_ingest::sources::sha256(&path)? == expected,
        "Operational mechanism checksum mismatch"
    );
    Ok(path)
}

pub fn load_mechanism(data: &Path) -> anyhow::Result<MechanismData> {
    let path = std::env::var_os("ATLAS_OPERATIONAL_MANIFEST").context("Missing operational manifest")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 256 * 1024, "Operational manifest exceeds limit");
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        manifest["public_release"] == false,
        "Expected private operational manifest"
    );
    let projection = atlas_ingest::identity_projection::load(data)?;
    let gate = &projection.accepted.manifest_sha256;
    anyhow::ensure!(
        manifest["identity_gate_sha256"].as_str() == Some(gate.as_str()),
        "Operational identity gate mismatch"
    );
    let path = checked_path(
        &manifest,
        std::env::var_os("RARE_ATLAS_MECHANISM_SNAPSHOT").map(PathBuf::from),
    )?;
    let (mechanism, signature) = atlas_ingest::mechanism::load(&path)?;
    let binding = format!(
        "identity:{}:{}:{}:{}:",
        atlas_core::identity_policy::RULE,
        atlas_core::identity_policy::VERSION,
        atlas_core::identity_policy::code_sha256(),
        gate
    );
    anyhow::ensure!(
        signature.contains(&binding),
        "Operational mechanism identity binding mismatch"
    );
    Ok(mechanism)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanism_pin_rejects_missing_modified_and_override_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = dir.path().join("mechanism.snapshot");
        std::fs::write(&snapshot, b"immutable fixture").unwrap();
        let manifest = serde_json::json!({"mechanism_snapshot":snapshot,"mechanism_sha256":atlas_ingest::sources::sha256(&snapshot).unwrap()});
        assert_eq!(checked_path(&manifest, None).unwrap(), snapshot);
        assert!(checked_path(&serde_json::json!({"mechanism_snapshot":snapshot}), None).is_err());
        let other = dir.path().join("other.snapshot");
        std::fs::write(&other, b"unapproved override").unwrap();
        assert!(checked_path(&manifest, Some(other)).is_err());
        std::fs::write(&snapshot, b"altered fixture").unwrap();
        assert!(checked_path(&manifest, None).is_err());
    }
}
