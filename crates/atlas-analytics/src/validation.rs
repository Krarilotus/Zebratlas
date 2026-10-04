//! Held-out validation of the clusters: partitions built *without* one signal are compared with
//! groupings derived *only* from that signal. Nothing here feeds back into parameters.
//!
//! Held-out groupings per disease (diseases without one are left out of that comparison):
//! - `g2p_mechanism`: effect class of its established G2P curations (LoF / GoF / DN / mixed);
//! - `go_slim`: the most specific GO-slim (goslim_generic) BP term in its genes' GO closure;
//! - `reactome_top`: the most specific top-level Reactome pathway in its genes' closure.
//!
//! Metrics: adjusted Rand index (Hubert & Arabie) and adjusted mutual information (Vinh et al.
//! 2010, arithmetic normalisation as scikit-learn's default), both 0 in expectation for chance.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use atlas_core::DiseaseIdx;
use atlas_core::mechanism::{Effect, ProcessKind};
use serde::Serialize;

use crate::cluster::{ClusterParams, PairMatrix, partition};
use crate::similarity::SimilarityIndex;

fn contingency(a: &[usize], b: &[usize]) -> (Vec<Vec<u64>>, Vec<u64>, Vec<u64>) {
    let ra = crate::cluster::canonical_labels(a);
    let rb = crate::cluster::canonical_labels(b);
    let na = ra.iter().max().map_or(0, |m| m + 1);
    let nb = rb.iter().max().map_or(0, |m| m + 1);
    let mut t = vec![vec![0u64; nb]; na];
    for (x, y) in ra.iter().zip(&rb) {
        t[*x][*y] += 1;
    }
    let rows = t.iter().map(|r| r.iter().sum()).collect();
    let cols = (0..nb).map(|j| t.iter().map(|r| r[j]).sum()).collect();
    (t, rows, cols)
}

fn comb2(n: u64) -> f64 {
    (n as f64) * (n as f64 - 1.0) / 2.0
}

/// Adjusted Rand index.
pub fn ari(a: &[usize], b: &[usize]) -> f64 {
    let n = a.len() as u64;
    if n < 2 {
        return 1.0;
    }
    let (t, rows, cols) = contingency(a, b);
    let index: f64 = t.iter().flatten().map(|&x| comb2(x)).sum();
    let sa: f64 = rows.iter().map(|&x| comb2(x)).sum();
    let sb: f64 = cols.iter().map(|&x| comb2(x)).sum();
    let expected = sa * sb / comb2(n);
    let max = (sa + sb) / 2.0;
    if (max - expected).abs() < 1e-12 {
        1.0
    } else {
        (index - expected) / (max - expected)
    }
}

fn entropy(counts: &[u64], n: f64) -> f64 {
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.ln()
        })
        .sum()
}

/// Adjusted mutual information (arithmetic mean normalisation).
pub fn ami(a: &[usize], b: &[usize]) -> f64 {
    let n = a.len();
    if n < 2 {
        return 1.0;
    }
    let (t, rows, cols) = contingency(a, b);
    if rows.len() == 1 && cols.len() == 1 || rows.len() == n && cols.len() == n {
        return 1.0;
    }
    let nf = n as f64;
    let mut mi = 0.0;
    for (i, r) in t.iter().enumerate() {
        for (j, &x) in r.iter().enumerate() {
            if x > 0 {
                let x = x as f64;
                mi += x / nf * ((nf * x) / (rows[i] as f64 * cols[j] as f64)).ln();
            }
        }
    }
    // expected MI under the hypergeometric model
    let lnf: Vec<f64> = (0..=n)
        .scan(0.0, |s, k| {
            if k > 0 {
                *s += (k as f64).ln();
            }
            Some(*s)
        })
        .collect();
    let mut emi = 0.0;
    for &ai in &rows {
        for &bj in &cols {
            let lo = (ai + bj).saturating_sub(n as u64).max(1);
            let hi = ai.min(bj);
            for nij in lo..=hi {
                let x = nij as f64;
                let term = x / nf * ((nf * x) / (ai as f64 * bj as f64)).ln();
                let lnp = lnf[ai as usize] + lnf[bj as usize] + lnf[n - ai as usize] + lnf[n - bj as usize]
                    - lnf[n]
                    - lnf[nij as usize]
                    - lnf[(ai - nij) as usize]
                    - lnf[(bj - nij) as usize]
                    - lnf[n + nij as usize - ai as usize - bj as usize];
                emi += term * lnp.exp();
            }
        }
    }
    let (ha, hb) = (entropy(&rows, nf), entropy(&cols, nf));
    let denom = (ha + hb) / 2.0 - emi;
    if denom.abs() < 1e-12 { 0.0 } else { (mi - emi) / denom }
}

