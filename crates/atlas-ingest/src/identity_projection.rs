//! Verified, immutable identity projection shared by native ingest and SSSOM.
use crate::IngestError;
use atlas_core::identity_policy::{self, AcceptedIdentity};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub fn directory(data: &Path) -> PathBuf {
    std::env::var_os("RARE_ATLAS_MAPPING_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("cache/mappings"))
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn invalid(path: &Path, reason: impl Into<String>) -> IngestError {
    IngestError::Schema {
        path: path.into(),
        found: reason.into(),
        expected: "complete hash-bound identity gate projection".into(),
    }
}
#[derive(Default)]
pub struct Projection {
    pub accepted: AcceptedIdentity,
    /// Decision id -> assertion id. Never includes candidates or rejected assertions.
    pub decisions: HashMap<String, String>,
    pub pairs: HashMap<String, (String, String)>,
}
impl Projection {
    pub fn permits(&self, decision: &str, assertion: &str, subject: &str, object: &str) -> bool {
        self.decisions.get(decision).is_some_and(|a| a == assertion)
            && self
                .pairs
                .get(decision)
                .is_some_and(|(a, b)| a == subject && b == object)
    }
}
/// Missing gates allow candidate-only inputs; exact consumption requires a verified receipt.
/// Existing but incomplete/tampered gates always fail, including on snapshot cache hits.
pub fn load(data: &Path) -> Result<Projection, IngestError> {
    load_directory(&directory(data))
}
pub fn load_directory(dir: &Path) -> Result<Projection, IngestError> {
    let path = dir.join("identity-gate.manifest.json");
    if !path.exists() {
        if std::env::var_os("RARE_ATLAS_REQUIRE_IDENTITY_GATE").is_some_and(|s| s == "1") {
            return Err(invalid(&path, "missing completion manifest"));
        }
        return Ok(Projection::default());
    }
    let bytes = std::fs::read(&path).map_err(IngestError::io(&path))?;
    if let Ok(pin) = std::env::var("RARE_ATLAS_IDENTITY_MANIFEST_SHA256") {
        if pin != digest(&bytes) {
            return Err(invalid(&path, "manifest differs from configured pin"));
        }
    }
    let manifest: Value = serde_json::from_slice(&bytes).map_err(|source| IngestError::Json {
        path: path.clone(),
        source,
    })?;
    if manifest["schema"] != "atlas.identity.gate"
        || manifest["version"] != 1
        || manifest["identity_policy_sha256"] != identity_policy::code_sha256()
        || manifest["rule_id"] != identity_policy::RULE
        || manifest["rule_version"] != identity_policy::VERSION
    {
        return Err(invalid(&path, "unsupported gate schema or policy"));
    }
    let outputs = manifest["outputs"]
        .as_array()
        .ok_or_else(|| invalid(&path, "missing outputs"))?;
    let mut inventory = HashSet::new();
    for output in outputs {
        let file = output["file"]
            .as_str()
            .ok_or_else(|| invalid(&path, "invalid output name"))?;
        if file.is_empty()
            || file.contains(['/', '\\', ':'])
            || file == "."
            || file == ".."
            || !inventory.insert(file.to_owned())
        {
            return Err(invalid(&path, "unsafe or duplicate output name"));
        }
        let p = dir.join(file);
        let hash = crate::sources::sha256(&p)?;
        if output["sha256"] != hash {
            return Err(invalid(&p, "output hash mismatch"));
        }
    }
    for file in [
        "identity-decisions.jsonl",
        "identity-invalidations.jsonl",
        "rules.json",
        "review-diff.tsv",
    ] {
        if !inventory.contains(file) {
            return Err(invalid(&path, format!("missing {file}")));
        }
    }
    if std::fs::read(dir.join("rules.json")).map_err(IngestError::io(&path))?
        != atlas_core::identity_rules::REGISTRY_JSON.as_bytes()
    {
        return Err(invalid(&path, "rule registry differs from consumer"));
    }
    for entry in std::fs::read_dir(dir).map_err(IngestError::io(dir))? {
        let entry = entry.map_err(IngestError::io(dir))?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.ends_with(".sssom.tsv") && !inventory.contains(&file) {
            return Err(invalid(&path, "unlisted mapping file"));
        }
    }
    let ledger = dir.join("identity-decisions.jsonl");
    let reader = BufReader::new(std::fs::File::open(&ledger).map_err(IngestError::io(&ledger))?);
    let mut pairs = Vec::new();
    let mut endpoints = HashMap::new();
    let mut decisions = HashMap::new();
    for line in reader.lines() {
        let line = line.map_err(IngestError::io(&ledger))?;
        let row: Value = serde_json::from_str(&line).map_err(|source| IngestError::Json {
            path: ledger.clone(),
            source,
        })?;
        if row["status"] != "accepted" {
            continue;
        }
        let original = &row["prov:wasDerivedFrom"];
        let columns = original["original_columns"]
            .as_array()
            .ok_or_else(|| invalid(&ledger, "missing original columns"))?;
        let values = original["original_values"]
            .as_array()
            .ok_or_else(|| invalid(&ledger, "missing original cells"))?;
        let get = |key: &str| {
            columns
                .iter()
                .position(|c| c == key)
                .and_then(|i| values.get(i))
                .and_then(Value::as_str)
                .unwrap_or("")
        };
        identity_policy::eligibility(
            get("subject_id"),
            get("object_id"),
            identity_policy::source_method(get("mapping_justification")),
            get("conflict"),
            get("rule_id"),
        )
        .map_err(|reason| invalid(&ledger, reason))?;
        let decision = row["@id"]
            .as_str()
            .ok_or_else(|| invalid(&ledger, "missing decision id"))?;
        let assertion = original["@id"]
            .as_str()
            .ok_or_else(|| invalid(&ledger, "missing assertion id"))?;
        if decisions.insert(decision.into(), assertion.into()).is_some() {
            return Err(invalid(&ledger, "duplicate accepted decision"));
        }
        let pair = (get("subject_id").to_owned(), get("object_id").to_owned());
        endpoints.insert(decision.into(), pair.clone());
        pairs.push(pair);
    }
    Ok(Projection {
        accepted: AcceptedIdentity::new(pairs, digest(&bytes)),
        decisions,
        pairs: endpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_projection_rejects_tampered_bytes_and_extra_mapping_files() {
        use crate::graph::fixtures::TempData;
        let d = TempData::new();
        let source = d.path().join("source");
        let output = d.path().join("gate");
        std::fs::create_dir(&source).unwrap();
        let text = format!(
            "subject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\nMONDO:1\tskos:exactMatch\tOMIM:1\tsemapv:BackgroundKnowledgeBasedMatching\tR-DIS-01\t1.0.0\thttps://example.org\t{}\trecord\n",
            "a".repeat(64)
        );
        std::fs::write(source.join("test.sssom.tsv"), text).unwrap();
        atlas_discovery::align::safety::directory(&source, &output, None).unwrap();
        let gate = load_directory(&output).unwrap();
        assert!(gate.accepted.equivalent("MONDO:1", "OMIM:1"));
        assert_eq!(gate.decisions.len(), 1);
        std::fs::write(output.join("unlisted.sssom.tsv"), "").unwrap();
        assert!(load_directory(&output).is_err());
        std::fs::remove_file(output.join("unlisted.sssom.tsv")).unwrap();
        std::fs::write(output.join("test.sssom.tsv"), "tampered").unwrap();
        assert!(load_directory(&output).is_err());
    }
}
