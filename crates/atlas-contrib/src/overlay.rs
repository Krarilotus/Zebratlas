//! The accepted contributions as an overlay the graph loads next to the curated data.
//!
//! Everything in it is `user_asserted`: a separate edge kind and link level, so the UI can always
//! tell it apart from curated (`observed`) edges. Nothing here edits curated data: corrections and
//! contact updates are annotations on curated ids, shown beside them, never replacing them.
//!
//! File: `contrib-overlay.json` next to the database (rewritten on every accept), and
//! `GET /api/contribute/overlay`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::checks::{asserted_edge, new_node_id};
use crate::error::{ContribError, Result};
use crate::graph::parse_edge_id;
use crate::model::{Agent, CheckStatus, Contribution, ContributionKind, Decision, State};
use crate::util::now_rfc3339;

pub const FORMAT: &str = "rare-atlas-contrib-overlay/1";
/// Edge kind and link level of every overlay edge.
pub const USER_ASSERTED: &str = "user_asserted";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    pub format: String,
    pub generated_at: String,
    pub nodes: Vec<OverlayNode>,
    pub edges: Vec<OverlayEdge>,
    pub annotations: Vec<OverlayAnnotation>,
}

/// A node the curated graph does not have (a patient group, registry, study or person).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Agent>,
    /// `contrib:<contribution id>`.
    pub id: String,
    /// API.md node kind: `organisation`, `study`, `person`.
    pub kind: String,
    /// `patient_group`, `organisation`, `registry`, `study`, `person`.
    pub subject_kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_url: Option<String>,
    pub origin: String,
    pub contribution: String,
}

/// Evidence of a user-asserted edge, in the API's `Evidence` spirit plus what the check saw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayEvidence {
    /// Always `contribution`.
    pub source: String,
    /// Contribution id (the record locator).
    pub record: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// Whether the auto-check found the quote on the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_found: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayEdge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Agent>,
    /// `from|relation|to`.
    pub id: String,
    pub from: String,
    pub relation: String,
    pub to: String,
    /// Always `user_asserted`.
    pub kind: String,
    /// Always `user_asserted`.
    pub level: String,
    /// The contributor's statement, as written.
    pub reason: String,
    pub evidence: Vec<OverlayEvidence>,
    pub contribution: String,
    pub submitted_at: String,
    pub accepted_at: String,
    pub review_reason: String,
    /// PROV entity of the accepted version (`contrib:<id>/v<n>`); `/api/contribute/<id>/prov`.
    pub prov: String,
}

/// A note on a curated (or overlay) edge or node: correction, missing evidence, new contact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayAnnotation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Agent>,
    /// Unmapped/custom proposal details, kept as submitted for review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// `correction`, `missing_evidence`, `outdated_contact`.
    pub kind: String,
    /// Edge id or node id the note is about.
    pub about: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<OverlayEvidence>,
    pub origin: String,
    pub contribution: String,
    pub accepted_at: String,
    pub review_reason: String,
    pub prov: String,
}

