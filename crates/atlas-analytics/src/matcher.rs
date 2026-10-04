//! Phenotype-driven disease ranking: one likelihood ratio per patient term (LIRICAL-style).
//!
//! For a patient term q and disease d:
//!   d has q or a descendant      P(q|d) = best annotated frequency          LR = P / p(q)
//!   d has only an ancestor a     P(q|d) = f(a) * p(q)/p(a)                   LR = f(a) / p(a)
//!   nothing recorded             open world: weak evidence against          LR = not_recorded_lr
//!   d is NOT-annotated q or an ancestor of q                                 LR = contradiction_lr
//! For a term n the patient does not have:
//!   d has n or a descendant at frequency f                                   LR = 1 - f
//!   d is NOT-annotated n or an ancestor of n                                 LR = excluded_match_lr
//! p(t) is the background share of diseases with t in their closure, exp(-IC(t)).
//! Score = sum of log LRs; probability = softmax over all diseases with phenotype data (flat prior).
//!
//! Candidates are active diseases with present annotations; retired nodes never rank.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use atlas_core::evidence::PhenotypeAnnotation;
use atlas_core::{Atlas, DiseaseIdx, TermIdx};

use crate::resnik::Resnik;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoringParams {
    /// Annotation without a frequency.
    pub unknown_frequency: f64,
    /// Automatic, unreviewed annotations count less.
    pub iea_weight: f64,
    pub min_frequency: f64,
    /// Missing is weak evidence against, never impossible.
    pub not_recorded_lr: f64,
    pub contradiction_lr: f64,
    /// Patient lacks a feature the disease (almost) always has.
    pub min_absent_lr: f64,
    /// Patient lacks a feature the disease is documented to lack.
    pub excluded_match_lr: f64,
}

