//! The contribution queue (`atlas.discovery.candidates` v1) is verified and staged, never imported.

use atlas_discovery::candidates::stage;
use serde_json::Value;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/discovery-candidates.json");

fn fixture() -> Vec<u8> {
    std::fs::read(FIXTURE).unwrap()
}

#[test]
fn accepted_proposal_is_staged_for_review_with_provenance() {
    let q = stage(&fixture(), "fixture", "2026-10-04T00:00:00Z").unwrap();
    assert_eq!(q.staged.len(), 1);
    let p = &q.staged[0];
    assert_eq!(p.id, "contrib:placeholder-0001");
    assert_eq!(p.stage, "pending_review");
    assert!(p.receipt_hash_verified);
    assert!(!p.graph_import_allowed);
    assert_eq!(p.checks["identifiers"], "warn");
    assert!(
        p.review_items[0].contains("non-passing"),
        "a warning check comes first in the review list"
    );
    assert!(p.review_items.iter().any(|i| i.contains("licence")));
    // Upstream PROV-O history kept verbatim, plus our staging activity deriving from the queue entity.
    assert_eq!(q.provenance.len(), 2);
    let ours = serde_json::to_string(&q.provenance[1]).unwrap();
    assert!(ours.contains("urn:atlas:discovery-candidate:placeholder-0001:v3:export-test"));
    assert!(ours.contains("prov:wasDerivedFrom"));
}

#[test]
fn tampered_submission_is_blocked_not_trusted() {
    let mut v: Value = serde_json::from_slice(&fixture()).unwrap();
    v["candidates"][0]["submission"]["data_source"]["url"] = "https://example.invalid/other".into();
    // Re-seal the envelope so only the receipt hash is wrong.
    use sha2::{Digest, Sha256};
    v["sha256"] = format!("{:x}", Sha256::digest(serde_json::to_vec(&v["candidates"]).unwrap())).into();
    let q = stage(&serde_json::to_vec(&v).unwrap(), "fixture", "t").unwrap();
    assert_eq!(q.staged[0].stage, "blocked");
    assert!(!q.staged[0].receipt_hash_verified);
}

#[test]
fn import_flag_or_broken_envelope_is_refused() {
    let mut v: Value = serde_json::from_slice(&fixture()).unwrap();
    v["candidates"][0]["graph_import_allowed"] = true.into();
    assert!(
        stage(&serde_json::to_vec(&v).unwrap(), "fixture", "t").is_err(),
        "envelope hash no longer matches"
    );
    use sha2::{Digest, Sha256};
    v["sha256"] = format!("{:x}", Sha256::digest(serde_json::to_vec(&v["candidates"]).unwrap())).into();
    let q = stage(&serde_json::to_vec(&v).unwrap(), "fixture", "t").unwrap();
    assert_eq!(q.staged[0].stage, "blocked");
    assert!(
        q.staged[0]
            .blocked_reasons
            .iter()
            .any(|r| r.contains("graph_import_allowed"))
    );
    v["schema"] = "something.else".into();
    assert!(stage(&serde_json::to_vec(&v).unwrap(), "fixture", "t").is_err());
}
