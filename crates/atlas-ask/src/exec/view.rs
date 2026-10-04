//! Thin adapters over atlas-core that the executors share.
//!
//! Gene edges, their ids and the causal-gene rule come from atlas-journeys (shared with
//! atlas-server), so a cited id opens the same record at `/api/provenance/{id}`.

use atlas_core::disease::PhenotypeEdge;
use atlas_core::graph::{GraphEdge, Person, RecIdx, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::{Atlas, DiseaseIdx, Graph, TermIdx};
pub use atlas_journeys::{GENE_RELATION, MAX_CONDITION_GENES, phenotype_edge_id};

use super::Ctx;

/// One condition–gene edge (all source links for one symbol).
#[derive(Clone, Debug)]
pub struct GeneLinkView {
    pub symbol: String,
    /// Gene node id (HGNC, else NCBIGene, else the symbol).
    pub gene_id: String,
    pub edge_id: String,
    /// Distinct association types, as the sources write them.
    pub associations: Vec<String>,
    pub sources: Vec<String>,
    /// [`atlas_journeys::GeneEdge::is_causal`].
    pub causal: bool,
}

/// Gene edges of a condition, one per symbol, in source order.
pub fn gene_links(atlas: &Atlas, d: DiseaseIdx) -> Vec<GeneLinkView> {
    atlas_journeys::condition_gene_edges(atlas, d)
        .into_iter()
        .map(|e| {
            let first = e.links[0];
            let mut v = GeneLinkView {
                symbol: e.symbol.to_owned(),
                causal: e.is_causal(),
                gene_id: e.gene_id,
                edge_id: e.edge_id,
                associations: vec![first.association.clone()],
                sources: vec![first.source.clone()],
            };
            for link in &e.links[1..] {
                push_unique(&mut v.associations, &link.association);
                push_unique(&mut v.sources, &link.source);
            }
            v
        })
        .collect()
}

fn push_unique(v: &mut Vec<String>, s: &str) {
    if !s.is_empty() && !v.iter().any(|x| x == s) {
        v.push(s.to_owned());
    }
}

pub fn causal_genes(atlas: &Atlas, d: DiseaseIdx) -> Vec<GeneLinkView> {
    gene_links(atlas, d).into_iter().filter(|g| g.causal).collect()
}

/// Plain statement of a condition–gene edge.
pub fn gene_fact_text(atlas: &Atlas, d: DiseaseIdx, g: &GeneLinkView) -> serde_json::Value {
    let dis = atlas.disease_at(d);
    let assoc = g
        .associations
        .iter()
        .map(|a| {
            if a == "MENDELIAN" {
                "Mendelian (single-gene) cause"
            } else {
                a.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    crate::copy::msg(
        "ask.fact.gene_association",
        serde_json::json!({"arg0": g.symbol, "arg1": g.gene_id, "arg2": dis.name, "arg3": dis.id, "assoc": assoc, "arg4": g.sources.join(", ")}),
    )
}

/// Highest frequency on a phenotype edge: (raw text, probability).
pub fn best_frequency(e: &PhenotypeEdge) -> Option<(String, f64)> {
    e.annotations
        .iter()
        .filter_map(|a| {
            let f = a.frequency.as_ref()?;
            let p = match f.cohort {
                Some((n, m)) if m > 0 => n as f64 / m as f64,
                _ => f.value?,
            };
            Some((f.raw.clone(), p))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Plain words for a frequency (HPO classes).
pub fn frequency_words(p: f64) -> serde_json::Value {
    let kind = match p {
        p if p >= 1.0 => "always",
        p if p >= 0.8 => "very_often",
        p if p >= 0.3 => "often",
        p if p >= 0.05 => "sometimes",
        p if p > 0.0 => "rarely",
        _ => "not",
    };
    crate::copy::msg(&format!("ask.frequency.{kind}"), serde_json::json!({}))
}

/// HPO layperson synonym, if any.
pub fn layperson(atlas: &Atlas, t: TermIdx) -> Option<&str> {
    atlas
        .hpo
        .term(t)
        .synonyms
        .iter()
        .find(|s| s.is_kind("layperson"))
        .map(|s| s.text.as_str())
}

/// `HP term name` plus its layperson wording.
pub fn term_text(atlas: &Atlas, t: TermIdx) -> serde_json::Value {
    let term = atlas.hpo.term(t);
    crate::copy::msg(
        "ask.term.label",
        serde_json::json!({"name": term.name, "id": term.id, "plain": layperson(atlas, t).filter(|l| !l.eq_ignore_ascii_case(&term.name))}),
    )
}

/// Plain name of a card kind.
pub fn plain_kind(kind: &str) -> &'static str {
    match kind {
        "trial" => "clinical trial",
        "registry" => "patient registry",
        "natural_history" => "natural-history study",
        "observational" => "observational study",
        "expanded_access" => "expanded-access programme",
        "patient_group" => "patient group",
        "expert_centre" => "expert centre",
        "researcher" => "researcher",
        "grant" => "research project",
        "sponsor" => "study sponsor",
        "institution" => "institution",
        _ => "organisation",
    }
}

pub fn study_contacts_url(nct: &str) -> String {
    format!("https://clinicaltrials.gov/study/{nct}#contacts-and-locations")
}

/// Public profile only (D13): ORCID page, else the PubMed author listing.
pub fn person_url(p: &Person) -> String {
    if let Some(o) = p.orcids.first() {
        return format!("https://orcid.org/{o}");
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
    let enc: String = term
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("https://pubmed.ncbi.nlm.nih.gov/?term={enc}%5Bau%5D")
}

/// `YYYY-MM-DD` of a record's fetch time (else its file's retrieval time).
pub fn checked_on(graph: &Graph, r: RecIdx) -> Option<String> {
    let rec = graph.record(r);
    rec.fetched_at
        .clone()
        .or_else(|| graph.provenance().entity(rec.entity).retrieved_at.clone())
        .map(|t| t.chars().take(10).collect())
}

pub fn relation_phrase(r: Relation) -> serde_json::Value {
    crate::copy::msg(&format!("ask.relation.{}", r.as_str()), serde_json::json!({}))
}

/// Source name of a graph edge, by the node kind it starts from.
pub fn edge_source(ctx: &Ctx<'_>, e: &GraphEdge) -> &'static str {
    match ctx.graph.node(&e.from).map(|k| k.kind) {
        Some(NodeKind::Study) => "ClinicalTrials.gov",
        Some(NodeKind::Grant) => "NIH RePORTER",
        Some(NodeKind::Paper) => "PubMed",
        Some(NodeKind::Organisation) => "organisation list",
        Some(NodeKind::Person) => match e.relation {
            Relation::PrincipalInvestigatorOf => "NIH RePORTER",
            Relation::SameAs => "people identity",
            _ => "PubMed",
        },
        _ => "atlas graph",
    }
}

/// `"<from label> (<from id>) <relation> <to label> (<to id>) [reason]"`.
pub fn edge_text(ctx: &Ctx<'_>, e: &GraphEdge) -> serde_json::Value {
    let side = |id: &str| {
        let l = ctx.label(id);
        if l == id { id.to_owned() } else { format!("{l} ({id})") }
    };
    let reason = if e.reason.trim().is_empty() {
        serde_json::Value::Null
    } else {
        crate::copy::msg(
            "ask.fragment.match_reason",
            serde_json::json!({"reason": e.reason.trim()}),
        )
    };
    crate::copy::msg(
        "ask.fact.recorded_relationship",
        serde_json::json!({"arg0": side(&e.from), "arg1": relation_phrase(e.relation), "arg2": side(&e.to), "reason": reason}),
    )
}

/// First record of a graph node (for "checked on").
pub fn node_checked_on(graph: &Graph, k: NodeKey) -> Option<String> {
    graph.node_records(k).first().and_then(|&r| checked_on(graph, r))
}

/// Up to `n` items, then "and N more".
pub fn list_some(items: &[String], n: usize) -> serde_json::Value {
    crate::copy::msg(
        "ask.list.more",
        serde_json::json!({
            "items": &items[..items.len().min(n)], "remaining": items.len().saturating_sub(n)
        }),
    )
}
