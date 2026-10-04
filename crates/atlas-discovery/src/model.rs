//! Manifest, mentions, alignment decisions and the staged bundle types.

use std::collections::{BTreeMap, BTreeSet};

use atlas_core::curie::normalize_query;
use atlas_core::node::{Edge, NodeKind, NodeRef};
use atlas_core::provenance::{Provenance, RecordRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const POLICY: &str = "explicit-identifier-v1";
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub url: String,
    pub file: String,
    pub version: String,
    pub retrieved_at: String,
    pub sha256: String,
    pub bytes: u64,
    pub licence: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub elapsed_seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub version: u32,
    pub seeds: Vec<String>,
    pub sources: Vec<Source>,
    pub discovery: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub active: bool,
    pub evidence: RecordRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mention {
    /// A source-scoped gene mention, never the identifier of its enclosing protein/panel.
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub explicit_ids: Vec<String>,
    pub evidence: RecordRef,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Exact,
    Review,
    Excluded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alignment {
    pub mention: Mention,
    pub decision: Decision,
    pub targets: Vec<String>,
    pub reason: String,
    /// Rule tier, not an estimated probability.
    pub confidence_tier: String,
}

/// Explicit identifiers must all resolve to one active, same-kind target with consistent label.
/// Unknown/retired/conflicting IDs veto automatic alignment, even if another ID is valid.
/// A label can propose a candidate but cannot establish identity.
pub fn align(mention: Mention, targets: &[Target]) -> Alignment {
    let mut found = BTreeSet::new();
    let mut invalid = false;
    for raw in &mention.explicit_ids {
        match normalize_query(raw).and_then(|id| targets.iter().find(|t| t.id == id)) {
            Some(t) if t.active && t.kind == mention.kind => {
                found.insert(t.id.clone());
            }
            _ => invalid = true,
        }
    }
    let label_conflict = found.iter().any(|id| {
        targets
            .iter()
            .any(|t| &t.id == id && !mention.label.is_empty() && t.label != mention.label)
    });
    let exact = !invalid && !label_conflict && found.len() == 1;
    let reason = if exact {
        "explicit identifier verified against active canonical record"
    } else if invalid {
        "unknown, retired, malformed or wrong-kind explicit identifier"
    } else if found.len() > 1 {
        "conflicting explicit identifiers"
    } else if label_conflict {
        "identifier and label disagree; possible stale symbol"
    } else {
        "no explicit identity evidence; label candidates require review"
    };
    if found.is_empty() {
        found.extend(
            targets
                .iter()
                .filter(|t| t.active && t.kind == mention.kind && t.label.eq_ignore_ascii_case(&mention.label))
                .map(|t| t.id.clone()),
        );
    }
    Alignment {
        mention,
        decision: if exact { Decision::Exact } else { Decision::Review },
        targets: found.into_iter().collect(),
        reason: reason.into(),
        confidence_tier: if exact { "identifier_verified" } else { "unverified" }.into(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub subject_id: String,
    pub subject_label: String,
    pub predicate_id: String,
    pub object_id: String,
    pub object_label: String,
    pub mapping_justification: String,
    pub mapping_provider: String,
    pub mapping_date: String,
    pub subject_source: String,
    pub object_source: String,
    pub subject_source_version: String,
    pub object_source_version: String,
    /// Reference into the accompanying PROV graph (includes both exact record locators).
    pub mapping_evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: String,
    pub subject_id: String,
    pub reason: String,
    pub candidates: Vec<String>,
    pub evidence: RecordRef,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub source: String,
    pub locator: RecordRef,
    pub status: String,
    pub reason: String,
    /// Original bytes always remain in the hashed source, at `locator`; never rewritten.
    pub alignment: Option<Alignment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Characterization {
    pub source: String,
    pub schema: String,
    pub fields: BTreeMap<String, String>,
    pub licence: Option<String>,
    pub update_cadence: String,
    pub identifiers: Vec<String>,
    pub language: String,
    pub publication_status: String,
    pub evidence: RecordRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub schema: String,
    pub version: u32,
    pub policy: String,
    pub manifest_sha256: String,
    pub sources: Manifest,
    pub provenance: Provenance,
    pub targets: Vec<Target>,
    pub characterizations: Vec<Characterization>,
    pub records: Vec<Record>,
    pub mappings: Vec<Mapping>,
    pub nodes: Vec<NodeRef>,
    pub edges: Vec<Edge>,
    pub review_queue: Vec<ReviewItem>,
    pub metrics: BTreeMap<String, u64>,
}
