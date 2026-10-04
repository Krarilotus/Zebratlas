//! Rare-disease atlas computations. Each result carries its explanation.

pub mod cluster;
pub mod evidence;
pub mod matcher;
pub mod mechanism_statements;
pub mod ranker_v3;
pub mod ranking;
pub mod related;
pub mod resnik;
pub mod similarity;
pub mod validation;

pub use cluster::{Cluster, ClusterParams, ClusterReport, Slice, clusters, dee_slice};
pub use matcher::{Contribution, ContributionKind, Match, Matcher, Scored, ScoringParams};
pub use ranking::{Ranked, Scorer};
pub use related::{Counterexample, CounterexampleKind, GeneMechanisms, Related, RelatedError, RelatedReport, Verdict};
pub use similarity::{
    EffectRule, MechanismSimilarity, PhenotypeSimilarity, ProcessSet, RunProvenance, SimilarityIndex, SimilarityParams,
};