impl Overlay {
    /// Builds the overlay from contributions (non-accepted ones are ignored).
    pub fn build(contributions: &[Contribution]) -> Self {
        let mut out = Overlay {
            format: FORMAT.into(),
            generated_at: now_rfc3339(),
            nodes: Vec::new(),
            edges: Vec::new(),
            annotations: Vec::new(),
        };
        let mut accepted: Vec<&Contribution> = contributions
            .iter()
            .filter(|c| c.state == State::Accepted && c.review.as_ref().is_some_and(|r| r.decision == Decision::Accept))
            .collect();
        accepted.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        for c in accepted {
            let review = c.review.as_ref().expect("filtered on review");
            let evidence = evidence_of(c);
            let sub = &c.submission;
            match c.kind {
                ContributionKind::DataSource => continue,
                ContributionKind::NewLink if asserted_edge(c).is_some() => {
                    let Some(edge) = asserted_edge(c) else { continue };
                    let Some((from, relation, to)) = parse_edge_id(&edge) else {
                        continue;
                    };
                    let report = c.checks.as_ref();
                    if report.and_then(|r| r.subject.as_ref()).is_none() {
                        out.nodes.push(OverlayNode {
                            attribution: Some(public_attribution(c)),
                            id: new_node_id(&c.id),
                            kind: sub.subject_kind.map_or("organisation", |k| k.node_kind()).into(),
                            subject_kind: sub.subject_kind.map_or("organisation", |k| k.as_str()).into(),
                            label: sub
                                .subject
                                .label
                                .clone()
                                .or_else(|| sub.subject.id.clone())
                                .unwrap_or_default(),
                            contact_url: sub.contact_url.clone(),
                            origin: USER_ASSERTED.into(),
                            contribution: c.id.clone(),
                        });
                    }
                    out.edges.push(OverlayEdge {
                        attribution: Some(public_attribution(c)),
                        id: edge.clone(),
                        from: from.into(),
                        relation: relation.into(),
                        to: to.into(),
                        kind: USER_ASSERTED.into(),
                        level: USER_ASSERTED.into(),
                        reason: sub.statement.clone(),
                        evidence,
                        contribution: c.id.clone(),
                        submitted_at: c.created_at.clone(),
                        accepted_at: review.at.clone(),
                        review_reason: review.reason.clone(),
                        prov: c.entity(),
                    });
                }
                kind => {
                    let report = c.checks.as_ref();
                    let about = sub
                        .edge
                        .clone()
                        .or_else(|| report.and_then(|r| r.subject.as_ref()).map(|h| h.id.clone()))
                        .or_else(|| sub.subject.id.clone())
                        .or_else(|| report.and_then(|r| r.target.as_ref()).map(|h| h.id.clone()))
                        .or_else(|| sub.target.as_ref().and_then(|t| t.id.clone()));
                    let about = about.unwrap_or_else(|| format!("contrib:{}", c.id));
                    out.annotations.push(OverlayAnnotation {
                        attribution: Some(public_attribution(c)),
                        details: Some(c.public()["submission"].clone()),
                        kind: kind.as_str().into(),
                        about,
                        statement: sub.statement.clone(),
                        contact_url: sub.contact_url.clone(),
                        evidence,
                        origin: USER_ASSERTED.into(),
                        contribution: c.id.clone(),
                        accepted_at: review.at.clone(),
                        review_reason: review.reason.clone(),
                        prov: c.entity(),
                    });
                }
            }
        }
        out
    }

    /// Reads an overlay file (for the graph loader).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ContribError::Internal(format!("reading {}: {e}", path.display())))?;
        let overlay: Overlay = serde_json::from_str(&text)
            .map_err(|e| ContribError::Internal(format!("parsing {}: {e}", path.display())))?;
        if overlay.format != FORMAT {
            return Err(ContribError::Internal(format!(
                "{}: unknown overlay format {}",
                path.display(),
                overlay.format
            )));
        }
        Ok(overlay)
    }

    /// Writes atomically (temporary file, then rename).
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| ContribError::Internal(format!("creating {}: {e}", dir.display())))?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(|e| ContribError::Internal(e.to_string()))?;
        std::fs::write(&tmp, body).map_err(|e| ContribError::Internal(format!("writing {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| ContribError::Internal(format!("replacing {}: {e}", path.display())))?;
        Ok(())
    }
}

fn public_attribution(c: &Contribution) -> Agent {
    Agent::person(
        format!("agent:contributor/{}", c.id),
        Some(
            c.contributor
                .name
                .clone()
                .unwrap_or_else(|| "anonymous contributor".into()),
        ),
    )
}

fn evidence_of(c: &Contribution) -> Vec<OverlayEvidence> {
    let sub = &c.submission;
    if sub.evidence_url.is_none() && sub.quote.is_none() {
        return Vec::new();
    }
    let report = c.checks.as_ref();
    let fetched = report.and_then(|r| r.fetched.iter().find(|f| Some(&f.url) == sub.evidence_url.as_ref()));
    let quote_found = report.and_then(|r| r.find("quote")).and_then(|q| match q.status {
        CheckStatus::Pass => Some(true),
        CheckStatus::Fail => Some(false),
        _ => None,
    });
    vec![OverlayEvidence {
        source: "contribution".into(),
        record: c.id.clone(),
        url: sub.evidence_url.clone(),
        quote: sub.quote.clone(),
        quote_found,
        retrieved_at: fetched.map(|f| f.retrieved_at.clone()),
        sha256: fetched.and_then(|f| f.sha256.clone()),
        date: c.created_at.clone(),
    }]
}
