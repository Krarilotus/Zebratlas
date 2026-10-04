//! Contribution data model: what people submit, what the auto-checks found, and what a reviewer
//! decided. Submissions are cleaned (trimmed, length-limited, URLs checked) before they are stored;
//! the cleaned text is stored as given, never rewritten.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ContribError, Result};
use crate::text::normalize;

/// What the contribution is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionKind {
    /// A patient group, organisation, registry, study or person connected to a condition or gene.
    NewLink,
    /// Something shown is wrong (an edge or a node).
    Correction,
    /// An edge or condition lacks evidence we should have (a report, optionally with a lead).
    MissingEvidence,
    /// The contact shown for an organisation, study or person is outdated.
    OutdatedContact,
    /// A resource proposed for discovery and alignment, never a graph assertion.
    DataSource,
    /// A proposal outside the listed kinds; never automatically classified.
    Other,
}

impl ContributionKind {
    pub const ALL: [Self; 6] = [
        Self::NewLink,
        Self::Correction,
        Self::MissingEvidence,
        Self::OutdatedContact,
        Self::DataSource,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewLink => "new_link",
            Self::Correction => "correction",
            Self::MissingEvidence => "missing_evidence",
            Self::OutdatedContact => "outdated_contact",
            Self::DataSource => "data_source",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What kind of thing a new link connects to the condition or gene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    PatientGroup,
    Organisation,
    Registry,
    Study,
    Person,
    Other,
}

impl SubjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PatientGroup => "patient_group",
            Self::Organisation => "organisation",
            Self::Registry => "registry",
            Self::Study => "study",
            Self::Person => "person",
            Self::Other => "other",
        }
    }

    /// The graph node kind (API.md `NodeRef.kind`) this subject becomes.
    pub fn node_kind(self) -> &'static str {
        match self {
            Self::PatientGroup | Self::Organisation => "organisation",
            Self::Registry | Self::Study => "study",
            Self::Person => "person",
            Self::Other => "other",
        }
    }

    /// Relation of the user-asserted edge `subject → target`. Organisation and study relations reuse
    /// the curated graph's names; `researches_*` is new (curated person links go through papers).
    pub fn relation(self, target_kind: Option<&str>) -> Option<&'static str> {
        if self == Self::Other {
            return None;
        }
        let gene = target_kind == Some("gene");
        Some(match (self.node_kind(), gene) {
            ("organisation", false) => "serves_condition",
            ("organisation", true) => "serves_gene",
            ("study", false) => "studies_condition",
            ("study", true) => "names_gene",
            (_, false) => "researches_condition",
            (_, true) => "researches_gene",
        })
    }
}

/// A node named by id (from a card or condition page) or by free text (typed by a person).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl NodeInput {
    pub fn is_empty(&self) -> bool {
        self.id.is_none() && self.label.is_none()
    }

    fn clean(self, field: &str) -> Result<Self> {
        Ok(Self {
            id: clean_opt(self.id, &format!("{field}.id"))?,
            label: clean_opt(self.label, &format!("{field}.label"))?,
        })
    }

    /// Key for duplicate detection before resolution: the id, else the normalised label.
    pub fn key(&self) -> Option<String> {
        self.id
            .clone()
            .or_else(|| self.label.as_deref().map(|l| format!("label:{}", normalize(l))))
    }
}

/// Where the lead came from: the page the person was on, an AI assistant that found it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundVia {
    /// Atlas page or external page the person started from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// Name of the assistant that found the lead (`ChatGPT`, `Claude`, ...), as the person says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant: Option<String>,
}

/// Who contributes, when not signed in (or in addition to the account).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributorInput {
    /// Name to show with the contribution (optional; anonymous otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Required contact e-mail (filled from a signed-in account); never shown publicly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// Patient group or institution the person speaks for, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organisation: Option<String>,
}

impl fmt::Debug for ContributorInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContributorInput")
            .field("name", &self.name)
            .field("contact", &self.contact.as_ref().map(|_| "***"))
            .field("organisation", &self.organisation)
            .finish()
    }
}

