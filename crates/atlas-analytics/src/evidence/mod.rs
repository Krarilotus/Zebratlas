//! Explainable evidence review, not calibrated probabilities or clinical classification.
//! Policy and integration contract: docs/design/EVIDENCE.md.
mod assess;
mod graph;
mod proposal;
#[cfg(test)]
mod tests;

use atlas_core::node::EdgeKind;
use atlas_core::provenance::{Activity, SourceEntity};
use serde::{Deserialize, Serialize};

pub use assess::{analyze, confidence};
pub use graph::{analyze_atlas, statements_from_atlas};
pub use proposal::propose_shared_mechanism;

pub const POLICY_VERSION: &str = "atlas-evidence/1.0.0";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Phenotype,
    Mechanism,
    GeneAssociation,
    Pathway,
    StudyLink,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Present,
    Absent,
    LossOfFunction,
    GainOfFunction,
    DominantNegative,
    Unknown,
    Frequency { affected: u64, examined: u64 },
    Exact,
    Umbrella,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Expert,
    Curated,
    Cohort,
    AuthorStatement,
    Computational,
    Unrated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curation {
    Reviewed,
    Unreviewed,
    Disputed,
    Refuted,
    Unknown,
}

/// Only explicitly known scope belongs here. None never means universal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub source_disease: String,
    pub inheritance: Option<String>,
    pub variant: Option<String>,
    pub population: Option<String>,
    pub onset: Option<String>,
    pub sex: Option<String>,
    pub modifiers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub entity: SourceEntity,
    pub locator: String,
    pub references: Vec<String>,
}

/// A source assertion, not an aggregate edge. Identifiers must be unique and stable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Statement {
    pub id: String,
    pub subject: String,
    pub relation: Relation,
    pub object: String,
    pub value: Value,
    pub raw_value: String,
    pub kind: EdgeKind,
    pub tier: Tier,
    pub curation: Curation,
    /// Native upstream class (e.g. G2P definitive); never promoted to mechanism certainty.
    pub source_classification: Option<String>,
    pub context: Context,
    pub citations: Vec<Citation>,
    pub reviewed_year: Option<u16>,
    /// Verbatim supporting span required for extracted assertions.
    pub quote: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    DirectContradiction,
    ScopeReview,
    ContextDifference,
    CohortVariation,
    UnknownMechanism,
    IndirectLink,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub statements: Vec<String>,
    pub explanation: String,
    pub next_question: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub reason: String,
    pub points: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    /// Heuristic evidence support, 0..100; NOT a probability or clinical validity category.
    pub support_score: u8,
    pub label: String,
    pub components: Vec<Component>,
    pub limitations: Vec<String>,
    pub statement_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeAssessment {
    pub subject: String,
    pub relation: Relation,
    pub object: String,
    /// Competing values remain visible in Review.statements; never majority-voted away.
    pub confidence: Confidence,
    pub finding_indexes: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub policy_version: String,
    pub activity: Activity,
    pub statements: Vec<Statement>,
    pub edges: Vec<EdgeAssessment>,
    pub findings: Vec<Finding>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub kind: EdgeKind,
    pub policy_version: String,
    pub activity: Activity,
    pub question: String,
    pub rationale: String,
    pub statement_ids: Vec<String>,
    pub citations: Vec<Citation>,
    pub design: Vec<String>,
    pub would_support: String,
    pub would_challenge: String,
    pub must_validate: Vec<String>,
}