/// Held-out grouping labels for a disease.
pub fn g2p_mechanism(index: &SimilarityIndex, d: DiseaseIdx) -> Option<String> {
    let m = index.mechanism_data();
    let id = &index.atlas().disease_at(d).id;
    let effects: BTreeSet<Effect> = m
        .g2p_for_disease(id)
        .filter(|r| r.established())
        .filter_map(|r| r.mechanism)
        .collect();
    match effects.len() {
        0 => None,
        1 => effects.first().map(|e| e.as_str().to_owned()),
        _ => Some("mixed".into()),
    }
}

/// Most specific (highest IC) term of `terms` in the disease's genes' closure of `kind`.
fn most_specific(index: &SimilarityIndex, d: DiseaseIdx, kind: ProcessKind, terms: &BTreeSet<u32>) -> Option<String> {
    let m = index.mechanism_data();
    let o = m.processes(kind);
    let profile = index.profile(d)?;
    let mut best: Option<(f64, u32)> = None;
    for g in &profile.genes {
        for p in m.gene_processes(kind, &g.symbol) {
            if terms.contains(&p) {
                let ic = o.ic(p);
                if best.is_none_or(|(b, bp)| ic > b || (ic == b && p < bp)) {
                    best = Some((ic, p));
                }
            }
        }
    }
    best.map(|(_, p)| format!("{} {}", o.id(p), o.name(p)))
}

pub fn go_slim(index: &SimilarityIndex, d: DiseaseIdx) -> Option<String> {
    let go = &index.mechanism_data().go_bp;
    let slim: BTreeSet<u32> = go
        .subsets
        .get("goslim_generic")
        .into_iter()
        .flatten()
        .copied()
        .collect();
    most_specific(index, d, ProcessKind::GoBp, &slim)
}

pub fn reactome_top(index: &SimilarityIndex, d: DiseaseIdx) -> Option<String> {
    let r = &index.mechanism_data().reactome;
    let top: BTreeSet<u32> = (0..r.len() as u32)
        .filter(|&p| r.parents[p as usize].is_empty() && !r.names[p as usize].is_empty())
        .collect();
    most_specific(index, d, ProcessKind::Reactome, &top)
}

#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    pub run: String,
    pub signals: String,
    pub held_out: String,
    pub labelled: usize,
    pub groups: usize,
    pub clusters: usize,
    pub ami: f64,
    pub ari: f64,
    /// AMI / ARI of the same run against the grouping with cluster labels shuffled (mean of 200).
    pub shuffled_ami: f64,
    pub shuffled_ari: f64,
    pub modularity: f64,
    /// Mean ARI between the run's partition and runs with 20 other Louvain seeds.
    pub seed_ari: f64,
}

/// A held-out grouping: name, label per disease, runs it must not be compared with (they used it).
pub type HeldOut<'a> = (&'a str, &'a dyn Fn(DiseaseIdx) -> Option<String>, &'a [&'a str]);

/// One clustering configuration over a fixed member list.
pub struct Run<'a> {
    pub name: &'a str,
    pub signals: &'a str,
    pub index: &'a SimilarityIndex,
    pub alpha: f64,
}

