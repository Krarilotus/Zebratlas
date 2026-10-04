//! Official ecosystem doors. These are organisation nodes, with separately dated
//! source evidence for their actions and publisher-stated scope.

use super::RecIdx;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InitiativeScope {
    pub kind: String,
    pub label: String,
    pub target: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OfficialAction {
    pub action: String,
    pub url: String,
    pub outcome: String,
    pub audience: String,
    pub retrieved_at: String,
    pub sha256: String,
    pub source_url: String,
    pub source_version: String,
    pub record_locator: String,
    pub availability: Option<String>,
    pub records: Vec<RecIdx>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Initiative {
    /// ID of its organisation node; never a name-based merge with another organisation.
    pub id: String,
    pub verified: bool,
    pub exclusion_reason: Option<String>,
    pub research_only: bool,
    pub scopes: Vec<InitiativeScope>,
    /// Only official, acquired destinations; blocked and closed routes stay in the cache.
    pub actions: Vec<OfficialAction>,
    /// Provided-context references remain separate from acquired official actions.
    pub reported_actions: Vec<ReportedAction>,
    pub records: Vec<RecIdx>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReportedAction {
    pub url: String,
    pub action: String,
    pub outcome: String,
    pub audience: String,
    pub verification: String,
    pub destination_status: String,
    pub source_version: String,
    pub sha256: String,
    pub retrieved_at: String,
    pub record_locator: String,
    pub records: Vec<RecIdx>,
}
