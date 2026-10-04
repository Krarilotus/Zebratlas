//! People: source-asserted ORCID authors; name-unioned overlap caches are excluded.
//! Every source author occurrence has its own record. Only accepted ORCID receipts
//! canonicalise occurrences; a shared name/grant never authorizes identity.

use std::collections::HashMap;
use std::path::Path;

use atlas_core::graph::{Coverage, LinkLevel, Person, PersonSource, Relation, activity};
use atlas_core::node::{EdgeKind, NodeKind};

use super::builder::{Builder, NewEdge};
use super::works::OrcidAuthor;
use crate::error::IngestError;

pub const OVERLAP: &str = "cache/people/overlap.json";
const MAX_AFFILIATIONS: usize = 5;

pub fn ingest(
    b: &mut Builder<'_>,
    data: &Path,
    orcid_authors: Vec<OrcidAuthor>,
    identity: &super::sssom::Identity,
) -> Result<(), IngestError> {
    let path = data.join(OVERLAP);
    let mut coverage = Coverage {
        source: "people".into(),
        label: "People resolution (PubMed authors + RePORTER PIs)".into(),
        status: "absent".into(),
        files: vec![OVERLAP.into()],
        ..Coverage::default()
    };
    let act = b.start(
        activity::INGEST_PEOPLE,
        "Source author occurrences and accepted ORCID identities",
        &[],
    );
    let ident = b.start(
        activity::IDENTITY_PEOPLE,
        "Person identity: accepted source-asserted ORCID decisions only",
        &[],
    );
    b.param(ident, "identity_rule", "R-PER-04@1.0.0");
    if path.exists() {
        coverage.status = "excluded".into();
        coverage.scope = "name-unioned overlap profiles excluded; source author occurrences and PIs remain".into();
    }

    coverage.records = orcid_authors.len() as u64;
    let orcid_people = orcid(b, orcid_authors, act, identity);
    if orcid_people > 0 {
        coverage.status = "loaded_source_occurrences".into();
    }
    b.finish(
        act,
        &[
            ("excluded:unverified-overlap", usize::from(path.exists())),
            ("orcid_people", orcid_people),
        ],
    );
    b.finish(ident, &[("same_as", 0)]);
    b.data.coverage.push(coverage);
    Ok(())
}

/// ORCID-identified authors not covered by the overlap file become their own person nodes.
fn orcid(
    b: &mut Builder<'_>,
    authors: Vec<OrcidAuthor>,
    act: atlas_core::provenance::ActivityIdx,
    identity: &super::sssom::Identity,
) -> usize {
    let mut created = 0;
    let mut grouped: HashMap<String, Vec<OrcidAuthor>> = HashMap::new();
    let mut order = Vec::new();
    for a in authors {
        let entity = b.data.records[a.record as usize].entity;
        if !b.data.provenance.activity(act).used.contains(&entity) {
            b.data.provenance.activity_mut(act).used.push(entity);
        }
        if !atlas_core::identity_policy::valid_orcid(&a.orcid) {
            continue;
        }
        let pid = identity.canonical(&a.occurrence_id).to_owned();
        if !grouped.contains_key(&pid) {
            order.push(pid.clone());
        }
        grouped.entry(pid).or_default().push(a);
    }
    for pid in order {
        let mentions = grouped.remove(&pid).unwrap_or_default();
        if b.node(&pid).is_none() {
            created += 1;
            let first = &mentions[0];
            let mut affiliations: Vec<String> = Vec::new();
            for a in mentions.iter().flat_map(|m| &m.affiliations) {
                if !a.is_empty() && !affiliations.contains(a) && affiliations.len() < MAX_AFFILIATIONS {
                    affiliations.push(a.clone());
                }
            }
            b.register(&pid, NodeKind::Person, b.data.people.len());
            b.data.people.push(Person {
                id: pid.clone(),
                name: first.name.clone(),
                name_variants: Vec::new(),
                orcids: mentions.iter().map(|a| a.orcid.clone()).collect(),
                affiliations,
                source: PersonSource::Orcid,
                genes: Vec::new(),
                communities: Vec::new(),
                cross_community: false,
                matched_by: vec!["orcid".into()],
                merge_basis: Vec::new(),
                records: Vec::new(),
            });
        }
        let (_, pi) = b.node(&pid).expect("person");
        for m in &mentions {
            let p = &mut b.data.people[pi as usize];
            if p.source == PersonSource::Orcid {
                if !p.records.contains(&m.record) {
                    p.records.push(m.record);
                }
                if !p.genes.contains(&m.gene) {
                    p.genes.push(m.gene.clone());
                }
            }
            let e = NewEdge {
                from: &pid,
                relation: Relation::AuthorOf,
                to: &m.paper,
                kind: EdgeKind::Observed,
                level: LinkLevel::Curated,
                reason: "source author occurrence with ORCID; canonical identity requires accepted gate decision"
                    .into(),
                activity: act,
            };
            b.edge(e, &[m.record]);
        }
    }
    created
}