impl Default for ScoringParams {
    fn default() -> Self {
        Self {
            unknown_frequency: 0.5,
            iea_weight: 0.7,
            min_frequency: 0.01,
            not_recorded_lr: 0.1,
            contradiction_lr: (-4f64).exp(),
            min_absent_lr: 0.05,
            excluded_match_lr: 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionKind {
    /// Patient has the term: annotated exactly.
    Exact,
    ViaDescendant,
    ViaAncestor,
    NotRecorded,
    /// Patient has the term, disease is NOT-annotated with it or an ancestor.
    ExcludedConflict,
    /// Patient lacks the term, disease has it.
    AbsentExpected,
    /// Patient lacks the term, disease is documented to lack it.
    ExcludedOk,
}

impl ContributionKind {
    /// Evidence comes from the disease's NOT annotations.
    pub fn from_excluded(self) -> bool {
        matches!(self, Self::ExcludedConflict | Self::ExcludedOk)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contribution {
    pub query_term: TermIdx,
    pub matched_term: Option<TermIdx>,
    pub kind: ContributionKind,
    pub log_lr: f64,
    pub frequency: Option<f64>,
}

impl Contribution {
    /// Annotation rows behind the matched term (NOT rows for excluded kinds).
    pub fn evidence<'a>(&self, atlas: &'a Atlas, disease: DiseaseIdx) -> &'a [PhenotypeAnnotation] {
        let Some(t) = self.matched_term else { return &[] };
        let d = atlas.disease_at(disease);
        let edge = if self.kind.from_excluded() {
            d.excluded_phenotype(t)
        } else {
            d.phenotype(t)
        };
        edge.map_or(&[], |e| e.annotations.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub disease: DiseaseIdx,
    pub score: f64,
    pub probability: f64,
    pub contributions: Vec<Contribution>,
}

pub fn annotation_frequency(a: &PhenotypeAnnotation, params: &ScoringParams) -> f64 {
    let mut value = match &a.frequency {
        // Beta(1,1) shrinkage: 1/1 is not 100%
        Some(f) if f.cohort.is_some() => {
            let (n, m) = f.cohort.unwrap();
            (n + 1) as f64 / (m + 2) as f64
        }
        Some(f) if f.value.is_some() => f.value.unwrap(),
        _ => params.unknown_frequency,
    };
    if a.evidence == "IEA" {
        value *= params.iea_weight;
    }
    value.max(params.min_frequency)
}

const NONE: u32 = u32::MAX;

pub struct Matcher {
    atlas: Arc<Atlas>,
    pub params: ScoringParams,
    /// Ranking candidates in disease order (the Python `freq` dict order).
    candidates: Vec<DiseaseIdx>,
    /// Disease -> position in `candidates`, or NONE.
    slot: Vec<u32>,
    /// Per candidate: (term, best annotation frequency), sorted by term.
    freq: Vec<Vec<(TermIdx, f64)>>,
    /// Term -> candidates NOT-annotated with it.
    by_excluded: Vec<Vec<DiseaseIdx>>,
    resnik: OnceLock<Resnik>,
}

impl Matcher {
    pub fn new(atlas: Arc<Atlas>, params: ScoringParams) -> Self {
        let mut candidates = Vec::new();
        let mut slot = vec![NONE; atlas.diseases().len()];
        let mut freq = Vec::new();
        let mut by_excluded = vec![Vec::new(); atlas.hpo.len()];
        for (i, d) in atlas.active().filter(|(_, d)| !d.phenotypes.is_empty()) {
            slot[i as usize] = candidates.len() as u32;
            candidates.push(i);
            let mut f: Vec<(TermIdx, f64)> = d
                .phenotypes
                .iter()
                .map(|e| {
                    let best = e
                        .annotations
                        .iter()
                        .map(|a| annotation_frequency(a, &params))
                        .fold(f64::MIN, f64::max);
                    (e.term, best)
                })
                .collect();
            f.sort_unstable_by_key(|(t, _)| *t);
            freq.push(f);
            // only candidates: a disease with only NOT annotations cannot rank (Python 88f216d)
            for e in &d.excluded {
                by_excluded[e.term as usize].push(i);
            }
        }
        Self {
            atlas,
            params,
            candidates,
            slot,
            freq,
            by_excluded,
            resnik: OnceLock::new(),
        }
    }

    pub fn atlas(&self) -> &Arc<Atlas> {
        &self.atlas
    }

    fn freq(&self, disease: DiseaseIdx, term: TermIdx) -> f64 {
        let f = &self.freq[self.slot[disease as usize] as usize];
        f[f.binary_search_by_key(&term, |(t, _)| *t)
            .expect("by_phenotype entry without frequency")]
        .1
    }

    /// (usable HPO terms, unknown or non-phenotype ids as given), duplicates removed.
    pub fn canonical_terms<S: AsRef<str>>(&self, ids: &[S]) -> (Vec<TermIdx>, Vec<String>) {
        let hpo = &self.atlas.hpo;
        let mut usable = Vec::new();
        let mut unknown = Vec::new();
        for raw in ids {
            match hpo.canonical(raw.as_ref().trim()) {
                Some(t) if hpo.is_phenotype(t) => {
                    if !usable.contains(&t) {
                        usable.push(t);
                    }
                }
                _ => unknown.push(raw.as_ref().to_owned()),
            }
        }
        (usable, unknown)
    }

    fn present(&self, q: TermIdx) -> HashMap<DiseaseIdx, Contribution> {
        let (atlas, hpo, p) = (&*self.atlas, &self.atlas.hpo, &self.params);
        let mut out = HashMap::new();
        let mut best: HashMap<DiseaseIdx, (f64, TermIdx)> = HashMap::new();
        for &t in hpo.descendants(q) {
            for &did in atlas.by_phenotype(t) {
                let f = self.freq(did, t);
                // ties: the query term itself, else the lowest index (Python: set order, varies per run)
                if best.get(&did).is_none_or(|&(bf, _)| f > bf || (f == bf && t == q)) {
                    best.insert(did, (f, t));
                }
            }
        }
        let bg_q = hpo.background(q);
        for (&did, &(f, t)) in &best {
            let kind = if t == q {
                ContributionKind::Exact
            } else {
                ContributionKind::ViaDescendant
            };
            let c = Contribution {
                query_term: q,
                matched_term: Some(t),
                kind,
                log_lr: (f / bg_q).ln(),
                frequency: Some(f),
            };
            out.insert(did, c);
        }

        let floor = p.not_recorded_lr.ln();
        for &a in hpo.ancestors(q).iter().filter(|&&a| a != q) {
            let bg_a = hpo.background(a);
            for &did in atlas.by_phenotype(a) {
                if best.contains_key(&did) {
                    continue;
                }
                let f = self.freq(did, a);
                let log_lr = (f / bg_a).ln();
                if log_lr > floor && out.get(&did).is_none_or(|c: &Contribution| log_lr > c.log_lr) {
                    let kind = ContributionKind::ViaAncestor;
                    out.insert(
                        did,
                        Contribution {
                            query_term: q,
                            matched_term: Some(a),
                            kind,
                            log_lr,
                            frequency: Some(f),
                        },
                    );
                }
            }
        }

        for &a in hpo.ancestors(q) {
            for &did in &self.by_excluded[a as usize] {
                let kind = ContributionKind::ExcludedConflict;
                let log_lr = p.contradiction_lr.ln();
                out.insert(
                    did,
                    Contribution {
                        query_term: q,
                        matched_term: Some(a),
                        kind,
                        log_lr,
                        frequency: None,
                    },
                );
            }
        }
        out
    }

    fn absent(&self, n: TermIdx) -> HashMap<DiseaseIdx, Contribution> {
        let (atlas, hpo, p) = (&*self.atlas, &self.atlas.hpo, &self.params);
        let mut out: HashMap<DiseaseIdx, Contribution> = HashMap::new();
        for &t in hpo.descendants(n) {
            for &did in atlas.by_phenotype(t) {
                let f = self.freq(did, t);
                let log_lr = (1.0 - f).max(p.min_absent_lr).ln();
                if out.get(&did).is_none_or(|c| log_lr < c.log_lr) {
                    let kind = ContributionKind::AbsentExpected;
                    out.insert(
                        did,
                        Contribution {
                            query_term: n,
                            matched_term: Some(t),
                            kind,
                            log_lr,
                            frequency: Some(f),
                        },
                    );
                }
            }
        }
        for &a in hpo.ancestors(n) {
            for &did in &self.by_excluded[a as usize] {
                out.entry(did).or_insert(Contribution {
                    query_term: n,
                    matched_term: Some(a),
                    kind: ContributionKind::ExcludedOk,
                    log_lr: p.excluded_match_lr.ln(),
                    frequency: None,
                });
            }
        }
        out
    }

    /// Rank by HPO ids (alt/obsolete ids resolved, unknown ids ignored).
    pub fn rank<S: AsRef<str>>(&self, present: &[S], excluded: &[S], top: usize) -> Vec<Match> {
        let (present, _) = self.canonical_terms(present);
        let (excluded, _) = self.canonical_terms(excluded);
        self.rank_terms(&present, &excluded, top)
    }

    /// Ranking candidates (active diseases with present annotations), in score-vector order.
    pub fn candidates(&self) -> &[DiseaseIdx] {
        &self.candidates
    }

    /// The Resnik baseline over the same candidates (built on first use).
    pub fn resnik(&self) -> &Resnik {
        self.resnik.get_or_init(|| Resnik::new(&self.atlas, &self.candidates))
    }

    /// Full score vector over [`Matcher::candidates`]; `None` when there is nothing to score.
    pub fn score_terms(&self, present: &[TermIdx], excluded: &[TermIdx]) -> Option<Scored<'_>> {
        let excluded: Vec<TermIdx> = excluded.iter().copied().filter(|n| !present.contains(n)).collect();
        if (present.is_empty() && excluded.is_empty()) || self.candidates.is_empty() {
            return None;
        }
        let floor = self.params.not_recorded_lr.ln();
        let per_term: Vec<_> = present.iter().map(|&q| self.present(q)).collect();
        let per_absent: Vec<_> = excluded.iter().map(|&n| self.absent(n)).collect();
        let base = floor * present.len() as f64;
        let mut scores = vec![base; self.candidates.len()];
        for contribs in &per_term {
            for (&did, c) in contribs {
                scores[self.slot[did as usize] as usize] += c.log_lr - floor;
            }
        }
        for contribs in &per_absent {
            for (&did, c) in contribs {
                scores[self.slot[did as usize] as usize] += c.log_lr;
            }
        }
        let peak = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let norm = python_fsum(scores.iter().map(|s| (s - peak).exp())).ln() + peak;
        Some(Scored {
            matcher: self,
            present: present.to_vec(),
            floor,
            per_term,
            per_absent,
            scores,
            norm,
        })
    }

    /// Rank by canonical terms; an excluded term that is also present is ignored.
    pub fn rank_terms(&self, present: &[TermIdx], excluded: &[TermIdx], top: usize) -> Vec<Match> {
        let Some(scored) = self.score_terms(present, excluded) else {
            return Vec::new();
        };
        let scores = &scored.scores;
        let mut order: Vec<usize> = (0..scores.len()).collect();
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a])); // stable, like Python sorted(reverse=True)
        order.truncate(top);
        order.into_iter().map(|s| scored.explain(s)).collect()
    }
}

/// One query scored over every candidate, with what is needed to explain any of them.
pub struct Scored<'a> {
    matcher: &'a Matcher,
    present: Vec<TermIdx>,
    floor: f64,
    per_term: Vec<HashMap<DiseaseIdx, Contribution>>,
    per_absent: Vec<HashMap<DiseaseIdx, Contribution>>,
    /// Log-LR score per candidate slot.
    pub scores: Vec<f64>,
    /// log of the softmax denominator.
    norm: f64,
}

