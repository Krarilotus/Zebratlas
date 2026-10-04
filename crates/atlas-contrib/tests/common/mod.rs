#![allow(dead_code)]

use std::sync::Arc;

use atlas_contrib::model::{ContributorInput, FoundVia, NodeInput, SubjectKind};
use atlas_contrib::{Contrib, ContribConfig, ContributionKind, MemoryGraph, Submission};

/// A small graph: one condition, one gene, one curated patient group serving the condition, one
/// study. Labels are placeholders, not real organisations.
pub fn graph() -> MemoryGraph {
    MemoryGraph::new()
        .node("MONDO:0000001", "disease", "Example encephalopathy", &["EXE"], &[])
        .node("MONDO:0000002", "disease", "Example syndrome type 1", &[], &[])
        .node("MONDO:0000003", "disease", "Example syndrome type 2", &[], &[])
        .node("HGNC:1", "gene", "EXG1", &[], &[])
        .node(
            "org:example-group",
            "organisation",
            "Example Families Group",
            &[],
            &["https://example.org/", "https://example.org/contact"],
        )
        .node("NCT00000001", "study", "Example natural history study", &[], &[])
        .edge("org:example-group", "serves_condition", "MONDO:0000001", "observed")
        .edge("NCT00000001", "studies_condition", "MONDO:0000001", "observed")
}

pub fn service() -> Contrib {
    Contrib::new(ContribConfig::for_tests(), Arc::new(graph())).unwrap()
}

pub fn new_link(subject: &str, target_id: &str, evidence_url: Option<&str>, quote: Option<&str>) -> Submission {
    Submission {
        kind: ContributionKind::NewLink,
        kind_other: None,
        subject_kind_other: None,
        relationship: None,
        relationship_other: None,
        data_source: None,
        subject_kind: Some(SubjectKind::PatientGroup),
        subject: NodeInput {
            id: None,
            label: Some(subject.into()),
        },
        target: Some(NodeInput {
            id: Some(target_id.into()),
            label: None,
        }),
        edge: None,
        statement: format!("{subject} supports families with this condition."),
        evidence_url: evidence_url.map(String::from),
        quote: quote.map(String::from),
        contact_url: None,
        found_via: FoundVia::default(),
        lang: Some("en".into()),
        contributor: ContributorInput {
            name: Some("Test Parent".into()),
            contact: Some("parent@example.invalid".into()),
            organisation: None,
        },
    }
}

pub fn correction(edge: &str) -> Submission {
    Submission {
        kind: ContributionKind::Correction,
        kind_other: None,
        subject_kind_other: None,
        relationship: None,
        relationship_other: None,
        data_source: None,
        subject_kind: None,
        subject: NodeInput::default(),
        target: None,
        edge: Some(edge.into()),
        statement: "This group closed in 2025.".into(),
        evidence_url: Some("https://example.org/closed".into()),
        quote: None,
        contact_url: None,
        found_via: FoundVia::default(),
        lang: None,
        contributor: ContributorInput {
            contact: Some("parent@example.invalid".into()),
            ..Default::default()
        },
    }
}

/// Answers from a fixed table; unknown URLs are unreachable. No network.
pub struct StubFetcher(pub Vec<(&'static str, u16, &'static str)>);

#[async_trait::async_trait]
impl atlas_contrib::Fetcher for StubFetcher {
    async fn fetch(&self, url: &str) -> atlas_contrib::Fetched {
        use sha2::Digest;
        match self.0.iter().find(|(u, ..)| *u == url) {
            Some((_, status, body)) => atlas_contrib::Fetched {
                sample: Some(body.to_string()),
                body: Some(body.as_bytes().to_vec()),
                record: atlas_contrib::model::FetchRecord {
                    url: url.into(),
                    final_url: None,
                    status: Some(*status),
                    retrieved_at: "2026-10-03T20:00:00Z".into(),
                    sha256: Some(format!("{:x}", sha2::Sha256::digest(body.as_bytes()))),
                    bytes: body.len() as u64,
                    content_type: Some("text/html".into()),
                    outcome: if (200..300).contains(status) {
                        "ok".into()
                    } else {
                        "http_status".into()
                    },
                },
                text: (200..300)
                    .contains(status)
                    .then(|| atlas_contrib::text::html_to_text(body)),
            },
            None => atlas_contrib::Fetched::failed(url, "network", None),
        }
    }
}

pub fn stubbed(pages: Vec<(&'static str, u16, &'static str)>) -> Contrib {
    service().with_fetcher(Arc::new(StubFetcher(pages)))
}

pub fn reviewer() -> atlas_contrib::model::Agent {
    atlas_contrib::model::Agent::person("agent:reviewer/tester", Some("reviewer tester".into()))
}
