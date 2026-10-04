//! The live phenotype ranking (`/api/match`, D30.2): one of three scorers over the same candidates.
//!
//! - `fusion` (default): ranker-v3 rank fusion of the atlas scorer and the Resnik baseline with the
//!   preregistered parameters ([`ranker_v3::SELECTED`]).
//! - `atlas`: the atlas likelihood-ratio scorer ([`Matcher`]), as before D30.2.
//! - `resnik`: the evaluation's Resnik baseline ([`crate::resnik`]).
//!
//! Every scorer ranks with RANK-v2: full depth, descending single-linkage ties at absolute
//! tolerance 1e-9, tied candidates share their mid-rank (never broken by id or input order). The
//! listing order inside a tie is disease-id order and carries no meaning.

use serde::{Deserialize, Serialize};

use crate::matcher::{Match, Matcher, Scored};
use crate::ranker_v3::{self, FusedScore, FusionError, TIE_TOLERANCE};
use atlas_core::{DiseaseIdx, TermIdx};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scorer {
    #[default]
    Fusion,
    Atlas,
    Resnik,
}

impl Scorer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fusion => "fusion",
            Self::Atlas => "atlas",
            Self::Resnik => "resnik",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fusion" => Some(Self::Fusion),
            "atlas" => Some(Self::Atlas),
            "resnik" => Some(Self::Resnik),
            _ => None,
        }
    }

    /// Version of the scoring method (changes when the method or its parameters change).
    pub fn version(self) -> &'static str {
        match self {
            Self::Fusion => "ranker-v3-fusion-w0.25-k1",
            Self::Atlas => "atlas-lr-v1",
            Self::Resnik => "resnik-onesided-v1",
        }
    }
}

/// Mid-rank of a candidate in its tie group (RANK-v2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rank {
    pub mid: f64,
    pub best: usize,
    pub worst: usize,
    pub tied: usize,
}

/// A component of the fused score, kept to explain it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Component {
    pub score: f64,
    pub midrank: f64,
}

#[derive(Clone, Debug)]
pub struct Ranked {
    pub disease: DiseaseIdx,
    /// The chosen scorer's score (uncalibrated for fusion and resnik).
    pub score: f64,
    pub rank: Rank,
    pub atlas: Component,
    pub resnik: Component,
    /// The atlas scorer's explanation (its score, probability and per-term contributions).
    pub explanation: Match,
}

/// One query ranked over every candidate by one scorer.
pub struct FullRanking<'a> {
    pub scorer: Scorer,
    pub scored: Scored<'a>,
    pub fused: Vec<FusedScore>,
    /// The chosen scorer's score per candidate slot.
    pub scores: Vec<f64>,
    /// Every slot with its rank, best first.
    pub order: Vec<(usize, Rank)>,
}

/// Score and rank every candidate; `None` when there is nothing to score.
pub fn rank_all<'a>(
    m: &'a Matcher,
    scorer: Scorer,
    present: &[TermIdx],
    excluded: &[TermIdx],
) -> Result<Option<FullRanking<'a>>, FusionError> {
    let Some(scored) = m.score_terms(present, excluded) else {
        return Ok(None);
    };
    let resnik = m.resnik().scores(m.atlas(), present);
    let fused = ranker_v3::fuse_vectors(&scored.scores, &resnik, ranker_v3::SELECTED)?;
    let scores: Vec<f64> = match scorer {
        Scorer::Fusion => fused.iter().map(|f| f.score).collect(),
        Scorer::Atlas => scored.scores.clone(),
        Scorer::Resnik => resnik,
    };
    let order = tie_groups(&scores, m);
    Ok(Some(FullRanking {
        scorer,
        scored,
        fused,
        scores,
        order,
    }))
}

impl FullRanking<'_> {
    /// Rank and score of a disease, if it is a candidate.
    pub fn of(&self, disease: DiseaseIdx) -> Option<(Rank, f64)> {
        self.order
            .iter()
            .find(|(s, _)| self.scored.disease(*s) == disease)
            .map(|&(s, r)| (r, self.scores[s]))
    }

    /// The first `top` entries, explained.
    pub fn top(&self, top: usize) -> Vec<Ranked> {
        self.order
            .iter()
            .take(top)
            .map(|&(slot, rank)| {
                let f = &self.fused[slot];
                Ranked {
                    disease: self.scored.disease(slot),
                    score: self.scores[slot],
                    rank,
                    atlas: Component {
                        score: f.atlas_score,
                        midrank: f.atlas_midrank,
                    },
                    resnik: Component {
                        score: f.resnik_score,
                        midrank: f.resnik_midrank,
                    },
                    explanation: self.scored.explain(slot),
                }
            })
            .collect()
    }
}

/// Rank by canonical terms with `scorer`; the first `top` entries in RANK-v2 order.
pub fn rank(
    m: &Matcher,
    scorer: Scorer,
    present: &[TermIdx],
    excluded: &[TermIdx],
    top: usize,
) -> Result<Vec<Ranked>, FusionError> {
    Ok(rank_all(m, scorer, present, excluded)?.map_or_else(Vec::new, |r| r.top(top)))
}

/// Every slot with its RANK-v2 rank, best first; ties listed in disease-id order.
fn tie_groups(scores: &[f64], m: &Matcher) -> Vec<(usize, Rank)> {
    let atlas = m.atlas();
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let mut out = Vec::with_capacity(order.len());
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && scores[order[end - 1]] - scores[order[end]] <= TIE_TOLERANCE {
            end += 1;
        }
        let rank = Rank {
            mid: (start + end + 1) as f64 / 2.0,
            best: start + 1,
            worst: end,
            tied: end - start,
        };
        let mut group = order[start..end].to_vec();
        group.sort_by(|&a, &b| {
            let id = |s: usize| &atlas.disease_at(m.candidates()[s]).id;
            id(a).cmp(id(b))
        });
        out.extend(group.into_iter().map(|s| (s, rank)));
        start = end;
    }
    out
}
