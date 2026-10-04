//! An additive, evidence-preserving semantic-units view. No source graph is modified.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::graph::{AssetKind, GraphEdge, OrgKind, RecordWithhold, StudyKind};
use crate::node::{EdgeKind, NodeKind, NodeRef};
use crate::provenance::{Activity, Agent, RecordRef, SourceEntity};
use crate::{Atlas, Graph};

pub const SCHEMA: &str = "atlas.semantic-units.v1";
pub const RULE: &str = "semantic-units-v1: direct Reactome co-membership; no therapeutic inference";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitType {
    Statement,
    Item,
    Community,
    MechanismGroup,
}

/// Aggregation is never called observed: it is an inferred presentation grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionStatus {
    Asserted,
    Extracted,
    Inferred,
    Hypothesis,
}

impl From<EdgeKind> for AssertionStatus {
    fn from(k: EdgeKind) -> Self {
        match k {
            EdgeKind::Observed => Self::Asserted,
            EdgeKind::Extracted => Self::Extracted,
            EdgeKind::Inferred => Self::Inferred,
            EdgeKind::Hypothesis => Self::Hypothesis,
        }
    }
}

/// Self-contained source pointer; missing upstream metadata stays null, never fabricated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitEvidence {
    pub source: SourceEntity,
    pub locator: String,
    pub record_url: Option<String>,
    pub retrieved_at: Option<String>,
    pub record_sha256: Option<String>,
    pub upstream_activity: Option<String>,
    pub evidence_code: Option<String>,
    pub status: Option<AssertionStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitContact {
    pub node: String,
    pub kind: String,
    pub url: String,
    pub evidence: Vec<UnitEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitResource {
    pub node: NodeRef,
    /// `patient_group`, `registry`, a StudyKind, or an AssetKind.
    pub role: String,
    /// Membership of this presentation grouping is inferred; the proof retains its own status.
    pub support: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticUnit {
    /// Stable logical identity; content changes are identified by `sha256`.
    pub id: String,
    pub unit_type: UnitType,
    pub label: String,
    pub subject: NodeRef,
    pub status: AssertionStatus,
    pub relation: Option<String>,
    pub members: Vec<NodeRef>,
    pub children: Vec<String>,
    /// Raw edge IDs or membership statement IDs; proof, not owned content triples.
    pub support: Vec<String>,
    pub evidence: Vec<UnitEvidence>,
    pub contacts: Vec<UnitContact>,
    pub resources: Vec<UnitResource>,
    pub generated_by: String,
    /// SHA-256 of the canonical struct serialization with this field empty.
    pub sha256: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PathwayData {
    pub annotations: Vec<PathwayAnnotation>,
    pub activity: Activity,
    /// Excluded rows remain identifiable and retain their reason (D10).
    pub excluded: Vec<ExcludedAnnotation>,
    pub available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExcludedAnnotation {
    pub locator: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathwayAnnotation {
    pub gene: String,
    pub pathway: NodeRef,
    pub evidence: UnitEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitCollection {
    pub schema: String,
    pub focus: NodeRef,
    pub roots: Vec<String>,
    /// Compact complete overview, including roots outside the JSON unit page.
    pub root_summaries: Vec<UnitSummary>,
    pub links: Vec<UnitLink>,
    pub units: Vec<SemanticUnit>,
    pub overview: BTreeMap<String, usize>,
    pub activity: Activity,
    pub pathway_activity: Activity,
    pub pathways_available: bool,
    pub excluded: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitSummary {
    pub id: String,
    pub unit_type: UnitType,
    pub subject: NodeRef,
    pub status: AssertionStatus,
    pub resources: usize,
    pub contacts: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitLink {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relation: String,
    pub status: AssertionStatus,
    pub support: Vec<String>,
    pub evidence: Vec<UnitEvidence>,
    pub generated_by: String,
    pub sha256: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn unit_id(kind: &str, key: &str) -> String {
    format!("unit:{kind}:{}", digest(key.as_bytes()))
}

fn node(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeRef> {
    if let Some(d) = atlas.disease_idx(id) {
        return Some(atlas.disease_ref(d));
    }
    if let Some(g) = atlas.gene(id) {
        let g = atlas.gene_at(g);
        return Some(NodeRef {
            id: g.id().into(),
            kind: NodeKind::Gene,
            label: g.symbol.clone(),
        });
    }
    graph.node(id).map(|k| graph.node_ref(k))
}

pub fn resolve_focus(atlas: &Atlas, graph: &Graph, pathways: &PathwayData, id: &str) -> Option<NodeRef> {
    node(atlas, graph, id).or_else(|| {
        pathways
            .annotations
            .iter()
            .find(|a| a.pathway.id == id)
            .map(|a| a.pathway.clone())
    })
}

fn new_unit(kind: UnitType, key: &str, subject: NodeRef, label: String) -> SemanticUnit {
    let tag = match kind {
        UnitType::Statement => "statement",
        UnitType::Item => "item",
        UnitType::Community => "community",
        UnitType::MechanismGroup => "mechanism",
    };
    SemanticUnit {
        id: unit_id(tag, key),
        unit_type: kind,
        label,
        subject,
        status: AssertionStatus::Inferred,
        relation: None,
        members: Vec::new(),
        children: Vec::new(),
        support: Vec::new(),
        evidence: Vec::new(),
        contacts: Vec::new(),
        resources: Vec::new(),
        generated_by: "activity:semantic-units-v1".into(),
        sha256: String::new(),
    }
}

fn record_evidence(graph: &Graph, records: &[u32], activity: Option<String>) -> Vec<UnitEvidence> {
    records
        .iter()
        .map(|&i| {
            let r = graph.record(i);
            UnitEvidence {
                source: graph.provenance().entity(r.entity).clone(),
                locator: r.locator.to_string(),
                record_url: r.url.clone(),
                retrieved_at: r.fetched_at.clone(),
                record_sha256: Some(crate::graph::hex(&r.sha256)),
                upstream_activity: activity.clone(),
                evidence_code: None,
                status: None,
            }
        })
        .collect()
}

fn atlas_evidence(atlas: &Atlas, r: &RecordRef) -> UnitEvidence {
    UnitEvidence {
        source: atlas.provenance.entity(r.entity).clone(),
        locator: r.locator.to_string(),
        record_url: None,
        retrieved_at: atlas.provenance.entity(r.entity).retrieved_at.clone(),
        record_sha256: None,
        evidence_code: None,
        status: Some(AssertionStatus::Asserted),
        upstream_activity: atlas.provenance.generator_of(r.entity).map(|a| a.id.clone()),
    }
}

fn visible(graph: &Graph, filter: &dyn RecordWithhold, id: &str) -> bool {
    graph.node(id).is_none_or(|k| filter.node_withheld(k).is_none())
}

fn edge_unit(atlas: &Atlas, graph: &Graph, filter: &dyn RecordWithhold, edge: &GraphEdge) -> Option<SemanticUnit> {
    if !visible(graph, filter, &edge.from)
        || !visible(graph, filter, &edge.to)
        || filter.records_withheld(&edge.records).is_some()
    {
        return None;
    }
    let from = node(atlas, graph, &edge.from)?;
    let to = node(atlas, graph, &edge.to)?;
    let mut u = new_unit(
        UnitType::Statement,
        &edge.id(),
        from.clone(),
        // Source labels only. Consumers localise the relation code through their catalogs.
        format!("{} · {}", from.label, to.label),
    );
    u.status = edge.kind.into();
    u.relation = Some(edge.relation.as_str().into());
    u.members = vec![from, to];
    u.support.push(edge.id());
    u.evidence = record_evidence(
        graph,
        &edge.records,
        Some(graph.provenance().activity(edge.activity).id.clone()),
    );
    for e in &mut u.evidence {
        e.status = Some(edge.kind.into());
    }
    Some(u)
}

fn finalize(u: &mut SemanticUnit) {
    u.members.sort_by(|a, b| a.id.cmp(&b.id));
    u.members.dedup_by(|a, b| a.id == b.id);
    u.children.sort();
    u.children.dedup();
    u.support.sort();
    u.support.dedup();
    u.evidence.sort_by_cached_key(|e| serde_json::to_string(e).unwrap());
    u.evidence.dedup();
    u.contacts.sort_by(|a, b| (&a.node, &a.url).cmp(&(&b.node, &b.url)));
    u.contacts.dedup();
    u.resources.sort_by(|a, b| a.node.id.cmp(&b.node.id));
    u.sha256.clear();
    u.sha256 = digest(&serde_json::to_vec(u).unwrap());
}

mod build;
pub use build::{build, build_with_filter};
pub mod rdf;

#[cfg(test)]
mod tests;
