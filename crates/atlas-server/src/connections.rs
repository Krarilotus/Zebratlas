//! Who connects to a condition (J1.3, J1.4, J2.3, J2.4, J3.1): studies, patient groups, expert
//! centres, grants and researchers, each with the edges that justify it.
//!
//! Exact support requires a source link to the diagnosis for studies, grants and researchers.
//! Gene-only paths stay related. Organisations may be exact through a bounded causal-gene link;
//! umbrella links and gene links for broad clinical entities remain related.

use crate::copy::msg;
use atlas_core::graph::{LinkLevel, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::{Atlas, DiseaseIdx, Graph};
pub use atlas_journeys::connections::is_card_org;
use atlas_journeys::connections::{self as discovery, Evidence, Origin};
use serde_json::{Value, json};

/// Card kinds, in display order.
pub const KINDS: [&str; 10] = [
    "patient_group",
    "expert_centre",
    "registry",
    "natural_history",
    "trial",
    "observational",
    "expanded_access",
    "researcher",
    "grant",
    "organisation",
];

/// One citable statement behind a card (`key` = edge id, or node id for node attributes).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Fact {
    pub key: String,
    pub text: String,
    pub msg: Value,
    /// The edge, when the fact is an edge (`/api/provenance/{id}`, `/api/verify/{id}`).
    pub edge_id: Option<String>,
    pub source: String,
}

impl Fact {
    fn edge(id: String, message: Value, source: &str) -> Self {
        Self {
            key: id.clone(),
            text: message["fallback"].as_str().unwrap_or_default().to_owned(),
            msg: message,
            edge_id: Some(id),
            source: source.to_owned(),
        }
    }