impl Scored<'_> {
    /// The disease at candidate slot `s`.
    pub fn disease(&self, s: usize) -> DiseaseIdx {
        self.matcher.candidates[s]
    }

    /// The match (score, probability, contributions) of candidate slot `s`.
    pub fn explain(&self, s: usize) -> Match {
        let did = self.matcher.candidates[s];
        let mut contributions: Vec<Contribution> = self
            .present
            .iter()
            .zip(&self.per_term)
            .map(|(&q, contribs)| {
                contribs.get(&did).cloned().unwrap_or(Contribution {
                    query_term: q,
                    matched_term: None,
                    kind: ContributionKind::NotRecorded,
                    log_lr: self.floor,
                    frequency: None,
                })
            })
            .collect();
        contributions.extend(self.per_absent.iter().filter_map(|c| c.get(&did).cloned()));
        Match {
            disease: did,
            score: self.scores[s],
            probability: (self.scores[s] - self.norm).exp(),
            contributions,
        }
    }
}

/// Python >= 3.12 `sum()` over floats: Neumaier-compensated, so the softmax norm matches the reference.
pub(crate) fn python_fsum(values: impl Iterator<Item = f64>) -> f64 {
    let (mut sum, mut c) = (0.0f64, 0.0f64);
    for x in values {
        let t = sum + x;
        if sum.abs() >= x.abs() {
            c += (sum - t) + x;
        } else {
            c += (x - t) + sum;
        }
        sum = t;
    }
    if c != 0.0 && c.is_finite() { sum + c } else { sum }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::evidence::Frequency;
    use atlas_core::provenance::{EntityIdx, RecordRef};

    fn ann(freq: Option<Frequency>, evidence: &str) -> PhenotypeAnnotation {
        PhenotypeAnnotation {
            disease_id: "OMIM:1".into(),
            disease_name: "x".into(),
            hpo_id: "HP:1".into(),
            negated: false,
            references: vec![],
            evidence: evidence.into(),
            onset: None,
            frequency: freq,
            sex: None,
            modifiers: vec![],
            aspect: "P".into(),
            biocuration: String::new(),
            record: RecordRef::line(EntityIdx(0), 1),
        }
    }

    #[test]
    fn frequency_shrinkage_and_weights() {
        let p = ScoringParams::default();
        assert_eq!(
            annotation_frequency(&ann(Frequency::parse("1/1"), "PCS"), &p),
            2.0 / 3.0
        );
        assert_eq!(annotation_frequency(&ann(None, "PCS"), &p), p.unknown_frequency);
        assert!((annotation_frequency(&ann(None, "IEA"), &p) - p.unknown_frequency * p.iea_weight).abs() < 1e-15);
        assert_eq!(
            annotation_frequency(&ann(Frequency::parse("HP:0040285"), "PCS"), &p),
            p.min_frequency
        );
    }

    #[test]
    fn compensated_sum() {
        assert_eq!(python_fsum([1e16, 1.0, -1e16].into_iter()), 1.0);
    }
}
