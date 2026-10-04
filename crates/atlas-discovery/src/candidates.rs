//! Reader for the contribution queue `atlas.discovery.candidates` v1 (written by atlas-contrib on
//! acceptance of a `data_source` proposal). Each accepted proposal is verified (envelope and receipt
//! hashes, status, import flag) and staged for the normal acquisition and alignment review. Nothing is
//! acquired, aligned or merged here; `graph_import_allowed` stays false on every staged item.

use anyhow::{Result, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "atlas.discovery.candidates";
pub const STAGED_SCHEMA: &str = "atlas.discovery.staged_candidates";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Serialize)]
pub struct StagedProposal {
    /// `contrib:<id>` from the queue.
    pub id: String,
    /// The queue entity this item derives from (`urn:atlas:discovery-candidate:...`).
    pub derived_from: String,
    pub url: String,
    pub resource_kind: String,
    /// The submitter's licence claim, as given; never treated as permission.
    pub licence_claim: String,
    pub spdx_id_claim: String,
    pub identifier_systems: Vec<String>,
    pub accepted_by: Value,
    pub accepted_at: Value,
    pub receipt: Value,
    pub receipt_hash_verified: bool,
    /// Automatic check results by name → status, as reported by atlas-contrib.
    pub checks: Value,
    /// `pending_review` or `blocked` (with `blocked_reasons`).
    pub stage: String,
    pub blocked_reasons: Vec<String>,
    /// What a reviewer must do before any acquisition or alignment, in order.
    pub review_items: Vec<String>,
    pub publication_gate: String,
    pub graph_import_allowed: bool,
}

#[derive(Debug, Serialize)]
pub struct StagedQueue {
    pub schema: &'static str,
    pub version: u32,
    pub input: Value,
    pub staged: Vec<StagedProposal>,
    /// The queue's PROV-O histories, verbatim, plus our staging activity.
    pub provenance: Vec<Value>,
}

/// Verify and stage. `queue_path` is recorded only as provenance; `bytes` are the exact file bytes.
pub fn stage(bytes: &[u8], queue_path: &str, staged_at: &str) -> Result<StagedQueue> {
    let v: Value = serde_json::from_slice(bytes)?;
    ensure!(v["schema"] == SCHEMA, "not an {SCHEMA} file (schema = {})", v["schema"]);
    ensure!(v["version"] == 1, "unsupported {SCHEMA} version {}", v["version"]);
    let Some(candidates) = v["candidates"].as_array() else {
        bail!("candidates array missing")
    };
    let envelope = sha(&serde_json::to_vec(&v["candidates"])?);
    if v["sha256"].as_str() != Some(envelope.as_str()) {
        bail!(
            "envelope sha256 mismatch: file says {}, candidates hash to {envelope}",
            v["sha256"]
        );
    }
    let file_sha = sha(bytes);
    let queue_entity = format!("urn:sha256:{file_sha}");
    let activity = format!("urn:atlas:discovery:stage-candidates:{file_sha}");
    let mut staged = Vec::new();
    for c in candidates {
        let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
        let mut blocked = Vec::new();
        let receipt_ok = c["submission"].is_object()
            && c["source"]["sha256"].as_str() == Some(sha(&serde_json::to_vec(&c["submission"])?).as_str());
        if !receipt_ok {
            blocked.push("receipt sha256 does not match the public submission".to_string());
        }
        if c["status"] != "accepted_for_discovery" {
            blocked.push(format!("status {} is not accepted_for_discovery", c["status"]));
        }
        if c["graph_import_allowed"] != false {
            blocked.push("graph_import_allowed is not false; the queue contract forbids direct import".into());
        }
        if !s("url").starts_with("https://") && !s("url").starts_with("http://") {
            blocked.push("url is not http(s)".into());
        }
        let checks: serde_json::Map<String, Value> = c["checks"]["checks"]
            .as_array()
            .or_else(|| c["checks"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|k| Some((k["name"].as_str()?.to_string(), k["status"].clone())))
            .collect();
        let mut review_items = vec![
            "adjudicate the licence from the provider's own terms (the submitter's claim is not permission; D35-D37 gate)".to_string(),
            "verify the retained response snapshot hashes listed in checks against the snapshot files".into(),
            "characterize schema, identifier spaces, version and update cadence; write a source manifest".into(),
            "acquire the data under data/raw/<source>/<version>/ with a PROV manifest".into(),
            "align identifiers with the exact-only rules (docs/design/IDENTITY.md); everything else stays a candidate".into(),
        ];
        if checks.values().any(|st| st != "pass") {
            review_items.insert(0, "resolve non-passing automatic checks first".into());
        }
        staged.push(StagedProposal {
            id: s("id"),
            derived_from: s("@id"),
            url: s("url"),
            resource_kind: s("resource_kind"),
            licence_claim: s("licence"),
            spdx_id_claim: s("spdx_id"),
            identifier_systems: c["identifier_systems"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            accepted_by: c["accepted_by"].clone(),
            accepted_at: c["accepted_at"].clone(),
            receipt: c["source"].clone(),
            receipt_hash_verified: receipt_ok,
            checks: Value::Object(checks),
            stage: if blocked.is_empty() {
                "pending_review"
            } else {
                "blocked"
            }
            .into(),
            blocked_reasons: blocked,
            review_items,
            publication_gate: "closed until the licence is adjudicated".into(),
            graph_import_allowed: false,
        });
    }
    let mut provenance: Vec<Value> = v["provenance"].as_array().cloned().unwrap_or_default();
    provenance.push(json!({
        "@context": {"prov": "http://www.w3.org/ns/prov#"},
        "@graph": [
            {"@id": queue_entity, "@type": "prov:Entity", "atlas:path": queue_path, "atlas:sha256": file_sha,
             "atlas:schema": SCHEMA, "atlas:envelope_sha256": envelope},
            {"@id": activity, "@type": "prov:Activity", "atlas:label": "verify and stage accepted source proposals",
             "prov:used": {"@id": queue_entity}, "prov:endedAtTime": staged_at,
             "prov:wasAssociatedWith": {"@id": "urn:atlas:software:atlas-discovery:0.1.0"}},
            {"@id": "urn:atlas:software:atlas-discovery:0.1.0", "@type": "prov:SoftwareAgent", "atlas:version": env!("CARGO_PKG_VERSION")}
        ]
    }));
    for p in &staged {
        if let Some(g) = provenance.last_mut().and_then(|x| x["@graph"].as_array_mut()) {
            g.push(
                json!({"@id": format!("urn:atlas:discovery:staged:{}:{file_sha}", p.id), "@type": "prov:Entity",
                "prov:wasDerivedFrom": {"@id": p.derived_from}, "prov:wasGeneratedBy": {"@id": activity},
                "atlas:stage": p.stage}),
            );
        }
    }
    Ok(StagedQueue {
        schema: STAGED_SCHEMA,
        version: 1,
        input: json!({"path": queue_path, "sha256": file_sha, "envelope_sha256": envelope, "generated_at": v["generated_at"], "candidates": candidates.len()}),
        staged,
        provenance,
    })
}