/// Compare each run's partition with each held-out grouping it did not use.
pub fn validate(
    members: &[DiseaseIdx],
    runs: &[Run<'_>],
    held_out: &[HeldOut<'_>],
    params: &ClusterParams,
) -> Vec<Comparison> {
    let mut out = Vec::new();
    for run in runs {
        let pm = PairMatrix::new(run.index, members);
        let n = pm.len();
        let w = pm.combined(run.alpha);
        let (comm, q) = partition(&w, n, params.k, params.resolution, params.seed);
        let seed_ari = (0..20)
            .map(|s| ari(&comm, &partition(&w, n, params.k, params.resolution, 5000 + s).0))
            .sum::<f64>()
            / 20.0;
        for (name, label, excluded_for) in held_out {
            if excluded_for.contains(&run.name) {
                continue;
            }
            let mut ids: HashMap<String, usize> = HashMap::new();
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for (i, &d) in members.iter().enumerate() {
                if let Some(l) = label(d) {
                    let next = ids.len();
                    b.push(*ids.entry(l).or_insert(next));
                    a.push(comm[i]);
                }
            }
            let mut rng = crate::similarity::SplitMix(99);
            let (mut sa, mut sr) = (0.0, 0.0);
            for _ in 0..200 {
                let mut s = a.clone();
                rng.shuffle(&mut s);
                sa += ami(&s, &b);
                sr += ari(&s, &b);
            }
            out.push(Comparison {
                run: run.name.to_owned(),
                signals: run.signals.to_owned(),
                held_out: (*name).to_owned(),
                labelled: a.len(),
                groups: ids.len(),
                clusters: a.iter().collect::<BTreeSet<_>>().len(),
                ami: ami(&a, &b),
                ari: ari(&a, &b),
                shuffled_ami: sa / 200.0,
                shuffled_ari: sr / 200.0,
                modularity: q,
                seed_ari,
            });
        }
    }
    out
}

/// Distribution of a held-out grouping over the members (for the report).
pub fn distribution(members: &[DiseaseIdx], label: &dyn Fn(DiseaseIdx) -> Option<String>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for &d in members {
        *out.entry(label(d).unwrap_or_else(|| "(none)".into())).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_and_independent_partitions() {
        let a = [0, 0, 0, 1, 1, 1, 2, 2, 2];
        let b = [5, 5, 5, 7, 7, 7, 1, 1, 1];
        assert!((ari(&a, &b) - 1.0).abs() < 1e-12);
        assert!((ami(&a, &b) - 1.0).abs() < 1e-9);
        // sklearn: adjusted_rand_score([0,0,1,1],[0,1,0,1]) = -0.5; AMI = -0.5 (arithmetic)
        let (x, y) = ([0, 0, 1, 1], [0, 1, 0, 1]);
        assert!((ari(&x, &y) + 0.5).abs() < 1e-12);
        assert!((ami(&x, &y) + 0.5).abs() < 1e-9, "{}", ami(&x, &y));
    }

    #[test]
    fn ami_matches_sklearn_example() {
        // sklearn.metrics.adjusted_mutual_info_score([0,0,0,1,1,1],[0,0,1,1,2,2]) = 0.2987924581708901
        let v = ami(&[0, 0, 0, 1, 1, 1], &[0, 0, 1, 1, 2, 2]);
        assert!((v - 0.2987924581708901).abs() < 1e-9, "{v}");
        // adjusted_rand_score = 0.24242424242424246
        let r = ari(&[0, 0, 0, 1, 1, 1], &[0, 0, 1, 1, 2, 2]);
        assert!((r - 0.24242424242424246).abs() < 1e-12, "{r}");
    }
}

/// Area under the ROC curve (Mann-Whitney, ties count half).
pub fn auroc(scores: &[f64], labels: &[bool]) -> f64 {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    let mut ranks = vec![0.0; scores.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && scores[idx[j + 1]] == scores[idx[i]] {
            j += 1;
        }
        let r = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            ranks[idx[k]] = r;
        }
        i = j + 1;
    }
    let pos = labels.iter().filter(|&&l| l).count() as f64;
    let neg = labels.len() as f64 - pos;
    if pos == 0.0 || neg == 0.0 {
        return f64::NAN;
    }
    let sum: f64 = ranks.iter().zip(labels).filter(|(_, l)| **l).map(|(r, _)| r).sum();
    (sum - pos * (pos + 1.0) / 2.0) / (pos * neg)
}

/// Average precision (scikit-learn's definition: steps at distinct score thresholds).
pub fn average_precision(scores: &[f64], labels: &[bool]) -> f64 {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let pos = labels.iter().filter(|&&l| l).count() as f64;
    if pos == 0.0 {
        return f64::NAN;
    }
    let (mut tp, mut fp, mut ap, mut last_recall) = (0.0, 0.0, 0.0, 0.0);
    let mut i = 0;
    while i < idx.len() {
        let s = scores[idx[i]];
        while i < idx.len() && scores[idx[i]] == s {
            if labels[idx[i]] {
                tp += 1.0;
            } else {
                fp += 1.0;
            }
            i += 1;
        }
        let recall = tp / pos;
        ap += (recall - last_recall) * tp / (tp + fp);
        last_recall = recall;
    }
    ap
}

#[derive(Clone, Debug, Serialize)]
pub struct Pairwise {
    pub score: String,
    pub pairs: String,
    pub n: usize,
    pub positives: usize,
    pub prevalence: f64,
    pub auroc: f64,
    pub auprc: f64,
}

/// Does similarity predict "same G2P mechanism class" for disease pairs? Undetermined = NA (left
/// out). `index` must not use effects (EffectRule::Ignore), so the label is held out.
pub fn pairwise_same_mechanism(index: &SimilarityIndex, members: &[DiseaseIdx], alpha: f64) -> Vec<Pairwise> {
    let labelled: Vec<(DiseaseIdx, String)> = members
        .iter()
        .filter_map(|&d| g2p_mechanism(index, d).filter(|l| l != "mixed").map(|l| (d, l)))
        .collect();
    let mut out = Vec::new();
    for (pairs, skip_shared_gene) in [("all labelled pairs", false), ("pairs without a shared gene", true)] {
        let (mut ph, mut pr, mut comb, mut y) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, (a, la)) in labelled.iter().enumerate() {
            for (b, lb) in &labelled[i + 1..] {
                let s = index.score(*a, *b);
                if skip_shared_gene && s.compatible_gene {
                    continue;
                }
                ph.push(s.phenotype);
                pr.push(s.process);
                comb.push(s.combined(alpha));
                y.push(la == lb);
            }
        }
        let positives = y.iter().filter(|&&l| l).count();
        for (name, v) in [("phenotype", &ph), ("process", &pr), ("combined", &comb)] {
            out.push(Pairwise {
                score: name.to_owned(),
                pairs: pairs.to_owned(),
                n: y.len(),
                positives,
                prevalence: positives as f64 / y.len().max(1) as f64,
                auroc: auroc(v, &y),
                auprc: average_precision(v, &y),
            });
        }
    }
    out
}

#[cfg(test)]
mod curve_tests {
    use super::*;

    #[test]
    fn auroc_and_ap_match_sklearn() {
        // sklearn: roc_auc_score([0,0,1,1],[0.1,0.4,0.35,0.8]) = 0.75; average_precision_score = 0.8333333
        let s = [0.1, 0.4, 0.35, 0.8];
        let y = [false, false, true, true];
        assert!((auroc(&s, &y) - 0.75).abs() < 1e-12);
        assert!((average_precision(&s, &y) - 0.8333333333333333).abs() < 1e-12);
    }
}
