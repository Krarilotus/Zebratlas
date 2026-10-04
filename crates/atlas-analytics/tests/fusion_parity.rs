//! Live `/api/match` ranking (D30.2) against the frozen ranker-v3 test run
//! `eval/results/runs/20261003T195145Z-45a0acf-ranker-v3/ranks_test.csv`: for 20 test cases
//! (the first 20 by member sha256), every scorer (`atlas`, `resnik`, `fusion`) must give the
//! target the recorded exact mid/best/worst rank and tie size, and the recorded target score.
//!
//! That run scored the pre-D30 atlas (label-only merges), so the reproduction builds the atlas with
//! `LabelPolicy::Merge`. The same cases on the D30 atlas are reported (not asserted): D30.1 changes
//! the candidate set and the IC background, which is why the eval owner reruns a versioned
//! evaluation. Skipped when data/raw or the run files are missing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atlas_analytics::ranking::{self, Scorer};
use atlas_analytics::{Matcher, ScoringParams};
use atlas_core::Atlas;
use atlas_core::identity::LabelPolicy;
use serde_json::Value;

const RUN: &str = "eval/results/runs/20261003T195145Z-45a0acf-ranker-v3";
const SAMPLE: usize = 20;

struct Expected {
    mid: f64,
    best: usize,
    worst: usize,
    tied: usize,
    score: f64,
}

struct Case {
    id: String,
    target: String,
    present: Vec<String>,
    excluded: Vec<String>,
    expected: HashMap<Scorer, Expected>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("list")
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

fn cases(root: &Path) -> Vec<Case> {
    let run = root.join(RUN);
    let mut expected: HashMap<String, HashMap<Scorer, Expected>> = HashMap::new();
    let mut csv = csv::Reader::from_path(run.join("ranks_test.csv")).unwrap();
    let head = csv.headers().unwrap().clone();
    let col = |name: &str| head.iter().position(|h| h == name).unwrap();
    for row in csv.records() {
        let row = row.unwrap();
        let scorer = match &row[col("method")] {
            "atlas scorer" => Scorer::Atlas,
            "resnik (BMA)" => Scorer::Resnik,
            "rrf-w0.25-k1" => Scorer::Fusion,
            other => panic!("unexpected method {other}"),
        };
        let e = Expected {
            mid: row[col("exact_mid")].parse().unwrap(),
            best: row[col("exact_best")].parse().unwrap(),
            worst: row[col("exact_worst")].parse().unwrap(),
            tied: row[col("exact_tied")].parse().unwrap(),
            score: row[col("exact_score")].parse().unwrap(),
        };
        expected
            .entry(row[col("composite_id")].to_owned())
            .or_default()
            .insert(scorer, e);
    }
    let text = std::fs::read_to_string(run.join("derivations.jsonl")).unwrap();
    let mut out: Vec<(String, Case)> = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|r| r["view"] == "test")
        .map(|r| {
            let id = r["composite_id"].as_str().unwrap().to_owned();
            let case = Case {
                expected: expected.remove(&id).expect("ranks for every test case"),
                id,
                target: r["target"].as_str().unwrap().to_owned(),
                present: strings(&r["present"]),
                excluded: strings(&r["excluded"]),
            };
            (r["member_sha256"].as_str().unwrap().to_owned(), case)
        })
        .collect();
    assert_eq!(out.len(), 387);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().take(SAMPLE).map(|(_, c)| c).collect()
}

/// Per case: do all three target ranks match, and do all three scores match too?
struct Outcome {
    ranks_same: usize,
    all_same: usize,
    diffs: Vec<String>,
}

fn compare(atlas: Atlas, cases: &[Case]) -> Outcome {
    let m = Matcher::new(Arc::new(atlas), ScoringParams::default());
    let atlas = m.atlas().clone();
    let mut out = Outcome {
        ranks_same: 0,
        all_same: 0,
        diffs: Vec::new(),
    };
    for c in cases {
        let (present, _) = m.canonical_terms(&c.present);
        let (excluded, _) = m.canonical_terms(&c.excluded);
        assert_eq!(present.len(), c.present.len(), "{}: present terms are canonical", c.id);
        let target = atlas.disease_idx(&c.target);
        let (mut ranks_ok, mut scores_ok) = (true, true);
        for scorer in [Scorer::Atlas, Scorer::Resnik, Scorer::Fusion] {
            let e = &c.expected[&scorer];
            let full = ranking::rank_all(&m, scorer, &present, &excluded).unwrap().unwrap();
            let got = target.and_then(|t| full.of(t));
            let rank_ok =
                got.is_some_and(|(r, _)| r.mid == e.mid && r.best == e.best && r.worst == e.worst && r.tied == e.tied);
            let tol = if scorer == Scorer::Atlas { 1e-6 } else { 1e-12 };
            let score_ok = got.is_some_and(|(_, s)| (s - e.score).abs() <= tol * e.score.abs().max(1.0));
            if !(rank_ok && score_ok) {
                out.diffs.push(format!(
                    "{} {} {} [{}]: got {:?}, run mid {} best {} worst {} tied {} score {}",
                    c.id,
                    c.target,
                    scorer.as_str(),
                    if rank_ok { "score only" } else { "RANK" },
                    got,
                    e.mid,
                    e.best,
                    e.worst,
                    e.tied,
                    e.score
                ));
            }
            ranks_ok &= rank_ok;
            scores_ok &= score_ok;
        }
        out.ranks_same += usize::from(ranks_ok);
        out.all_same += usize::from(ranks_ok && scores_ok);
    }
    out
}

#[test]
fn live_ranking_reproduces_ranker_v3_test_ranks() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let data = root.join("data");
    let complete = atlas_ingest::sources::SOURCES
        .iter()
        .all(|s| data.join("raw").join(s.file).exists());
    if !complete || !root.join(RUN).join("ranks_test.csv").exists() {
        eprintln!("skipped: data/raw or the ranker-v3 run is missing");
        return;
    }
    let cases = cases(&root);
    let raw = atlas_ingest::raw_dir(&data);

    let legacy = atlas_ingest::build_with(&raw, LabelPolicy::Merge).expect("pre-D30 atlas");
    let o = compare(legacy, &cases);
    for d in &o.diffs {
        eprintln!("MISMATCH {d}");
    }
    assert_eq!(
        o.all_same, SAMPLE,
        "pre-D30 atlas: {}/{SAMPLE} cases reproduce the frozen run",
        o.all_same
    );
    eprintln!("pre-D30 atlas: {SAMPLE}/{SAMPLE} test cases reproduce all three frozen target ranks and scores");

    let current = atlas_ingest::build(&raw).expect("D30 atlas");
    let o = compare(current, &cases);
    eprintln!(
        "D30 atlas (informational): {}/{SAMPLE} keep all three target ranks, {}/{SAMPLE} also every score",
        o.ranks_same, o.all_same
    );
    for d in o.diffs.iter().filter(|d| d.contains("[RANK]")) {
        eprintln!("  rank changed: {d}");
    }
}