/// `POST /api/contribute` body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub kind: ContributionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_other: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_source: Option<DataSourceInput>,
    /// Optional classification of a new link; missing/custom kinds stay unmapped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_kind: Option<SubjectKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_kind_other: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_other: Option<String>,
    /// New link: the group/registry/study/person. Outdated contact: whose contact. Correction: the
    /// node that is wrong (when it is not an edge).
    #[serde(default)]
    pub subject: NodeInput,
    /// The condition or gene (new link, missing evidence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeInput>,
    /// Existing edge `from|relation|to` the correction or missing-evidence report is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// In the person's own words: what is new, wrong or missing.
    #[serde(default)]
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
    /// Exact words from the evidence page that show it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// Official channel (new link) or the current contact (outdated contact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_url: Option<String>,
    #[serde(default)]
    pub found_via: FoundVia,
    /// Language the statement is written in (BCP 47).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default)]
    pub contributor: ContributorInput,
}

pub const MAX_STATEMENT_CHARS: usize = 2000;
pub const MAX_QUOTE_CHARS: usize = 1000;
pub const MAX_REASON_CHARS: usize = 1000;

impl Submission {
    /// Trims and length-checks every field and enforces the per-kind required fields.
    pub fn clean(self) -> Result<Self> {
        let data_source = self.data_source.map(DataSourceInput::clean).transpose()?;
        let statement = if self.kind == ContributionKind::DataSource && self.statement.is_empty() {
            data_source.as_ref().map(|s| s.description.clone()).unwrap_or_default()
        } else {
            self.statement
        };
        let statement = clean_opt(Some(statement), "statement")?.unwrap_or_default();
        let s = Self {
            kind: self.kind,
            kind_other: clean_opt(self.kind_other, "kind_other")?,
            data_source,
            subject_kind: self.subject_kind,
            subject_kind_other: clean_opt(self.subject_kind_other, "subject_kind_other")?,
            relationship: self.relationship,
            relationship_other: clean_opt(self.relationship_other, "relationship_other")?,
            subject: self.subject.clean("subject")?,
            target: match self.target {
                Some(t) => Some(t.clean("target")?).filter(|t| !t.is_empty()),
                None => None,
            },
            edge: clean_opt(self.edge, "edge")?,
            statement,
            evidence_url: clean_url(self.evidence_url, "evidence_url")?,
            quote: clean_opt(self.quote, "quote")?,
            contact_url: clean_url(self.contact_url, "contact_url")?,
            found_via: FoundVia {
                page: clean_opt(self.found_via.page, "found_via.page")?,
                assistant: clean_opt(self.found_via.assistant, "found_via.assistant")?,
            },
            lang: clean_opt(self.lang, "lang")?,
            contributor: ContributorInput {
                name: clean_opt(self.contributor.name, "contributor.name")?,
                contact: clean_opt(self.contributor.contact, "contributor.contact")?,
                organisation: clean_opt(self.contributor.organisation, "contributor.organisation")?,
            },
        };
        if let Some(edge) = &s.edge
            && edge.split('|').count() != 3
        {
            return Err(ContribError::invalid("edge must look like from|relation|to"));
        }
        crate::schema::validate(&s)?;
        if s.kind != ContributionKind::DataSource && s.data_source.is_some() {
            return Err(ContribError::invalid("data_source is only valid for kind data_source"));
        }
        Ok(s)
    }

    /// Relation of the edge this contribution asserts (new links only).
    pub fn relation(&self, target_kind: Option<&str>) -> Option<&'static str> {
        if self.kind != ContributionKind::NewLink {
            return None;
        }
        if let Some(r) = self.relationship {
            return r.as_relation();
        }
        match (self.kind, self.subject_kind) {
            (_, Some(SubjectKind::Other)) => None,
            (ContributionKind::NewLink, Some(k)) => k.relation(target_kind),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Registry,
    Dataset,
    KnowledgeGraph,
    Api,
    SparqlEndpoint,
    SssomMapping,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IdentifierSystem {
    Mondo,
    Omim,
    Orpha,
    Hgnc,
    Hpo,
    Nct,
    #[serde(rename = "other", alias = "OTHER")]
    Other,
}

/// Explicit relationships are optional. Custom text is retained for review, never mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relationship {
    ServesCondition,
    ServesGene,
    StudiesCondition,
    NamesGene,
    ResearchesCondition,
    ResearchesGene,
    Other,
}

impl Relationship {
    pub fn as_relation(self) -> Option<&'static str> {
        match self {
            Self::ServesCondition => Some("serves_condition"),
            Self::ServesGene => Some("serves_gene"),
            Self::StudiesCondition => Some("studies_condition"),
            Self::NamesGene => Some("names_gene"),
            Self::ResearchesCondition => Some("researches_condition"),
            Self::ResearchesGene => Some("researches_gene"),
            Self::Other => None,
        }
    }
}

