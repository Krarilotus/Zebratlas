//! Weighted reciprocal-rank fusion of full atlas and Resnik score vectors (preregistered as
//! ranker-v3; the live `/api/match` default since D30.2, see [`crate::ranking`]).
//!
//! Component vectors must contain the same complete candidate set. Component and
//! final ranks use RANK-v2 single-linkage ties at absolute tolerance 1e-9. Scores
//! are uncalibrated: they are not probabilities. The atlas matcher itself is unchanged.
//! See eval/preregistration/ranker-v3.md for the frozen experiment and limitations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TIE_TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct FusionParams {
    /// Weight of the atlas component, between zero and one inclusive.
    pub atlas_weight: f64,
    /// Nonnegative rank offset; larger values soften emphasis on the very top.
    pub offset: f64,
}

#[derive(Debug, Error, PartialEq)]
pub enum FusionError {
    #[error("component candidate sets differ")]
    CandidateMismatch,
    #[error("component scores must all be finite")]
    NonFiniteScore,
    #[error("weight must be finite in [0,1] and offset finite and nonnegative")]
    InvalidParameters,
}

#[derive(Debug, Serialize)]
pub struct FusedScore {
    pub disease: String,
    pub score: f64,
    /// Retained so a caller can explain the fusion without inventing evidence.
    pub atlas_score: f64,
    pub resnik_score: f64,
    pub atlas_midrank: f64,
    pub resnik_midrank: f64,
}

/// Full-depth mid-ranks in the input order. Sorting stability cannot break ties.
pub fn midranks(scores: &[f64]) -> Result<Vec<f64>, FusionError> {
    if scores.iter().any(|s| !s.is_finite()) {
        return Err(FusionError::NonFiniteScore);
    }
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let mut ranks = vec![0.0; scores.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && scores[order[end - 1]] - scores[order[end]] <= TIE_TOLERANCE {
            end += 1;
        }
        let mid = (start + end + 1) as f64 / 2.0;
        for &index in &order[start..end] {
            ranks[index] = mid;
        }
        start = end;
    }
    Ok(ranks)
}

impl FusionParams {
    pub fn validate(self) -> Result<(), FusionError> {
        if !self.atlas_weight.is_finite()
            || !(0.0..=1.0).contains(&self.atlas_weight)
            || !self.offset.is_finite()
            || self.offset < 0.0
        {
            return Err(FusionError::InvalidParameters);
        }
        Ok(())
    }
}

/// Fuse full, uncensored component vectors. The result is in disease-ID order;
/// apply `midranks` to the returned scores for evaluation, rather than assigning
/// ordinal ranks after a stable sort. Source annotation provenance belongs to
/// the component scorers; this result preserves both scores and their mid-ranks.
pub fn fuse(
    atlas: &BTreeMap<String, f64>,
    resnik: &BTreeMap<String, f64>,
    params: FusionParams,
) -> Result<Vec<FusedScore>, FusionError> {
    if !atlas.keys().eq(resnik.keys()) {
        return Err(FusionError::CandidateMismatch);
    }
    let a: Vec<f64> = atlas.values().copied().collect();
    let r: Vec<f64> = resnik.values().copied().collect();
    let fused = fuse_vectors(&a, &r, params)?;
    Ok(atlas
        .keys()
        .zip(fused)
        .map(|(d, f)| FusedScore {
            disease: d.clone(),
            ..f
        })
        .collect())
}

/// [`fuse`] over two aligned score vectors (same candidate per index); `disease` is left empty.
pub fn fuse_vectors(atlas: &[f64], resnik: &[f64], params: FusionParams) -> Result<Vec<FusedScore>, FusionError> {
    params.validate()?;
    if atlas.len() != resnik.len() {
        return Err(FusionError::CandidateMismatch);
    }
    let ar = midranks(atlas)?;
    let rr = midranks(resnik)?;
    Ok((0..atlas.len())
        .map(|i| FusedScore {
            disease: String::new(),
            score: params.atlas_weight / (params.offset + ar[i])
                + (1.0 - params.atlas_weight) / (params.offset + rr[i]),
            atlas_score: atlas[i],
            resnik_score: resnik[i],
            atlas_midrank: ar[i],
            resnik_midrank: rr[i],
        })
        .collect())
}

