//! Connection discovery shared by the cards API and Ask. This module owns traversal,
//! exact/related classification, evidence selection and unique-work researcher tallies.
//! Callers own wording, JSON and presentation order. No snapshot or data changes occur here.

use std::collections::{BTreeMap, BTreeSet};

use atlas_core::graph::{GraphEdge, LinkLevel, OrgKind, RecordWithhold, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::{Atlas, DiseaseIdx, Graph};

use crate::{GeneEdge, causal_gene_edges, gene_exact, study_score};

/// A retained supporting path; every graph edge has passed the withholding boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum Evidence<'a> {
    Edge(&'a GraphEdge),
    /// Index into [`Connections::genes`], retaining all source links for that gene.
    Gene(usize),
    Hierarchy {
        ancestor: &'a str,
        name: &'a str,
    },
}

/// The strongest explanation path. Other supporting paths remain in `evidence`.
#[derive(Clone, Debug)]
pub enum Origin<'a> {
    Condition(&'a GraphEdge),
    Gene {
        edge: &'a GraphEdge,
        gene: usize,
    },
    Umbrella {
        edge: &'a GraphEdge,
        ancestor: &'a str,
        name: &'a str,
    },
    Grant {
        names_condition: bool,
        gene: Option<usize>,
    },
    Researcher(Researcher<'a>),
}

#[derive(Clone, Debug)]
pub struct Researcher<'a> {
    pub condition_works: usize,
    pub gene_works: usize,
    pub grants: usize,
    pub symbols: Vec<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Found<'a> {
    pub who: NodeKey,
    pub kind: &'static str,
    pub level: LinkLevel,
    pub exact: bool,
    pub score: f64,
    pub evidence: Vec<Evidence<'a>>,
    pub origin: Origin<'a>,
}

pub struct Connections<'a> {
    pub condition: DiseaseIdx,
    pub condition_id: &'a str,
    pub label: &'a str,
    pub genes: Vec<GeneEdge<'a>>,
    pub found: Vec<Found<'a>>,
    pub papers: usize,
    pub grants: usize,
}

pub fn is_card_org(kind: OrgKind) -> bool {
    !matches!(kind, OrgKind::Sponsor | OrgKind::Institution)
}

fn visible_node(graph: &Graph, id: &str) -> Option<NodeKey> {
    graph.node(id).filter(|&k| graph.node_withheld(k).is_none())
}

struct Incoming<'a> {
    who: NodeKey,
    edge: &'a GraphEdge,
    other: &'a str,
}

/// Stable, visible incoming paths. Checking the intermediate node is as important as the edge:
/// a hidden paper must never become a fact or a researcher score in a model's context.
fn incoming<'a>(graph: &'a Graph, id: &str) -> Vec<Incoming<'a>> {
    let mut edges: Vec<_> = graph
        .incident(id)
        .filter_map(|i| {
            if i.outgoing || graph.records_withheld(&i.edge.records).is_some() {
                return None;
            }
            let who = visible_node(graph, i.other)?;
            let edge = graph.edge(i.idx);
            Some(Incoming {
                who,
                edge,
                other: &edge.from,
            })
        })
        .collect();
    edges.sort_by(|a, b| {
        a.edge
            .from
            .cmp(&b.edge.from)
            .then_with(|| a.edge.relation.as_str().cmp(b.edge.relation.as_str()))
            .then_with(|| a.edge.to.cmp(&b.edge.to))
            .then_with(|| a.edge.records.cmp(&b.edge.records))
    });
    edges
}

fn extend_unique<'a>(out: &mut Vec<Evidence<'a>>, evidence: impl IntoIterator<Item = Evidence<'a>>) {
    for e in evidence {
        if !out.contains(&e) {
            out.push(e);
        }
    }
}

/// Keep the strongest classification without discarding another source's supporting path.
fn put<'a>(out: &mut BTreeMap<NodeKey, Found<'a>>, mut f: Found<'a>) {
    match out.remove(&f.who) {
        Some(mut old) => {
            let rank = |x: &Found<'_>| (x.exact, std::cmp::Reverse(x.level));
            if rank(&old) >= rank(&f) {
                extend_unique(&mut old.evidence, f.evidence);
                out.insert(old.who, old);
            } else {
                extend_unique(&mut f.evidence, old.evidence);
                out.insert(f.who, f);
            }
        }
        None => {
            out.insert(f.who, f);
        }
    }
}

fn kind_score(graph: &Graph, who: NodeKey, exact: bool, org_score: f64) -> Option<(&'static str, f64)> {
    match who.kind {
        NodeKind::Study => Some((graph.study(who.idx).kind.as_str(), study_score(graph, who.idx, exact))),
        NodeKind::Organisation if is_card_org(graph.org(who.idx).kind) => {
            Some((graph.org(who.idx).kind.as_str(), org_score))
        }
        _ => None,
    }
}

