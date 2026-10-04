//! Similarity parity with the Python oracle (`mechanism.py`) under the prototype's settings
//! (Reactome only, Orphanet + G2P effects unioned): phenotype simGIC, most specific shared terms,
//! mechanism score, shared genes, conflicts, most specific shared pathways, within 1e-6.
//! Regenerate: `uv run python crates/atlas-analytics/tests/fixtures/gen_mechanism_parity.py`.
//! The Python oracle has the pre-D30 identity, so the atlas is rebuilt with `LabelPolicy::Merge` (D30.1).
//!
//! One deliberate deviation is asserted too: for a shared gene with opposite effects, the process
//! score leaves that gene out (prototype: 1.0 through the gene's own pathways; here 0.0).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use atlas_analytics::{SimilarityIndex, SimilarityParams};
use atlas_ingest::mechanism;
use serde::Deserialize;

#[derive(Deserialize)]
struct Pair {
    a: String,
    b: String,
    phenotype: f64,
    phenotype_specific: Vec<String>,
    phenotype_specific_ic: BTreeMap<String, f64>,
    mechanism: f64,
    pathway_score: f64,
    shared_genes: Vec<String>,
    effect_conflicts: Vec<String>,
    shared_pathways: Vec<String>,
    genes_a: BTreeMap<String, Vec<String>>,
    genes_b: BTreeMap<String, Vec<String>>,
}

const EPS: f64 = 1e-6;

fn data() -> Option<PathBuf> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let raw = data.join("raw");
    atlas_ingest::sources::SOURCES
        .iter()
        .chain(mechanism::SOURCES.iter())
        .all(|s| raw.join(s.file).exists())
        .then_some(data)
}

#[test]
fn pairs_match_python() {
    let Some(data) = data() else {
        eprintln!("skipped: data/raw incomplete");
        return;
    };
    // the Python oracle has the pre-D30 identity (label merges, D30.1): rebuild that atlas in memory
    let raw = atlas_ingest::raw_dir(&data);
    let atlas = atlas_ingest::build_with(&raw, atlas_core::identity::LabelPolicy::Merge).expect("pre-D30 atlas");
    let m = mechanism::build(&raw, &atlas).expect("mechanism");
    let index = SimilarityIndex::new(Arc::new(atlas), Arc::new(m), SimilarityParams::prototype());
    let pairs: Vec<Pair> = serde_json::from_str(include_str!("fixtures/mechanism_parity.json")).unwrap();
    assert!(pairs.len() >= 4);
    for p in &pairs {
        let a = index.resolve(&p.a).expect("a");
        let b = index.resolve(&p.b).expect("b");
        let name = format!("{} vs {}", p.a, p.b);
        let profile_genes = |d| -> BTreeMap<String, Vec<String>> {
            index
                .profile(d)
                .unwrap()
                .genes
                .iter()
                .map(|g| {
                    (
                        g.symbol.clone(),
                        g.effects.iter().map(|e| e.as_str().to_owned()).collect(),
                    )
                })
                .collect()
        };
        assert_eq!(profile_genes(a), p.genes_a, "{name} genes a");
        assert_eq!(profile_genes(b), p.genes_b, "{name} genes b");

        let ph = index.phenotype(a, b);
        assert!(
            (ph.score - p.phenotype).abs() < EPS,
            "{name} phenotype {} vs {}",
            ph.score,
            p.phenotype
        );
        let atlas = index.atlas();
        let terms: BTreeSet<String> = index
            .specific_shared_terms(a, b)
            .into_iter()
            .map(|t| atlas.hpo.term(t).id.clone())
            .collect();
        assert_eq!(
            terms,
            p.phenotype_specific.iter().cloned().collect(),
            "{name} specific terms"
        );
        for t in &ph.shared {
            assert!(
                (t.ic - p.phenotype_specific_ic[&t.term.id]).abs() < EPS,
                "{name} IC {}",
                t.term.id
            );
        }

        let me = index.mechanism(a, b);
        let genes: Vec<&str> = me
            .shared_genes
            .iter()
            .chain(me.effect_conflicts.iter().map(|c| &c.gene))
            .map(|g| g.symbol.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(genes, p.shared_genes, "{name} shared genes");
        let conflicts: Vec<&str> = me.effect_conflicts.iter().map(|c| c.gene.symbol.as_str()).collect();
        assert_eq!(conflicts, p.effect_conflicts, "{name} conflicts");
        eprintln!(
            "{name}: phenotype {:.9} (py {:.9}) mechanism {:.9} (py {:.9}) process {:.9} (py {:.9})",
            ph.score, p.phenotype, me.score, p.mechanism, me.process_score, p.pathway_score
        );
        if p.effect_conflicts.is_empty() {
            assert!((me.score - p.mechanism).abs() < EPS, "{name} mechanism");
            assert!((me.process_score - p.pathway_score).abs() < EPS, "{name} pathway score");
            let pws: BTreeSet<String> = index.specific_shared_processes(a, b).into_iter().collect();
            assert_eq!(pws, p.shared_pathways.iter().cloned().collect(), "{name} pathways");
        } else {
            // deviation: the conflicting gene's own pathways no longer count as shared mechanism
            assert_eq!(p.pathway_score, 1.0);
            assert_eq!(me.process_score, 0.0, "{name}");
            assert_eq!(me.score, 0.0, "{name}");
            assert!(me.effect_conflicts[0].opposite);
        }
    }
}
