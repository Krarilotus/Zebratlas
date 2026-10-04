//! Mechanism ingest parity with `mechanism.py` / `explore_clusters.py` and provenance checks.
//! Skipped when data/raw is incomplete.

use std::collections::HashSet;
use std::path::PathBuf;

use atlas_ingest::mechanism::{self, Effect, ProcessKind};

fn data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let raw = data.join("raw");
    let all = atlas_ingest::sources::SOURCES
        .iter()
        .chain(mechanism::SOURCES.iter())
        .all(|s| raw.join(s.file).exists());
    all.then_some(data)
}

#[test]
fn counts_match_python_and_provenance_is_complete() {
    let Some(data) = data() else {
        eprintln!("skipped: data/raw incomplete");
        return;
    };
    let (atlas, _) = atlas_ingest::load_or_build(&data, false).expect("atlas");
    let (m, origin) = mechanism::load_or_build(&data, &atlas, false).expect("mechanism");
    eprintln!("mechanism data: {origin:?}");

    // explore_clusters.py: "hgnc 45187 reactome pathways 2883 genes-with-pathways 11788", "g2p diseases 3175"
    assert_eq!(m.hgnc.genes.len(), 45187);
    let named = m.reactome.names.iter().filter(|n| !n.is_empty()).count();
    assert_eq!(named, 2883);
    assert_eq!(m.reactome.by_gene.len(), 11788);
    let g2p_diseases: HashSet<&str> = m
        .g2p
        .iter()
        .flat_map(|r| r.diseases.iter().map(String::as_str))
        .collect();
    assert_eq!(g2p_diseases.len(), 3175);

    // STXBP1: LoF in G2P, ClinGen HI 3, vesicle processes in GO-BP
    let stx: Vec<_> = m.g2p_for_gene("STXBP1").collect();
    assert!(
        stx.iter()
            .any(|r| r.mechanism == Some(Effect::LoF) && r.diseases.contains(&"MONDO:0012812".to_string()))
    );
    assert!(m.clingen("STXBP1").is_some_and(|c| c.haploinsufficiency.sufficient()));
    let go = m.gene_processes(ProcessKind::GoBp, "STXBP1");
    let exo = m.go_bp.get("GO:0016079").expect("synaptic vesicle exocytosis");
    assert!(go.contains(&exo));
    // KCNQ2: LoF (benign neonatal) and GoF (DEE) curations
    let kcnq2: HashSet<Option<Effect>> = m.g2p_for_gene("KCNQ2").map(|r| r.mechanism).collect();
    assert!(kcnq2.contains(&Some(Effect::LoF)) && kcnq2.contains(&Some(Effect::GoF)));

    // every source entity has a checksum; every activity used something and ended
    assert_eq!(m.provenance.entities.len(), mechanism::SOURCES.len());
    assert!(
        m.provenance
            .entities
            .iter()
            .all(|e| e.sha256.as_ref().is_some_and(|h| h.len() == 64))
    );
    assert!(
        m.provenance
            .activities
            .iter()
            .all(|a| !a.used.is_empty() && a.ended_at.is_some())
    );
    let g2p_entity = m.provenance.entity(m.entity_by_file(mechanism::G2P).unwrap());
    assert_eq!(g2p_entity.version.as_deref(), Some("2026-09-28"));
    eprintln!(
        "{:#?}",
        m.provenance
            .activities
            .iter()
            .map(|a| (&a.id, &a.counts))
            .collect::<Vec<_>>()
    );
}
