//! Reading and writing the withholding lists (`data/suppression.json`, `data/cache/quarantine.json`).
//! The filter itself is `atlas_core::withhold`; this module only does the file IO.

use std::path::{Path, PathBuf};

use atlas_core::withhold::{
    QUARANTINE_FILE, SALT_ENV, SUPPRESSION_FILE, Salt, SuppressionEntry, SuppressionFile, Withhold,
};

use crate::error::IngestError;

pub fn suppression_path(data: &Path) -> PathBuf {
    data.join(SUPPRESSION_FILE)
}

pub fn quarantine_path(data: &Path) -> PathBuf {
    data.join(QUARANTINE_FILE)
}

/// The salt from `ATLAS_SUPPRESSION_SALT`, else the documented development salt.
pub fn salt_from_env() -> Salt {
    Salt::from_config(std::env::var(SALT_ENV).ok().as_deref())
}

fn read_opt(path: &Path) -> Result<Option<Vec<u8>>, IngestError> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(IngestError::Io {
            path: path.to_owned(),
            source: e,
        }),
    }
}

/// Strict load for builds and releases: any unreadable or invalid list is an error.
pub fn load(data: &Path, salt: Salt) -> Result<Withhold, IngestError> {
    let s = read_opt(&suppression_path(data))?;
    let q = read_opt(&quarantine_path(data))?;
    Withhold::from_lists(salt, s.as_deref(), q.as_deref()).map_err(|e| IngestError::Withhold(e.to_string()))
}

/// Load for servers: an error gives a closed filter (every person withheld) and is logged once,
/// without any list content.
pub fn load_or_closed(data: &Path, salt: Salt) -> Withhold {
    match load(data, salt.clone()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("withhold: list unavailable, failing closed: {e}");
            Withhold::closed(salt, e.to_string())
        }
    }
}

/// Signature part for snapshot freshness: salt id + both lists' size and content hash.
pub fn signature(data: &Path, salt: &Salt) -> Result<String, IngestError> {
    use sha2::{Digest, Sha256};
    let mut sig = format!("withhold:{};", salt.id());
    for p in [suppression_path(data), quarantine_path(data)] {
        match read_opt(&p)? {
            Some(b) => {
                let h = Sha256::digest(&b);
                sig.push_str(&format!("{}:{:x};", b.len(), h));
            }
            None => sig.push_str("-;"),
        }
    }
    Ok(sig)
}

/// Append one entry to `data/suppression.json` (validated, written to a temp file, then renamed).
/// A list that cannot be read is never overwritten: the error stops the approval.
pub fn append_suppression(data: &Path, entry: SuppressionEntry) -> Result<(), IngestError> {
    let path = suppression_path(data);
    let mut file = match read_opt(&path)? {
        Some(b) => serde_json::from_slice::<SuppressionFile>(&b).map_err(|e| IngestError::Json {
            path: path.clone(),
            source: e,
        })?,
        None => SuppressionFile::new(),
    };
    if file.entries.iter().any(|e| e.id == entry.id) {
        return Ok(());
    }
    file.entries.push(entry);
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| IngestError::Json {
        path: path.clone(),
        source: e,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(IngestError::io(&tmp))?;
    std::fs::rename(&tmp, &path).map_err(IngestError::io(&path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::withhold::{KeyKind, Scope};

    #[test]
    fn append_then_load_and_signature_changes() {
        let dir = std::env::temp_dir().join(format!("atlas-withhold-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        let salt = Salt::new("test-salt");
        let empty_sig = signature(&dir, &salt).unwrap();
        assert!(load(&dir, salt.clone()).unwrap().suppression_entries().is_empty());
        let e = SuppressionEntry {
            id: "sup_1".into(),
            keys: salt.key(KeyKind::Orcid, "0000-0002-1825-0097").into_iter().collect(),
            scope: Scope::All,
            reason: "gdpr_art17_erasure".into(),
            date: "2026-10-04T10:00:00Z".into(),
            reviewer: "agent:reviewer/tester".into(),
            request: None,
            salt_id: salt.id().into(),
        };
        append_suppression(&dir, e.clone()).unwrap();
        append_suppression(&dir, e).unwrap();
        let w = load(&dir, salt.clone()).unwrap();
        assert_eq!(w.suppression_entries().len(), 1, "idempotent by id");
        assert_ne!(signature(&dir, &salt).unwrap(), empty_sig);
        std::fs::write(suppression_path(&dir), b"not json").unwrap();
        assert!(load(&dir, salt.clone()).is_err(), "strict load fails");
        assert!(
            load_or_closed(&dir, salt).closed_reason().is_some(),
            "server load closes"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
