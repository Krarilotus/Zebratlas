//! Connected layer over the real caches: integrity contracts pass, header checksums recompute,
//! records re-verify, the snapshot round-trips. Skipped when data/raw or data/cache is missing.

use std::path::PathBuf;
use std::time::Instant;

use atlas_core::graph::Relation;
use atlas_core::{Graph, integrity, snapshot};
use atlas_ingest::graph;

fn data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let ok = atlas_ingest::sources::SOURCES
        .iter()
        .all(|s| data.join("raw").join(s.file).exists())
        && data.join("cache/trials/studies.jsonl.gz").exists();
    if !ok {
        eprintln!("skipped: data/raw or data/cache incomplete");
    }
    ok.then_some(data)
}

#[test]
fn graph_integrity_and_verification() {
    let Some(data) = data() else { return };
    let (atlas, _) = atlas_ingest::load_or_build(&data, false).expect("atlas");
    let t = Instant::now();
    let built = graph::build(&data, &atlas).expect("graph build");
    eprintln!("graph build: {:.1}s", t.elapsed().as_secs_f64());

    let dir = std::env::temp_dir().join(format!("atlas-graph-{}", std::process::id()));
    let path = dir.join("graph.snapshot");
    snapshot::save_graph(&path, &built, "sig").unwrap();
    let t = Instant::now();
    let (g, sig) = snapshot::load_graph(&path).unwrap();
    eprintln!("graph snapshot load: {:.2}s", t.elapsed().as_secs_f64());
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(sig, "sig");
    assert_eq!(g.data(), &built);

    let report = integrity::check(&atlas, &g);
    eprintln!("nodes {:?}", report.nodes);
    eprintln!("edges {:?}", report.edges_by_relation);
    for c in &report.contracts {
        eprintln!("contract {} checked {} violations {}", c.id, c.checked, c.violations);
    }
    for v in &report.violations {
        eprintln!("  {} {} {}", v.contract, v.subject, v.detail);
    }
    for c in &report.coverage {
        eprintln!(
            "coverage {} {} records {} header ok {} failed {}",
            c.source, c.status, c.records, c.header_checksums_verified, c.header_checksums_failed
        );
    }
    assert!(report.passed, "integrity contracts failed");
    for c in report.coverage.iter().filter(|c| c.status == "loaded") {
        assert_eq!(c.header_checksums_failed, 0, "{}: header checksum mismatch", c.source);
    }

    // STXBP1 is connected: studies, papers, grants and people reach the gene
    let stxbp1 = atlas.gene_at(atlas.gene("STXBP1").unwrap()).id().to_owned();
    let count = |r: Relation| g.incident(&stxbp1).filter(|i| i.edge.relation == r).count();
    eprintln!(
        "STXBP1: names_gene {} about_gene {}",
        count(Relation::NamesGene),
        count(Relation::AboutGene)
    );
    assert!(count(Relation::NamesGene) > 0 && count(Relation::AboutGene) > 100);

    // records re-verify: one per source kind
    verify_some(&data, &g);

    // J3: STX1A has no OMIM/Orphanet/MONDO condition; G2P defines two, kept as newly described
    // nodes linked narrower to the generic MONDO term; its Orphanet links are not causal
    let stx1a = atlas.gene("STX1A").expect("STX1A");
    let causal: Vec<&str> = atlas
        .gene_at(stx1a)
        .diseases
        .iter()
        .map(|&d| atlas.disease_at(d))
        .filter(|d| d.genes.iter().any(|l| l.symbol == "STX1A" && l.is_causal()))
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(causal, ["G2P:G2P03465", "G2P:G2P03466"], "{causal:?}");
    let d = atlas.disease("G2P:G2P03465").unwrap();
    assert!(d.is_newly_described());
    assert_eq!(d.classification("confidence"), Some("moderate"));
    assert!(
        d.related
            .iter()
            .any(|l| l.target == "MONDO:0100038" && l.relation == "narrower")
    );
    assert!(atlas.stats().newly_described > 0);
}

fn verify_some(data: &std::path::Path, g: &Graph) {
    let mut seen = std::collections::HashSet::new();
    for (i, r) in g.data().records.iter().enumerate() {
        if !seen.insert(r.entity) {
            continue;
        }
        let t = Instant::now();
        let v = graph::verify::record(data, g, i as u32);
        eprintln!(
            "verify {} {} {}: {} ({:.0} ms)",
            v.file,
            v.locator,
            v.record_id,
            v.matches,
            t.elapsed().as_secs_f64() * 1e3
        );
        assert!(v.matches, "{v:?}");
    }
}
