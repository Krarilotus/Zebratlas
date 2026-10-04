//! Parity with the Python reference (`uv run python -m rare_atlas.build`, stored in
//! fixtures/python_stats.json). The Python drops retired Orphanet entries; the Rust graph keeps
//! them as retired nodes, so the comparison covers active nodes and reports retired ones apart.
//! Skipped when data/raw is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use atlas_core::provenance::{Locator, activity};
use atlas_core::snapshot;

fn data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let ok = atlas_ingest::sources::SOURCES
        .iter()
        .all(|s| data.join("raw").join(s.file).exists());
    if !ok {
        eprintln!("skipped: data/raw incomplete");
    }
    ok.then_some(data)
}

/// INTENTIONAL deviation from the Python reference (D30.1, docs/plan/DECISIONS.md): label-only
/// matches no longer merge. The 161 source ids the Python merges on a name match keep their own
/// nodes, each with one `candidate_same_as` link to its MONDO term. Stats that change, with the
/// Python value they replace. Everything else must still equal the Python exactly, and the
/// pre-D30 policy (`LabelPolicy::Merge`) must still reproduce every Python number.
const D30_STATS: [(&str, u64, u64); 6] = [
    // (stat, python = pre-D30, atlas after D30)
    ("diseases", 20162, 20269),
    ("rare_diseases", 18459, 18522),
    ("diseases_with_phenotypes", 10689, 10690),
    ("diseases_with_genes", 8696, 8697),
    ("merged_from_multiple_sources", 3428, 3393),
    ("phenotype_edges", 248384, 248387),
];
/// Label-only matches: merged by the Python, candidates after D30.
const D30_CANDIDATES: usize = 161;

fn stats_map(atlas: &atlas_core::Atlas) -> BTreeMap<String, u64> {
    serde_json::from_value(serde_json::to_value(atlas.stats()).unwrap()).unwrap()
}

#[test]
fn stats_match_python_and_snapshot_round_trips() {
    let Some(data) = data() else { return };
    let raw = atlas_ingest::raw_dir(&data);
    let fixture = include_str!("fixtures/python_stats.json");
    let python: BTreeMap<String, u64> = serde_json::from_str(fixture).unwrap();

    // pre-D30 identity: still exactly the Python reference
    let legacy = atlas_ingest::build_with(&raw, atlas_core::identity::LabelPolicy::Merge).expect("legacy build");
    let ours = stats_map(&legacy);
    for (k, v) in &python {
        assert_eq!(
            ours.get(k),
            Some(v),
            "pre-D30 stat {k}: rust {:?} vs python {v}",
            ours.get(k)
        );
    }
    assert_eq!(legacy.identity.merge_counts()[2].1, D30_CANDIDATES);
    drop(legacy);

    let t = std::time::Instant::now();
    let atlas = atlas_ingest::build(&raw).expect("build");
    eprintln!("build: {:.1}s", t.elapsed().as_secs_f64());
    let stats = atlas.stats();
    let ours = stats_map(&atlas);
    eprintln!("D30 stats: {ours:?}");
    for (k, v) in &python {
        let expected = D30_STATS.iter().find(|(s, ..)| s == k).map_or(*v, |&(_, py, d30)| {
            assert_eq!(py, *v, "D30 table: python value of {k}");
            d30
        });
        assert_eq!(
            ours.get(k),
            Some(&expected),
            "stat {k}: rust {:?} vs expected {expected} (python {v})",
            ours.get(k)
        );
    }
    // D30.1: label matches are candidates, never merges; every candidate keeps its own active node
    assert_eq!(atlas.identity.merge_counts()[2].1, 0);
    assert_eq!(atlas.identity.candidates().count(), D30_CANDIDATES);
    for c in atlas.identity.candidates() {
        assert_eq!(atlas.identity.resolve(&c.source_id), c.source_id);
        assert!(
            atlas.identity.is_live_mondo(&c.target),
            "{} -> {}",
            c.source_id,
            c.target
        );
        assert!(!c.matched_labels.is_empty() && !c.target_label.is_empty());
    }
    eprintln!("retired diseases (kept, not in Python): {}", stats.retired_diseases);
    assert!(stats.retired_diseases > 0);

    // provenance: every source has a checksum, evidence rows point at their line
    assert!(
        atlas
            .provenance
            .entities
            .iter()
            .all(|e| e.sha256.as_ref().is_some_and(|h| h.len() == 64))
    );
    let hpoa = atlas.provenance.activity_by_id(activity::INGEST_HPOA).unwrap();
    eprintln!("hpoa counts: {:?}", hpoa.counts);
    let d = atlas.disease("OMIM:619340").expect("DEE 96");
    let first = &d.phenotypes[0].annotations[0];
    assert_eq!(
        first.record.locator,
        Locator::Line(6),
        "first data row of phenotype.hpoa is line 6"
    );

    let dir = std::env::temp_dir().join(format!("atlas-snap-{}", std::process::id()));
    let path = dir.join("atlas.snapshot");
    snapshot::save(&path, &atlas, "sig").unwrap();
    let t = std::time::Instant::now();
    let (loaded, sig) = snapshot::load(&path).unwrap();
    eprintln!("snapshot load: {:.2}s", t.elapsed().as_secs_f64());
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(sig, "sig");
    assert_eq!(loaded.stats(), stats);
    assert_eq!(loaded.provenance, atlas.provenance);
}