pub const MAX_OTHER_CHARS: usize = 300;
pub const MAX_IDENTIFIER_SYSTEMS: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSourceInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<ResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind_other: Option<String>,
    #[serde(default)]
    pub url: String,
    /// Submitter's claim, not a verified permission to ingest.
    #[serde(default)]
    pub licence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spdx_id: Option<String>,
    #[serde(default)]
    pub identifier_systems: Vec<IdentifierSystem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier_systems_other: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Legacy field, optional; no bundled consent is required to submit a lead.
    #[serde(default)]
    pub consent: bool,
}

impl DataSourceInput {
    fn clean(self) -> Result<Self> {
        if self.identifier_systems.len() > MAX_IDENTIFIER_SYSTEMS {
            return Err(ContribError::invalid("too many identifier systems"));
        }
        Ok(Self {
            resource_kind: self.resource_kind,
            resource_kind_other: clean_opt(self.resource_kind_other, "data_source.resource_kind_other")?,
            url: clean_url(Some(self.url), "data_source.url")?.unwrap_or_default(),
            licence: clean_opt(Some(self.licence), "data_source.licence")?.unwrap_or_default(),
            spdx_id: clean_opt(self.spdx_id, "data_source.spdx_id")?,
            identifier_systems: self.identifier_systems,
            identifier_systems_other: clean_opt(self.identifier_systems_other, "data_source.identifier_systems_other")?,
            description: clean_opt(Some(self.description), "data_source.description")?.unwrap_or_default(),
            consent: self.consent,
        })
    }
}

fn clean_opt(v: Option<String>, field: &str) -> Result<Option<String>> {
    // Public clean_url also accepts caller-defined diagnostic field names.
    let max = crate::schema::max_length(field).unwrap_or(2000);
    clean_with_limit(v, field, max)
}

fn clean_with_limit(v: Option<String>, field: &str, max: usize) -> Result<Option<String>> {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => {
            if s.chars().count() > max {
                return Err(ContribError::invalid(format!(
                    "{field} is longer than {max} characters"
                )));
            }
            if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err(ContribError::invalid(format!("{field} contains control characters")));
            }
            Ok(Some(s))
        }
    }
}

pub(crate) fn clean_req(v: String, field: &str, max: usize) -> Result<String> {
    clean_with_limit(Some(v), field, max)?.ok_or_else(|| ContribError::invalid(format!("{field} is empty")))
}

