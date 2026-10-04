//! Mechanism statements fed into the evidence review: the KCNQ2 G2P models (LoF benign neonatal vs
//! GoF epileptic encephalopathy) and UNC13A (monoallelic GoF vs biallelic LoF) must surface as
//! findings, with every statement sourced. Skipped when data/raw is incomplete.

use std::path::PathBuf;
use std::sync::Arc;

use atlas_analytics::evidence::{Relation, Value};
use atlas_analytics::{SimilarityIndex, SimilarityParams, mechanism_statements};
use atlas_ingest::mechanism;

#[test]
fn kcnq2_and_unc13a_surface_in_the_review() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let raw = data.join("raw");
    if !atlas_ingest::sources::SOURCES
        .iter()
        .chain(mechanism::SOURCES.iter())
        .all(|s| raw.join(s.file).exists())
    {
        eprintln!("skipped: data/raw incomplete");
        return;
    }
    let (atlas, _) = atlas_ingest::load_or_build(&data, false).expect("atlas");
    let (m, _) = mechanism::load_or_build(&data, &atlas, false).expect("mechanism");
    let index = SimilarityIndex::new(Arc::new(atlas), Arc::new(m), SimilarityParams::default());

    let mut ids = vec!["MONDO:0013387".to_owned(), "MONDO:0007365".to_owned()];
    for g in ["G2P:G2P03905", "G2P:G2P03909"] {
        if index.resolve(g).is_some() {
            ids.push(g.to_owned());
        }
    }
    let review = mechanism_statements::review(&index, Some(&ids), 2026).expect("review");
    let g2p: Vec<_> = review
        .statements
        .iter()
        .filter(|s| s.relation == Relation::Mechanism && s.id.starts_with("G2P"))
        .collect();
    let dee = g2p
        .iter()
        .find(|s| s.subject == "MONDO:0013387")
        .expect("KCNQ2 DEE G2P statement");
    assert_eq!(dee.value, Value::GainOfFunction);
    assert!(dee.raw_value.contains("support=inferred"));
    let bfns = g2p
        .iter()
        .find(|s| s.subject == "MONDO:0007365")
        .expect("KCNQ2 BFNS G2P statement");
    assert_eq!(bfns.value, Value::LossOfFunction);
    assert!(review.statements.iter().all(|s| !s.citations.is_empty()));
    assert!(review.statements.iter().any(|s| s.relation == Relation::Pathway));
    // The DEE7 mechanism edge holds the inferred G2P GoF call next to the Orphanet/HPO links that
    // leave the effect open; a literature statement (e.g. dominant-negative LoF, extracted with a
    // quote) passed through `additional` lands on this same edge and is reviewed against it.
    let edge = review
        .edges
        .iter()
        .find(|e| e.subject == dee.subject && e.relation == Relation::Mechanism && e.object == dee.object)
        .expect("KCNQ2 DEE mechanism edge");
    assert!(edge.confidence.statement_ids.contains(&dee.id));
    assert!(
        edge.confidence.statement_ids.len() >= 2,
        "{:?}",
        edge.confidence.statement_ids
    );
    eprintln!(
        "DEE7/KCNQ2 mechanism: {} {:?}",
        edge.confidence.label, edge.confidence.components
    );
    // UNC13A: the two G2P models carry opposite effects on their own condition nodes
    if ids.len() == 4 {
        let v: Vec<&Value> = g2p
            .iter()
            .filter(|s| s.subject.starts_with("G2P:"))
            .map(|s| &s.value)
            .collect();
        assert!(v.contains(&&Value::GainOfFunction) && v.contains(&&Value::LossOfFunction));
    }
}
