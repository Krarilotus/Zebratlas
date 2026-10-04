//! Matcher parity with the Python reference: for fixed queries, the top-5 disease ids, scores,
//! probabilities and contribution kinds/log LRs match `match.py` within 1e-6.
//! Regenerate: `uv run python crates/atlas-analytics/tests/fixtures/gen_match_parity.py`.
//! The Python reference has the pre-D30 identity, so the atlas is rebuilt with `LabelPolicy::Merge`
//! (D30.1 keeps label-only matches as candidates; the live ranking is checked in fusion_parity.rs).
//! Skipped when data/raw is missing.

use std::path::PathBuf;
use std::sync::Arc;

use atlas_analytics::{Matcher, ScoringParams};
use serde::Deserialize;

#[derive(Deserialize)]
struct Query {
    name: String,
    present: Vec<String>,
    excluded: Vec<String>,
    canonical_present: Vec<String>,
    canonical_excluded: Vec<String>,
    unknown: Vec<String>,
    top: Vec<Expected>,
}

#[derive(Deserialize)]
struct Expected {
    disease_id: String,
    score: f64,
    probability: f64,
    contributions: Vec<ExpectedContribution>,
}

#[derive(Deserialize)]
struct ExpectedContribution {
    query_term: String,
    kind: String,
    log_lr: f64,
}

const EPS: f64 = 1e-6;

#[test]
fn top5_matches_python() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    if !atlas_ingest::sources::SOURCES
        .iter()
        .all(|s| data.join("raw").join(s.file).exists())
    {
        eprintln!("skipped: data/raw incomplete");
        return;
    }
    // the Python oracle has the pre-D30 identity (label merges, D30.1): rebuild that atlas in memory
    let raw = atlas_ingest::raw_dir(&data);
    let atlas = atlas_ingest::build_with(&raw, atlas_core::identity::LabelPolicy::Merge).expect("pre-D30 atlas");
    let m = Matcher::new(Arc::new(atlas), ScoringParams::default());
    let atlas = m.atlas().clone();
    let queries: Vec<Query> = serde_json::from_str(include_str!("fixtures/match_parity.json")).unwrap();
    for q in queries {
        let (present, unknown_p) = m.canonical_terms(&q.present);
        let (excluded, unknown_e) = m.canonical_terms(&q.excluded);
        let ids = |ts: &[u32]| ts.iter().map(|&t| atlas.hpo.term(t).id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&present), q.canonical_present, "{}", q.name);
        assert_eq!(ids(&excluded), q.canonical_excluded, "{}", q.name);
        assert_eq!([unknown_p, unknown_e].concat(), q.unknown, "{}", q.name);

        let got = m.rank_terms(&present, &excluded, q.top.len());
        assert_eq!(got.len(), q.top.len(), "{}", q.name);
        for (rank, (g, e)) in got.iter().zip(&q.top).enumerate() {
            let id = &atlas.disease_at(g.disease).id;
            eprintln!(
                "{} #{rank}: {id} {:.9} (python {} {:.9})",
                q.name, g.score, e.disease_id, e.score
            );
            assert_eq!(id, &e.disease_id, "{} rank {rank}", q.name);
            assert!(
                (g.score - e.score).abs() < EPS,
                "{} {id} score {} vs {}",
                q.name,
                g.score,
                e.score
            );
            assert!(
                (g.probability - e.probability).abs() < EPS,
                "{} {id} probability",
                q.name
            );
            assert_eq!(g.contributions.len(), e.contributions.len(), "{} {id}", q.name);
            for (gc, ec) in g.contributions.iter().zip(&e.contributions) {
                let kind = serde_json::to_value(gc.kind).unwrap();
                assert_eq!(atlas.hpo.term(gc.query_term).id, ec.query_term);
                // Python breaks frequency ties between q and a descendant by set order (varies per
                // run); both give the same LR, so exact and via_descendant are one class here.
                let class = |k: &str| if k == "via_descendant" { "exact" } else { k }.to_owned();
                assert_eq!(class(kind.as_str().unwrap()), class(&ec.kind), "{} {id}", q.name);
                assert!((gc.log_lr - ec.log_lr).abs() < EPS, "{} {id} log_lr", q.name);
            }
        }
    }
}