    fn node(id: &str, message: Value, source: &str) -> Self {
        Self {
            key: id.to_owned(),
            text: message["fallback"].as_str().unwrap_or_default().to_owned(),
            msg: message,
            edge_id: None,
            source: source.to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub kind: &'static str,
    pub who: NodeKey,
    pub level: LinkLevel,
    pub exact: bool,
    pub facts: Vec<Fact>,
    /// Plain template sentence as a catalog message `{key, params, fallback}` (D26); the LLM
    /// sentence is optional (`explain=`).
    pub why: Value,
    /// Higher first.
    pub score: f64,
}

pub struct Connections {
    pub condition: DiseaseIdx,
    pub label: String,
    /// Causal genes: (id, symbol, atlas edge id).
    pub genes: Vec<(String, String, String)>,
    pub found: Vec<Found>,
    /// Works (papers + grants) linked to the condition or its genes.
    pub papers: usize,
    pub grants: usize,
}

/// Presentation adapter over the shared journey discovery engine.
pub fn collect(atlas: &Atlas, graph: &Graph, d: DiseaseIdx) -> Connections {
    let c = discovery::collect(atlas, graph, d, true);
    let found = c
        .found
        .iter()
        .map(|f| Found {
            who: f.who,
            kind: f.kind,
            level: f.level,
            exact: f.exact,
            score: f.score,
            facts: f.evidence.iter().map(|e| fact(graph, &c, e)).collect(),
            why: why(graph, &c, f),
        })
        .collect();
    Connections {
        condition: c.condition,
        label: c.label.to_owned(),
        genes: c
            .genes
            .iter()
            .map(|g| (g.gene_id.clone(), g.symbol.to_owned(), g.edge_id.clone()))
            .collect(),
        found,
        papers: c.papers,
        grants: c.grants,
    }
}

fn fact(graph: &Graph, c: &discovery::Connections<'_>, evidence: &Evidence<'_>) -> Fact {
    let cid = match evidence {
        Evidence::Edge(e) => e.to.as_str(),
        _ => "",
    };
    let label = graph
        .umbrella(c.condition_id)
        .iter()
        .find_map(|(ancestor, name)| (ancestor.as_str() == cid).then_some(name.as_str()))
        .unwrap_or(c.label);
    match evidence {
        Evidence::Gene(gene) => {
            let g = &c.genes[*gene];
            Fact::edge(
                g.edge_id.clone(),
                crate::copy_extra::msg("facts.gene_condition", json!({"symbol": g.symbol, "label": c.label})),
                "Orphanet / OMIM",
            )
        }
        Evidence::Hierarchy { ancestor, name } => Fact::node(
            c.condition_id,
            crate::copy_extra::msg(
                "facts.condition_parent",
                json!({"label": c.label, "anc_name": name, "anc": ancestor}),
            ),
            "MONDO",
        ),
        Evidence::Edge(e) => {
            let who = graph.node(&e.from).expect("discovery's visible node");
            let (text, source) = match e.relation {
                Relation::StudiesCondition => {
                    let s = graph.study(who.idx);
                    let text = if e.level == LinkLevel::Mesh && e.to == c.condition_id {
                        crate::copy_extra::msg(
                            "facts.study_indexed",
                            json!({"arg0": s.title, "arg1": s.id, "arg2": e.reason.trim_start_matches("MeSH "), "label": label}),
                        )
                    } else {
                        crate::copy_extra::msg(
                            "facts.study_registered",
                            json!({"arg0": s.title, "arg1": s.id, "arg2": e.reason, "label": label}),
                        )
                    };
                    (text, "ClinicalTrials.gov")
                }
                Relation::NamesGene => {
                    let s = graph.study(who.idx);
                    (
                        crate::copy_extra::msg(
                            "facts.study_record",
                            json!({"arg0": s.title, "arg1": s.id, "arg2": e.reason}),
                        ),
                        "ClinicalTrials.gov",
                    )
                }
                Relation::ServesCondition => (
                    crate::copy_extra::msg(
                        "facts.organisation_condition",
                        json!({"arg0": graph.org(who.idx).name, "label": label}),
                    ),
                    "organisation list",
                ),
                Relation::ServesGene => {
                    let symbol = c
                        .genes
                        .iter()
                        .find(|g| g.gene_id == e.to)
                        .map_or(e.to.as_str(), |g| g.symbol);
                    (
                        crate::copy_extra::msg(
                            "facts.organisation_gene",
                            json!({"arg0": graph.org(who.idx).name, "symbol": symbol}),
                        ),
                        "organisation list",
                    )
                }
                Relation::AboutCondition => (
                    crate::copy_extra::msg("facts.work_record", json!({"arg0": e.from, "arg1": e.reason})),
                    source_of(who.kind),
                ),
                Relation::AboutGene => {
                    let symbol = c
                        .genes
                        .iter()
                        .find(|g| g.gene_id == e.to)
                        .map_or(e.to.as_str(), |g| g.symbol);
                    (
                        crate::copy_extra::msg(
                            "facts.work_gene",
                            json!({"arg0": e.from, "symbol": symbol, "arg1": e.reason}),
                        ),
                        source_of(who.kind),
                    )
                }
                Relation::AuthorOf | Relation::PrincipalInvestigatorOf => {
                    let role = if e.relation == Relation::AuthorOf {
                        "facts.person_author"
                    } else {
                        "facts.person_lead"
                    };
                    let work_kind = graph.node(&e.to).expect("visible work").kind;
                    (
                        crate::copy_extra::msg(role, json!({"name": graph.node_ref(who).label, "arg0": e.to})),
                        source_of(work_kind),
                    )
                }
                Relation::SameAs => {
                    let other = graph.node(&e.to).expect("visible alias");
                    (
                        crate::copy_extra::msg(
                            "facts.person_same",
                            json!({"name": graph.node_ref(who).label, "other": graph.node_ref(other).label}),
                        ),
                        "people identity",
                    )
                }
                _ => unreachable!("connection evidence relation"),
            };
            Fact::edge(e.id(), text, source)
        }
    }
}

fn why(graph: &Graph, c: &discovery::Connections<'_>, f: &discovery::Found<'_>) -> Value {
    match &f.origin {
        Origin::Condition(_) => match f.who.kind {
            NodeKind::Study => msg(
                "why.study_registered",
                json!({"kind_label": plain_kind(f.kind), "condition": c.label}),
            ),
            _ => msg(
                "why.org_condition",
                json!({"organisation": graph.org(f.who.idx).name, "condition": c.label}),
            ),
        },
        Origin::Gene { gene, .. } => {
            let symbol = c.genes[*gene].symbol;
            match f.who.kind {
                NodeKind::Study => msg(
                    "why.study_gene",
                    json!({"kind_label": plain_kind(f.kind), "gene": symbol, "condition": c.label}),
                ),
                _ => msg(
                    "why.org_gene",
                    json!({"organisation": graph.org(f.who.idx).name, "gene": symbol, "condition": c.label}),
                ),
            }
        }
        Origin::Umbrella { name, .. } => match f.who.kind {
            NodeKind::Study => msg(
                "why.study_broader",
                json!({"kind_label": plain_kind(f.kind), "group": name, "condition": c.label}),
            ),
            _ => msg(
                "why.org_broader",
                json!({"organisation": graph.org(f.who.idx).name, "group": name, "condition": c.label}),
            ),
        },
        Origin::Grant { names_condition, gene } => {
            let about = if *names_condition {
                c.label
            } else {
                gene.map(|g| c.genes[g].symbol).unwrap_or_default()
            };
            msg(
                "why.grant",
                json!({"organisation": graph.grant(f.who.idx).organisation, "about": about}),
            )
        }
        Origin::Researcher(t) => {
            let about = if t.condition_works > 0 {
                c.label.to_owned()
            } else {
                t.symbols.join(", ")
            };
            msg(
                "why.researcher",
                json!({"name": graph.person(f.who.idx).name, "papers": t.condition_works + t.gene_works - t.grants, "about": about, "grants": t.grants}),
            )
        }
    }
}

fn source_of(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Paper => "PubMed",
        NodeKind::Grant => "NIH RePORTER",
        _ => "atlas",
    }
}

/// Plain name of a card kind for template sentences.
pub fn plain_kind(kind: &str) -> &'static str {
    match kind {
        "trial" => "clinical trial",
        "registry" => "patient registry",
        "natural_history" => "natural history study",
        "observational" => "observational study",
        "expanded_access" => "early access programme",
        "patient_group" => "patient group",
        "expert_centre" => "specialist centre",
        "researcher" => "researcher",
        "grant" => "research project",
        _ => "organisation",
    }
}

/// Kinds a `kind=` value selects (`study` = every study kind, `organisation` = every organisation).
pub fn expand_kinds(param: Option<&str>) -> Vec<&'static str> {
    let Some(p) = param.filter(|p| !p.trim().is_empty()) else {
        return KINDS.to_vec();
    };
    let mut out = Vec::new();
    for k in p.split(',').map(str::trim) {
        let add: &[&'static str] = match k {
            "study" | "studies" => &[
                "registry",
                "natural_history",
                "trial",
                "observational",
                "expanded_access",
            ],
            "organisation" | "organization" | "org" => &["patient_group", "expert_centre", "organisation"],
            "people" | "person" => &["researcher"],
            other => KINDS
                .iter()
                .find(|x| **x == other)
                .map(std::slice::from_ref)
                .unwrap_or(&[]),
        };
        for a in add {
            if !out.contains(a) {
                out.push(*a);
            }
        }
    }
    out
}