/// Only absolute `http`/`https` URLs with a host.
pub fn clean_url(v: Option<String>, field: &str) -> Result<Option<String>> {
    let Some(s) = clean_opt(v, field)? else {
        return Ok(None);
    };
    let url = reqwest::Url::parse(&s).map_err(|_| ContribError::invalid(format!("{field} is not a web address")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err(ContribError::invalid(format!(
            "{field} must start with http:// or https://"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ContribError::invalid(format!(
            "{field} must not contain a user name or password"
        )));
    }
    Ok(Some(url.to_string()))
}

/// Review state. `submitted → auto_checked → accepted | rejected`; spam may be rejected straight
/// from `submitted`; accepted and rejected are final (see [`crate::state`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Submitted,
    AutoChecked,
    Accepted,
    Rejected,
}

impl State {
    pub const ALL: [Self; 4] = [Self::Submitted, Self::AutoChecked, Self::Accepted, Self::Rejected];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::AutoChecked => "auto_checked",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn is_open(self) -> bool {
        matches!(self, Self::Submitted | Self::AutoChecked)
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skipped,
}

/// One auto-check result. `code` is stable (the UI translates it); `message` is plain English.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// `evidence_url`, `contact_url`, `quote`, `subject`, `target`, `edge`, `duplicate`, `conflict`.
    pub name: String,
    pub status: CheckStatus,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub message_msg: Value,
    /// A failed blocking check prevents accepting.
    #[serde(default)]
    pub blocking: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

impl Check {
    pub(crate) fn set_message(&mut self, msg: Value) {
        self.message = msg["fallback"].as_str().unwrap_or_default().to_owned();
        self.message_msg = msg;
    }
}

/// A node the checks resolved an input to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHit {
    pub id: String,
    /// API.md node kind (`disease`, `gene`, `organisation`, `study`, `person`, ...).
    pub kind: String,
    pub label: String,
    /// Exact id or name match (not a prefix/token/typo match).
    #[serde(default)]
    pub exact: bool,
    /// Official URLs the graph has for it (website, contact page), for the outdated-contact check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
}

/// A fetched evidence or contact page: what the auto-check saw, so anyone can re-check it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FetchRecord {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub retrieved_at: String,
    /// SHA-256 of the response body as received (hex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// `ok`, `http_status`, `blocked`, `timeout`, `network`, `too_large`, `invalid_url`.
    pub outcome: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckReport {
    pub checked_at: String,
    /// Software agent that ran the checks (`atlas-contrib@0.1.0`).
    pub agent: String,
    pub checks: Vec<Check>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<NodeHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NodeHit>,
    /// Relation of the asserted edge, once the target kind is known (new links).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fetched: Vec<FetchRecord>,
}

impl CheckReport {
    /// Messages of failed blocking checks.
    pub fn blockers(&self) -> Vec<&str> {
        self.checks
            .iter()
            .filter(|c| c.blocking && c.status == CheckStatus::Fail)
            .map(|c| c.message.as_str())
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.name == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accept,
    Reject,
}

/// `POST /api/review/{id}` body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewInput {
    pub decision: Decision,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub decision: Decision,
    pub reason: String,
    pub reviewer: Agent,
    pub at: String,
}

/// A PROV agent: contributor, reviewer, or the checking software.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// `agent:user/<id>`, `agent:anonymous/<contribution>`, `agent:reviewer/<id>`, `agent:software/<name>@<version>`.
    pub id: String,
    /// `prov:Person` or `prov:SoftwareAgent`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Agent {
    pub fn person(id: impl Into<String>, label: Option<String>) -> Self {
        Self {
            id: id.into(),
            kind: "prov:Person".into(),
            label,
        }
    }

    pub fn software() -> Self {
        Self {
            id: format!("agent:software/{}", crate::AGENT),
            kind: "prov:SoftwareAgent".into(),
            label: Some("atlas contribution auto-checks".into()),
        }
    }
}

/// The contributor as stored. `contact` is private to reviewers.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributor {
    /// Account id when signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organisation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
}

impl fmt::Debug for Contributor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Contributor")
            .field("user_id", &self.user_id)
            .field("name", &self.name)
            .field("organisation", &self.organisation)
            .field("contact", &self.contact.as_ref().map(|_| "***"))
            .finish()
    }
}

/// A stored contribution with its current state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    pub id: String,
    pub kind: ContributionKind,
    pub state: State,
    /// Increases with every state change; `contrib:<id>/v<version>` is the PROV entity.
    pub version: u32,
    pub submission: Submission,
    pub contributor: Contributor,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<CheckReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
}

impl Contribution {
    /// PROV entity id of the current version.
    pub fn entity(&self) -> String {
        entity_id(&self.id, self.version)
    }

    /// The agent who submitted it.
    pub fn contributor_agent(&self) -> Agent {
        match &self.contributor.user_id {
            Some(u) => Agent::person(format!("agent:user/{u}"), self.contributor.name.clone()),
            None => Agent::person(
                format!("agent:anonymous/{}", self.id),
                Some(
                    self.contributor
                        .name
                        .clone()
                        .unwrap_or_else(|| "anonymous contributor".into()),
                ),
            ),
        }
    }

    /// Public JSON: everything except the private contact and the account id.
    pub fn public(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(sub) = v.get_mut("submission").and_then(Value::as_object_mut)
            && let Some(c) = sub.get_mut("contributor").and_then(Value::as_object_mut)
        {
            c.remove("contact");
        }
        if let Some(c) = v.get_mut("contributor").and_then(Value::as_object_mut) {
            c.remove("contact");
            let signed_in = c.remove("user_id").is_some();
            c.insert("signed_in".into(), Value::Bool(signed_in));
        }
        if let Some(r) = v.get_mut("review").and_then(Value::as_object_mut) {
            // Reviewer accounts stay internal; the decision and reason are public.
            r.remove("reviewer");
        }
        v
    }
}

pub fn entity_id(id: &str, version: u32) -> String {
    format!("contrib:{id}/v{version}")
}
