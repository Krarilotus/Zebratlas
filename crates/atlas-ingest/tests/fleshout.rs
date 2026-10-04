//! The fleshed-out graph over the real caches (read-only): builds the connected layer from the
//! atlas snapshot of `$RARE_ATLAS_DATA`, saves it to a temp file, and reports size, counts and the
//! integrity contracts. Never writes into the data dir. Run with
//! `RARE_ATLAS_DATA=<main checkout>/data cargo test -j 4 -p atlas-ingest --test fleshout -- --ignored --nocapture`.

use std::path::PathBuf;
use std::time::Instant;

use atlas_core::{integrity, snapshot};
use atlas_ingest::graph;

#[test]
#[ignore = "needs the real data dir (RARE_ATLAS_DATA)"]
fn fleshout_real_caches() {
    let Some(data) = std::env::var_os("RARE_ATLAS_DATA").map(PathBuf::from) else {
        eprintln!("skipped: RARE_ATLAS_DATA not set");
        return;
    };
    let (atlas, _) = snapshot::load(&atlas_ingest::snapshot_path(&data)).expect("atlas snapshot");
    let t = Instant::now();
    let built = graph::build(&data, &atlas).expect("graph build");
    eprintln!("graph build: {:.1}s", t.elapsed().as_secs_f64());
    let path = std::env::temp_dir().join(format!("atlas-fleshout-{}.snapshot", std::process::id()));
    snapshot::save_graph(&path, &built, "sig").unwrap();
    let bytes = std::fs::metadata(&path).unwrap().len();
    let t = Instant::now();
    let (g, _) = snapshot::load_graph(&path).unwrap();
    eprintln!(
        "graph snapshot: {:.1} MB, load {:.2}s",
        bytes as f64 / 1e6,
        t.elapsed().as_secs_f64()
    );
    std::fs::remove_file(&path).ok();
    let report = integrity::check(&atlas, &g);
    eprintln!("nodes {:?}", report.nodes);
    eprintln!("edges {:?}", report.edges_by_relation);
    eprintln!("licences {:?}", report.licences);
    for c in &report.contracts {
        eprintln!("contract {} checked {} violations {}", c.id, c.checked, c.violations);
    }
    for v in report.violations.iter().take(30) {
        eprintln!("  {} {} {}", v.contract, v.subject, v.detail);
    }
    for c in &report.coverage {
        eprintln!(
            "coverage {:<18} {:<10} records {:>6} nodes {:>5} edges {:>6} excluded {:>4} header ok {} failed {}",
            c.source,
            c.status,
            c.records,
            c.nodes,
            c.edges,
            c.excluded,
            c.header_checksums_verified,
            c.header_checksums_failed
        );
    }
    assert!(report.passed, "integrity contracts failed");
}