/// The preregistered winner (w=0.25, k=1: 75% Resnik rank + 25% atlas rank), live since D30.2.
pub const SELECTED: FusionParams = FusionParams {
    atlas_weight: 0.25,
    offset: 1.0,
};

/// The frozen run that selected and tested [`SELECTED`].
pub const RUN_ID: &str = "20261003T195145Z-45a0acf-ranker-v3";

#[cfg(test)]
mod tests {
    use super::*;

    fn table(values: &[(&str, f64)]) -> BTreeMap<String, f64> {
        values.iter().map(|(d, s)| (d.to_string(), *s)).collect()
    }

    #[test]
    fn single_linkage_ties_and_permutations() {
        let values = [1.0, 1.0 - 0.75e-9, 1.0 - 1.5e-9, 0.0];
        assert_eq!(midranks(&values).unwrap(), [2.0, 2.0, 2.0, 4.0]);
        assert_eq!(
            midranks(&[0.0, values[2], values[0], values[1]]).unwrap(),
            [4.0, 2.0, 2.0, 2.0]
        );
        assert!(midranks(&[]).unwrap().is_empty());
    }

    #[test]
    fn explicit_fusion_and_explanation() {
        let a = table(&[("a", 3.0), ("b", 2.0), ("c", 1.0)]);
        let r = table(&[("c", 3.0), ("b", 2.0), ("a", 1.0)]);
        let scores = fuse(
            &a,
            &r,
            FusionParams {
                atlas_weight: 0.75,
                offset: 1.0,
            },
        )
        .unwrap();
        assert_eq!(scores[0].score, 0.75 / 2.0 + 0.25 / 4.0);
        assert_eq!((scores[0].atlas_midrank, scores[0].resnik_midrank), (1.0, 3.0));
        assert_eq!((scores[0].atlas_score, scores[0].resnik_score), (3.0, 1.0));
        assert_eq!(
            midranks(&scores.iter().map(|s| s.score).collect::<Vec<_>>()).unwrap(),
            [1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn refuses_mismatched_candidates_and_invalid_inputs() {
        let a = table(&[("a", 1.0)]);
        let b = table(&[("b", 1.0)]);
        let p = FusionParams {
            atlas_weight: 0.5,
            offset: 1.0,
        };
        assert_eq!(fuse(&a, &b, p).unwrap_err(), FusionError::CandidateMismatch);
        assert_eq!(midranks(&[f64::NAN]).unwrap_err(), FusionError::NonFiniteScore);
        assert_eq!(midranks(&[f64::INFINITY]).unwrap_err(), FusionError::NonFiniteScore);
        for params in [
            FusionParams {
                atlas_weight: 1.1,
                offset: 1.0,
            },
            FusionParams {
                atlas_weight: 0.5,
                offset: -1.0,
            },
            FusionParams {
                atlas_weight: f64::NAN,
                offset: 1.0,
            },
        ] {
            assert_eq!(fuse(&a, &a, params).unwrap_err(), FusionError::InvalidParameters);
        }
    }

    #[test]
    fn ties_remain_ties_not_id_based_winners() {
        let a = table(&[("a", 3.0), ("b", 3.0), ("c", 1.0)]);
        let out = fuse(
            &a,
            &a,
            FusionParams {
                atlas_weight: 0.25,
                offset: 60.0,
            },
        )
        .unwrap();
        let ranks = midranks(&out.iter().map(|s| s.score).collect::<Vec<_>>()).unwrap();
        assert_eq!(ranks, [1.5, 1.5, 3.0]);
    }
}