/// All visible connections to a condition, scored once. Presentation-specific sorting belongs
/// in the adapters. `people=false` avoids researcher tallying for callers that need only assets.
pub fn collect<'a>(atlas: &'a Atlas, graph: &'a Graph, d: DiseaseIdx, people: bool) -> Connections<'a> {
    let disease = atlas.disease_at(d);
    let cid = disease.id.as_str();
    let genes = causal_gene_edges(atlas, d);
    let gene_exact = gene_exact(genes.len());
    let mut by = BTreeMap::new();

    for i in incoming(graph, cid) {
        let who = i.who;
        if !matches!(
            (i.edge.relation, who.kind),
            (Relation::StudiesCondition, NodeKind::Study) | (Relation::ServesCondition, NodeKind::Organisation)
        ) {
            continue;
        }
        let exact = i.edge.level != LinkLevel::Related;
        let Some((kind, score)) = kind_score(graph, who, exact, 2000.0) else {
            continue;
        };
        put(
            &mut by,
            Found {
                who,
                kind,
                level: i.edge.level,
                exact,
                score,
                evidence: vec![Evidence::Edge(i.edge)],
                origin: Origin::Condition(i.edge),
            },
        );
    }
    for (gene, g) in genes.iter().enumerate() {
        for i in incoming(graph, &g.gene_id) {
            let who = i.who;
            if !matches!(
                (i.edge.relation, who.kind),
                (Relation::NamesGene, NodeKind::Study) | (Relation::ServesGene, NodeKind::Organisation)
            ) {
                continue;
            }
            let exact = who.kind == NodeKind::Organisation
                && gene_exact
                && i.edge.level != LinkLevel::Related
                && !serves_other_condition(graph, i.other, cid);
            let Some((kind, score)) = kind_score(graph, who, exact, 1900.0) else {
                continue;
            };
            let level = if who.kind == NodeKind::Study {
                LinkLevel::Gene
            } else {
                i.edge.level
            };
            put(
                &mut by,
                Found {
                    who,
                    kind,
                    level,
                    exact,
                    score,
                    evidence: vec![Evidence::Edge(i.edge), Evidence::Gene(gene)],
                    origin: Origin::Gene { edge: i.edge, gene },
                },
            );
        }
    }
    for (ancestor, name) in graph.umbrella(cid) {
        for i in incoming(graph, ancestor) {
            let who = i.who;
            if !matches!(
                (i.edge.relation, who.kind),
                (Relation::StudiesCondition, NodeKind::Study) | (Relation::ServesCondition, NodeKind::Organisation)
            ) {
                continue;
            }
            let Some((kind, score)) = kind_score(graph, who, false, 500.0) else {
                continue;
            };
            put(
                &mut by,
                Found {
                    who,
                    kind,
                    level: LinkLevel::Umbrella,
                    exact: false,
                    score,
                    evidence: vec![Evidence::Edge(i.edge), Evidence::Hierarchy { ancestor, name }],
                    origin: Origin::Umbrella {
                        edge: i.edge,
                        ancestor,
                        name,
                    },
                },
            );
        }
    }
    let (papers, grants) = works(graph, cid, &genes, people, &mut by);
    let mut found: Vec<_> = by.into_values().collect();
    found.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| graph.node_ref(a.who).id.cmp(&graph.node_ref(b.who).id))
    });
    Connections {
        condition: d,
        condition_id: cid,
        label: &disease.name,
        genes,
        found,
        papers,
        grants,
    }
}

struct Work<'a> {
    names_condition: bool,
    gene: Option<usize>,
    evidence: Vec<Evidence<'a>>,
}

#[derive(Default)]
struct Tally<'a> {
    condition_works: usize,
    gene_works: usize,
    grants: usize,
    latest: u16,
    symbols: BTreeSet<&'a str>,
    works: BTreeSet<NodeKey>,
    sampled: BTreeSet<NodeKey>,
    evidence: Vec<Evidence<'a>>,
}

