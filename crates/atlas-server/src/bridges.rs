//! Bridge people for research questions, from `people.persons` v1 (`data/cache/people/persons.json`,
//! `header.bridges`; docs/design/sources/people.md). Read once, lazily.
//!
//! Wording rule (adopted): (a) `facts[]`: what they worked on, cited (grants, papers, trials);
//! (b) `suggestion`: why that might matter, labelled `kind = "hypothesis"` (our suggestion), never
//! "could run the experiment" as a fact. Identity: id-backed = "same person"; candidate groups =
//! "possibly the same person" with reasons. Contact via institutional pages only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use atlas_core::node::NodeRef;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct File {
    header: Header,
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    retrieved_at: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    bridges: Vec<Bridge>,
}

#[derive(Clone, Deserialize)]
pub struct Bridge {
    pub identity: String,
    /// Null for a candidate group (no id-backed person); then `candidate_group` identifies it.
    #[serde(default)]
    pub person: Option<String>,
    #[serde(default)]
    pub candidate_group: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub countries: Vec<String>,
    #[serde(default)]
    pub affiliations: Vec<String>,
    /// gene → source type → record ids.
    #[serde(default)]
    pub genes: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub close_links: Vec<Value>,
}

pub struct People {
    bridges: Vec<Bridge>,
    source: Value,
}

/// `None` when the file is absent or unreadable (logged once).
pub fn load(data: &Path) -> Option<&'static People> {
    static P: OnceLock<Option<People>> = OnceLock::new();
    P.get_or_init(|| {
        let path = data.join("cache/people/persons.json");
        let bytes = std::fs::read(&path).ok()?;
        match serde_json::from_slice::<File>(&bytes) {
            Ok(f) => Some(People {
                source: json!({
                    "file": "data/cache/people/persons.json", "schema": "people.persons v1",
                    "retrieved_at": f.header.retrieved_at, "sha256": f.header.sha256,
                    "doc": "docs/design/sources/people.md",
                }),
                bridges: f.header.bridges,
            }),
            Err(e) => {
                eprintln!("people bridges unavailable: {e}");
                None
            }
        }
    })
    .as_ref()
}

use crate::copy::sentence as msg;

/// Bridges whose named evidence spans ≥2 of the question's communities (by causal gene).
/// `communities`: (condition, its causal gene symbols).
pub fn for_question(people: &People, communities: &[(NodeRef, Vec<String>)], limit: usize) -> Vec<Value> {
    let mut rows: Vec<(bool, usize, Value)> = Vec::new();
    for b in &people.bridges {
        let reached: Vec<&NodeRef> = communities
            .iter()
            .filter(|(_, genes)| genes.iter().any(|g| b.genes.contains_key(g)))
            .map(|(n, _)| n)
            .collect();
        if reached.len() < 2 {
            continue;
        }
        let genes: BTreeSet<&String> = communities
            .iter()
            .flat_map(|(_, g)| g)
            .filter(|g| b.genes.contains_key(*g))
            .collect();
        let institution = b.affiliations.first().cloned().unwrap_or_default();
        let facts: Vec<Value> = b
            .genes
            .iter()
            .map(|(gene, by)| {
                let n = |k: &str| by.get(k).map_or(0, Vec::len);
                let cites: Vec<String> = by.values().flatten().cloned().collect();
                msg(
                    "questions.bridge.worked_on",
                    json!({ "gene": gene, "grants": n("grant"), "papers": n("paper"), "trials": n("trial") }),
                    cites,
                )
            })
            .collect();
        let exact = b.identity == "exact";
        let identity = if exact {
            msg("questions.bridge.identity.same", json!({}), vec![])
        } else {
            msg(
                "questions.bridge.identity.possible",
                json!({ "links": b.close_links.len() }),
                vec![],
            )
        };
        let gene_list: Vec<&str> = genes.iter().map(|g| g.as_str()).collect();
        let mut suggestion = msg(
            "questions.bridge.suggestion",
            json!({ "genes": gene_list, "institution": institution }),
            vec![],
        );
        suggestion["kind"] = json!("hypothesis");
        rows.push((
            exact,
            genes.len(),
            json!({
                "person": { "id": b.person.as_ref().or(b.candidate_group.as_ref()), "kind": "person", "label": b.name },
                "identity": if exact { "exact" } else { "candidate" },
                "identity_note": identity,
                "close_links": b.close_links,
                "institution": institution,
                "affiliations": b.affiliations,
                "countries": b.countries,
                "communities": reached,
                "genes": genes,
                "works": [],
                "facts": facts,
                "suggestion": suggestion,
                "contact": { "via": "institutional_page", "institution": institution },
                "source": people.source,
            }),
        ));
    }
    rows.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    rows.into_iter().take(limit).map(|r| r.2).collect()
}
