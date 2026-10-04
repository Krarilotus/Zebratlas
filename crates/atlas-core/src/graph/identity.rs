//! Exact mapping rows and original members remain inspectable after canonicalisation.
use super::RecIdx;
use crate::provenance::Activity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityMapping {
    pub subject: String,
    pub object: String,
    pub mapping_set_id: String,
    pub mapping_set_version: String,
    /// SourceRecord of the exact physical TSV line; its SHA256 binds all columns.
    pub record: RecIdx,
    pub rule_id: String,
    pub rule_version: String,
    pub evidence_url: String,
    pub evidence_sha256: String,
    pub evidence_locator: String,
    pub mapping_tool: String,
    pub assertion_id: String,
    pub decision_id: String,
    pub gate_manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityMember {
    pub id: String,
    /// Original graph source records and/or the source-asserted mapping records.
    pub derived_from: Vec<RecIdx>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityMerge {
    pub canonical: String,
    pub activity: Activity,
    pub members: Vec<IdentityMember>,
    pub mappings: Vec<IdentityMapping>,
}
