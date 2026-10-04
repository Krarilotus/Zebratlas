//! Read sources, build privately, audit every retained alias, sample 20 clusters, rehash their rows.
use atlas_core::{Graph, snapshot};
use atlas_ingest::graph::{sssom, verify};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = atlas_ingest::data_dir();
    let start = Instant::now();
    let atlas = snapshot::load(&atlas_ingest::snapshot_path(&data))
        .map(|(a, _)| a)
        .or_else(|_| atlas_ingest::build(&data.join("raw")))?;
    let withhold = atlas_ingest::withhold::load(&data, atlas_ingest::withhold::salt_from_env())?;
    let graph = Graph::new(atlas_ingest::graph::build_with(&data, &atlas, &withhold)?);
    let audit = atlas_core::integrity::check(&atlas, &graph);
    let contract = audit
        .contracts
        .iter()
        .find(|c| c.id == "identity-merge-has-rule-evidence")
        .unwrap();
    assert!(contract.passed, "{:?}", audit.violations);
    let mut clusters: Vec<_> = graph.data().identity_merges.iter().collect();
    clusters.sort_by_cached_key(|m| {
        format!(
            "{:x}",
            Sha256::digest(format!("merge-provenance-20-v1|{}", m.canonical))
        )
    });
    assert!(clusters.len() >= 20, "need at least 20 actual merged nodes");
    let mut sample = Vec::new();
    for merge in clusters.iter().take(20) {
        assert!(!merge.mappings.is_empty());
        assert!(merge.members.iter().all(|m| !m.derived_from.is_empty()));
        let mut rows = Vec::new();
        for mapping in &merge.mappings {
            assert!(atlas_core::identity_rules::rule(&mapping.rule_id, &mapping.rule_version).is_some());
            assert!(!mapping.evidence_locator.is_empty());
            assert_eq!(mapping.evidence_sha256.len(), 64);
            let checked = verify::record(&data, &graph, mapping.record);
            assert!(checked.matches, "mapping row changed: {checked:?}");
            rows.push(json!({"mapping_set_id": mapping.mapping_set_id, "rule_id": mapping.rule_id,
                "rule_version": mapping.rule_version, "row_sha256": atlas_core::graph::hex(&graph.record(mapping.record).sha256),
                "row_locator": graph.record(mapping.record).locator.to_string(), "evidence_sha256": mapping.evidence_sha256,
                "evidence_locator": mapping.evidence_locator, "row_rehash_matches": checked.matches}));
        }
        // Public non-person identifiers only in the audit artifact.
        if merge.mappings.iter().all(|m| !m.rule_id.starts_with("R-PER-")) {
            sample.push(json!({"canonical": merge.canonical, "activity": merge.activity.id, "mapping_rows": rows}));
        } else {
            sample.push(json!({"withheld_person_cluster": true, "verified_rows": rows.len()}));
        }
    }
    let result = json!({"schema": "atlas.merge-provenance.acceptance", "version": 1,
        "generated_at": atlas_core::provenance::rfc3339(std::time::SystemTime::now()),
        "seed": "merge-provenance-20-v1", "merged_clusters": clusters.len(), "aliases_checked": graph.data().aliases.len(),
        "contract": contract, "sample_size": sample.len(), "sample": sample,
        "mapping_files": sssom::files(&data).iter().map(|p| json!({"file": p.file_name().unwrap().to_string_lossy(),
            "sha256": sssom::file_sha256(p).unwrap()})).collect::<Vec<_>>(),
        "elapsed_seconds": start.elapsed().as_secs_f64()});
    let out = std::env::var("RARE_ATLAS_MERGE_AUDIT").expect("set RARE_ATLAS_MERGE_AUDIT to a private output path");
    std::fs::write(out, serde_json::to_vec_pretty(&result)?)?;
    println!(
        "PASS: {} aliases, {} clusters, 20 hash-ranked random nodes; all sampled rows rehash",
        graph.data().aliases.len(),
        clusters.len()
    );
    Ok(())
}