fn works<'a>(
    graph: &'a Graph,
    cid: &str,
    genes: &[GeneEdge<'a>],
    people: bool,
    out: &mut BTreeMap<NodeKey, Found<'a>>,
) -> (usize, usize) {
    let mut works = BTreeMap::<String, (NodeKey, Work<'a>)>::new();
    for i in incoming(graph, cid) {
        let k = i.who;
        if i.edge.relation != Relation::AboutCondition || !matches!(k.kind, NodeKind::Paper | NodeKind::Grant) {
            continue;
        }
        let (_, w) = works.entry(i.other.to_owned()).or_insert((
            k,
            Work {
                names_condition: true,
                gene: None,
                evidence: Vec::new(),
            },
        ));
        extend_unique(&mut w.evidence, [Evidence::Edge(i.edge)]);
    }
    for (gene, g) in genes.iter().enumerate() {
        for i in incoming(graph, &g.gene_id) {
            let k = i.who;
            if i.edge.relation != Relation::AboutGene || !matches!(k.kind, NodeKind::Paper | NodeKind::Grant) {
                continue;
            }
            let (_, w) = works.entry(i.other.to_owned()).or_insert((
                k,
                Work {
                    names_condition: false,
                    gene: Some(gene),
                    evidence: Vec::new(),
                },
            ));
            w.gene.get_or_insert(gene);
            extend_unique(&mut w.evidence, [Evidence::Edge(i.edge), Evidence::Gene(gene)]);
        }
    }
    let (mut papers, mut grants) = (0, 0);
    let mut tallies = BTreeMap::<String, (NodeKey, Tally<'a>)>::new();
    let mut person_cache = BTreeMap::new();
    for (work_id, (k, w)) in &works {
        let exact = w.names_condition;
        match k.kind {
            NodeKind::Paper => papers += 1,
            NodeKind::Grant => {
                grants += 1;
                let latest = graph.grant(k.idx).fiscal_years.iter().max().copied().unwrap_or(0);
                out.insert(
                    *k,
                    Found {
                        who: *k,
                        kind: "grant",
                        exact,
                        level: if w.names_condition {
                            LinkLevel::Exact
                        } else {
                            LinkLevel::Gene
                        },
                        score: f64::from(u8::from(exact)) * 1000.0 + f64::from(latest),
                        evidence: w.evidence.clone(),
                        origin: Origin::Grant {
                            names_condition: w.names_condition,
                            gene: w.gene,
                        },
                    },
                );
            }
            _ => unreachable!("works are papers or grants"),
        }
        if !people {
            continue;
        }
        for i in incoming(graph, work_id) {
            if !matches!(i.edge.relation, Relation::AuthorOf | Relation::PrincipalInvestigatorOf) {
                continue;
            }
            let Some((person, identity)) = *person_cache
                .entry(i.other.to_owned())
                .or_insert_with(|| canonical_person(graph, i.other))
            else {
                continue;
            };
            let (_, t) = tallies
                .entry(graph.person(person.idx).id.clone())
                .or_insert((person, Tally::default()));
            if t.works.insert(*k) {
                if w.names_condition {
                    t.condition_works += 1;
                } else {
                    t.gene_works += 1;
                }
                if k.kind == NodeKind::Grant {
                    t.grants += 1;
                }
                for evidence in &w.evidence {
                    if let Evidence::Gene(gene) = evidence {
                        t.symbols.insert(genes[*gene].symbol);
                    }
                }
                let year = match k.kind {
                    NodeKind::Paper => graph.paper(k.idx).year.unwrap_or(0),
                    _ => graph.grant(k.idx).fiscal_years.iter().max().copied().unwrap_or(0),
                };
                t.latest = t.latest.max(year);
            }
            // As before, a researcher card samples two work paths for its short explanation.
            // Counts cover every unique visible work; all alternate sources of sampled paths stay.
            if t.sampled.len() < 2 || t.sampled.contains(k) {
                t.sampled.insert(*k);
                extend_unique(&mut t.evidence, [Evidence::Edge(i.edge)]);
                if let Some(e) = identity {
                    extend_unique(&mut t.evidence, [Evidence::Edge(e)]);
                }
                extend_unique(&mut t.evidence, w.evidence.iter().cloned());
            }
        }
    }
    for (_, (who, t)) in tallies {
        let exact = t.condition_works > 0;
        out.insert(
            who,
            Found {
                who,
                kind: "researcher",
                exact,
                level: if t.condition_works > 0 {
                    LinkLevel::Exact
                } else {
                    LinkLevel::Gene
                },
                score: f64::from(u8::from(exact)) * 1000.0
                    + (3 * t.condition_works + t.gene_works + 5 * t.grants) as f64
                    + f64::from(t.latest.saturating_sub(2000)) * 0.1,
                evidence: t.evidence,
                origin: Origin::Researcher(Researcher {
                    condition_works: t.condition_works,
                    gene_works: t.gene_works,
                    grants: t.grants,
                    symbols: t.symbols.into_iter().collect(),
                }),
            },
        );
    }
    (papers, grants)
}

fn serves_other_condition(graph: &Graph, org: &str, cid: &str) -> bool {
    graph.incident(org).any(|i| {
        i.outgoing
            && i.edge.relation == Relation::ServesCondition
            && i.other != cid
            && graph.records_withheld(&i.edge.records).is_none()
    })
}

/// RePORTER aliases count as the visible resolved person. A withheld alias or mapping is never
/// a shortcut into that person's tally, and conflicting aliases are selected in stable ID order.
fn canonical_person<'a>(graph: &'a Graph, id: &str) -> Option<(NodeKey, Option<&'a GraphEdge>)> {
    let original = visible_node(graph, id).filter(|k| k.kind == NodeKind::Person)?;
    let mut mappings: Vec<_> = graph
        .incident(id)
        .filter_map(|i| {
            if i.outgoing || i.edge.relation != Relation::SameAs || graph.records_withheld(&i.edge.records).is_some() {
                return None;
            }
            let who = visible_node(graph, i.other).filter(|k| k.kind == NodeKind::Person)?;
            Some((who, graph.edge(i.idx)))
        })
        .collect();
    mappings.sort_by(|a, b| a.1.from.cmp(&b.1.from).then_with(|| a.1.records.cmp(&b.1.records)));
    if let Some(&(who, edge)) = mappings.first() {
        return Some((who, Some(edge)));
    }
    Some((original, None))
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;
