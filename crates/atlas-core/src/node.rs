//! Node references and evidence-carrying edges, in the shape of the JSON contract (design/API.md).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Disease,
    Gene,
    Phenotype,
    Pathway,
    Paper,
    Study,
    Grant,
    Person,
    Organisation,
    Asset,
}

/// Dense node address: kind tag + index into that kind's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey {
    pub kind: NodeKind,
    pub idx: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeRef {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// Curated database record.
    Observed,
    /// LLM extraction with a quote span.
    Extracted,
    /// Computed.
    Inferred,
    Hypothesis,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
            Self::Hypothesis => "hypothesis",
        }
    }
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disease => "disease",
            Self::Gene => "gene",
            Self::Phenotype => "phenotype",
            Self::Pathway => "pathway",
            Self::Paper => "paper",
            Self::Study => "study",
            Self::Grant => "grant",
            Self::Person => "person",
            Self::Organisation => "organisation",
            Self::Asset => "asset",
        }
    }
}

/// One supporting record of an edge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: String,
    /// Source record locator (`phenotype.hpoa#L1234`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    pub references: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohort: Option<(u64, u64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// `from|relation|to`; key for `/api/provenance/{id}`.
    pub id: String,
    pub from: String,
    pub to: String,
    pub relation: String,
    pub kind: EdgeKind,
    pub evidence: Vec<Evidence>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub contradicted_by: Vec<Evidence>,
}

impl Edge {
    pub fn new(from: &str, relation: &str, to: &str, kind: EdgeKind, evidence: Vec<Evidence>) -> Self {
        Self {
            id: edge_id(from, relation, to),
            from: from.to_owned(),
            to: to.to_owned(),
            relation: relation.to_owned(),
            kind,
            evidence,
            contradicted_by: Vec::new(),
        }
    }
}

/// `from|relation|to`.
pub fn edge_id(from: &str, relation: &str, to: &str) -> String {
    format!("{from}|{relation}|{to}")
}

/// Inverse of [`edge_id`].
pub fn parse_edge_id(id: &str) -> Option<(&str, &str, &str)> {
    let (from, rest) = id.split_once('|')?;
    let (relation, to) = rest.split_once('|')?;
    Some((from, relation, to))
}
