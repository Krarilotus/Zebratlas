//! Node lookup across the atlas (conditions, genes, phenotypes) and the connected graph (studies,
//! grants, papers, people, organisations); reach channels and dates shared by the journey views.

use atlas_core::graph::{OrgKind, Person, RecIdx};
use atlas_core::node::{NodeKey, NodeKind, NodeRef};
use atlas_core::{Atlas, DiseaseIdx, Graph};
use serde_json::{Value, json};

use crate::views;

/// Node of any kind by id (canonical or source id for conditions; symbol or id for genes).
pub fn any_ref(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeRef> {
    if let Some(k) = graph.node(id) {
        return Some(graph.node_ref(k));
    }
    if let Some(d) = atlas.disease_idx(id) {
        return Some(atlas.disease_ref(d));
    }
    if let Some(g) = atlas.gene(id) {
        return Some(atlas.node_ref(NodeKey {
            kind: NodeKind::Gene,
            idx: g,
        }));
    }
    atlas.hpo.canonical(id.trim()).map(|t| atlas.term_ref(t))
}

/// Active condition by any id, else a 404 message.
pub fn condition(atlas: &Atlas, id: &str) -> Result<DiseaseIdx, Value> {
    let idx = atlas
        .disease_idx(id)
        .ok_or_else(|| crate::copy_extra::msg("api.error.condition_unknown", json!({"id": id})))?;
    if !atlas.disease_at(idx).is_active() {
        return Err(crate::copy_extra::msg("api.error.condition_retired", json!({"id": id})));
    }
    Ok(idx)
}

/// `YYYY-MM-DD` of an RFC 3339 time.
pub fn day(t: &str) -> String {
    t.chars().take(10).collect()
}

/// Date a record was fetched (else its file's retrieval date).
pub fn checked_on(graph: &Graph, r: RecIdx) -> Option<String> {
    let rec = graph.record(r);
    rec.fetched_at
        .clone()
        .or_else(|| graph.provenance().entity(rec.entity).retrieved_at.clone())
        .map(|t| day(&t))
}

/// Causal gene edges of a condition ([`atlas_journeys::causal_gene_edges`]). Returns
/// `(gene id, gene label, atlas edge id)`, one per symbol.
pub fn causal_genes(atlas: &Atlas, d: DiseaseIdx) -> Vec<(String, String, String)> {
    atlas_journeys::causal_gene_edges(atlas, d)
        .into_iter()
        .map(|ge| {
            let node = views::gene_ref(atlas, ge.links[0]);
            (node.id, node.label, ge.edge_id)
        })
        .collect()
}

/// Minimal percent-encoding for URL query values.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Path segments must encode a space as `%20`; `+` only means a space in query values.
pub fn encode_path(s: &str) -> String {
    encode(s).replace('+', "%20")
}

/// A reach channel's label as a catalog message (`channel.<kind>`, D26).
fn channel(kind: &str) -> serde_json::Value {
    crate::copy::msg(&format!("channel.{kind}"), json!({}))
}

/// The study's central contact as published on ClinicalTrials.gov (when the contacts cache has it),
/// else the study's contacts section.
pub fn study_reach(graph: &Graph, nct: &str) -> Value {
    let url = format!("https://clinicaltrials.gov/study/{nct}#contacts-and-locations");
    match graph.contacts(nct).filter(|c| !c.central.is_empty()) {
        Some(c) => {
            let rec = graph.record(c.record);
            let e = graph.provenance().entity(rec.entity);
            json!({
                "kind": "ctgov_central_contact",
                "channel": channel("ctgov_central_contact")["fallback"],
                "channel_msg": channel("ctgov_central_contact"),
                "url": url,
                "contacts": c.central,
                "record": format!("{}#{}", e.file, rec.locator),
                "record_sha256": atlas_core::graph::hex(&rec.sha256),
                "checked_on": rec.fetched_at.as_deref().map(day),
            })
        }
        None => json!({
            "kind": "ctgov_contacts",
            "channel": channel("ctgov_contacts")["fallback"],
            "channel_msg": channel("ctgov_contacts"),
            "url": url,
        }),
    }
}

pub fn grant_reach(url: &str) -> Value {
    json!({
        "kind": "reporter_project",
        "channel": channel("reporter_project")["fallback"],
        "channel_msg": channel("reporter_project"),
        "url": url,
    })
}

/// Public profile only (D13): ORCID page, else the person's PubMed author listing.
pub fn person_reach(p: &Person) -> Value {
    if let Some(o) = p.orcids.first() {
        return json!({
            "kind": "orcid",
            "channel": channel("orcid")["fallback"],
            "channel_msg": channel("orcid"),
            "url": format!("https://orcid.org/{o}"),
        });
    }
    let words: Vec<&str> = p.name.split_whitespace().collect();
    let term = match words.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first @ .., last] => {
            let initials: String = first.iter().filter_map(|w| w.chars().next()).collect();
            format!("{last} {initials}")
        }
    };
    json!({
        "kind": "pubmed_author",
        "channel": channel("pubmed_author")["fallback"],
        "channel_msg": channel("pubmed_author"),
        "url": format!("https://pubmed.ncbi.nlm.nih.gov/?term={}%5Bau%5D", encode(&term)),
    })
}

pub fn org_reach(url: Option<&str>, contact: Option<&str>, kind: OrgKind) -> Value {
    match (contact, url) {
        (Some(c), _) => json!({ "kind": "contact_form", "channel": channel("contact_form")["fallback"],
            "channel_msg": channel("contact_form"), "url": c }),
        (None, Some(u)) => json!({ "kind": "website", "channel": channel("website")["fallback"],
            "channel_msg": channel("website"), "url": u }),
        (None, None) => {
            let m = channel(if kind == OrgKind::Sponsor {
                "none_sponsor"
            } else {
                "none"
            });
            json!({ "kind": "none", "channel": m["fallback"], "channel_msg": m, "url": null })
        }
    }
}

#[cfg(test)]
mod encoding_tests {
    #[test]
    fn path_segments_keep_spaces_and_literal_pluses_distinct() {
        assert_eq!(super::encode("a b+c"), "a+b%2Bc");
        assert_eq!(super::encode_path("a b+c"), "a%20b%2Bc");
    }
}
