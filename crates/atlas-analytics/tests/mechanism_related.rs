//! Demo stories of the mechanism layer on the real data (J2.1, J2.2, J3.2), with default settings
//! (Reactome + GO-BP, G2P-first effects). Skipped when data/raw is incomplete.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use atlas_analytics::cluster::{self, ClusterParams};
use atlas_analytics::{CounterexampleKind, SimilarityIndex, SimilarityParams, Verdict};
use atlas_ingest::mechanism;

/// One index for both tests (they run in parallel; building twice would race on the snapshot).
fn index() -> Option<&'static SimilarityIndex> {
    static INDEX: OnceLock<Option<SimilarityIndex>> = OnceLock::new();
    INDEX.get_or_init(build).as_ref()
}

fn build() -> Option<SimilarityIndex> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let raw = data.join("raw");
    if !atlas_ingest::sources::SOURCES
        .iter()
        .chain(mechanism::SOURCES.iter())
        .all(|s| raw.join(s.file).exists())
    {
        eprintln!("skipped: data/raw incomplete");
        return None;
    }
    let (atlas, _) = atlas_ingest::load_or_build(&data, false).expect("atlas");
    let (m, _) = mechanism::load_or_build(&data, &atlas, false).expect("mechanism");
    Some(SimilarityIndex::new(
        Arc::new(atlas),
        Arc::new(m),
        SimilarityParams::default(),
    ))
}

#[test]
fn demo_stories() {
    let Some(index) = index() else { return };

    // STXBP1 DEE4: the CPLX1 DEE is a same-mechanism neighbour through vesicle-fusion processes
    let r = index.related("OMIM:612164", 10).expect("STXBP1 DEE4 by OMIM id");
    assert_eq!(r.query.id, "MONDO:0012812");
    assert_eq!(r.items.len(), 10);
    let cplx1 = r
        .items
        .iter()
        .find(|i| i.neighbour.id == "MONDO:0033372")
        .expect("CPLX1 DEE63 in top 10");
    assert_eq!(cplx1.verdict, Verdict::SameMechanism);
    assert!(
        cplx1
            .mechanism
            .shared_processes
            .iter()
            .any(|p| p.source == "go_bp" && p.name.contains("synaptic vesicle"))
    );
    assert!(r.items.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(
        r.counterexamples
            .iter()
            .any(|c| c.kind == CounterexampleKind::LookalikeDifferentMechanism)
    );
    assert!(
        r.genes
            .iter()
            .any(|g| g.symbol == "STXBP1" && g.dosage.as_ref().is_some_and(|d| d.haploinsufficiency == "3"))
    );
    // provenance: activity with parameters and every used entity checksummed
    let a = &r.provenance.activity;
    assert_eq!(a.id, atlas_analytics::related::ACTIVITY_RELATED);
    assert_eq!(a.parameters["query"], "MONDO:0012812");
    assert_eq!(a.used.len(), r.provenance.inputs.len());
    assert!(r.provenance.inputs.iter().any(|e| e.file == mechanism::GO_GAF));
    assert!(r.provenance.inputs.iter().all(|e| e.sha256.is_some()));

    // KCNQ2: DEE7 (GoF) vs benign familial neonatal seizures 1 (LoF) is a counterexample, not a neighbour
    let r = index.related("MONDO:0013387", 10).unwrap();
    let c = r
        .counterexamples
        .iter()
        .find(|c| c.kind == CounterexampleKind::SameGeneDifferentMechanism)
        .expect("KCNQ2 counterexample");
    assert_eq!(c.gene.as_deref(), Some("KCNQ2"));
    let n = c.neighbour.as_ref().unwrap();
    assert_eq!(n.neighbour.id, "MONDO:0007365");
    assert_eq!(n.verdict, Verdict::SameGeneDifferentMechanism);
    assert_eq!(n.mechanism.score, 0.0);
    assert!(n.mechanism.effect_conflicts[0].opposite);
    assert!(r.items.iter().all(|i| i.neighbour.id != "MONDO:0007365"));

    // UNC13A: G2P curates monoallelic GoF and biallelic LoF
    let g = index.gene_mechanisms("UNC13A");
    assert!(g.split);
    let effects: Vec<&str> = g
        .curations
        .iter()
        .filter_map(|c| c.effect)
        .map(|e| e.as_str())
        .collect();
    assert!(effects.contains(&"LoF") && effects.contains(&"GoF"));
    // where the build gives each G2P model its own node, the split is a disease-level counterexample
    if index.resolve("G2P:G2P03909").is_some() {
        let r = index.related("G2P:G2P03909", 10).unwrap();
        assert!(r.counterexamples.iter().any(|c| {
            c.kind == CounterexampleKind::SameGeneDifferentMechanism
                && c.neighbour.as_ref().is_some_and(|n| n.neighbour.id == "G2P:G2P03905")
        }));
    }

    // unknown ids are an error, not a panic
    assert!(index.related("MONDO:9999999", 5).is_err());
}

#[test]
fn slice_and_clusters() {
    let Some(index) = index() else { return };
    let slice = cluster::dee_slice(index);
    assert!(slice.from_dee >= 120, "{}", slice.from_dee);
    assert!(slice.from_vesicle_genes > 0);
    assert!(
        slice.seed_processes.iter().any(|(id, _)| id == "GO:0016082"),
        "synaptic vesicle priming"
    );
    let report = cluster::clusters(
        index,
        &slice,
        &ClusterParams {
            stability_runs: 5,
            ..ClusterParams::default()
        },
    );
    assert_eq!(report.slice_size, slice.members.len());
    assert!(report.clusters.len() >= 3 && report.modularity > 0.3);
    let total: usize = report.clusters.iter().map(|c| c.members.len()).sum();
    assert_eq!(total, slice.members.len());
    let c = report
        .cluster_of(index, "MONDO:0012812")
        .expect("STXBP1 DEE4 clustered");
    assert!(
        c.members.iter().any(|m| m.id == "MONDO:0033372"),
        "CPLX1 DEE63 with STXBP1 DEE4"
    );
    assert!(!c.edges.is_empty() && !c.why.is_empty() && !c.label.is_empty());
    assert!((0.0..=1.0).contains(&c.stability.bootstrap));
    assert_eq!(report.provenance.activity.id, cluster::ACTIVITY_CLUSTERS);
}
