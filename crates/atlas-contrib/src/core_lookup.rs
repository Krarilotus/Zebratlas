//! [`GraphLookup`] over atlas-core (feature `core`): conditions and genes through the atlas search
//! index, organisations, studies and people through the connected graph.
//!
//! The server holds the atlas inside its own state, so the adapter takes the shared state plus two
//! projections:
//!
//! ```ignore
//! let lookup = CoreLookup::new(app.clone(), |s| s.atlas(), |s| s.graph());
//! ```

use std::sync::Arc;

use atlas_core::Atlas;
use atlas_core::graph::Graph;
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::search::{SearchOptions, Tier};

use crate::graph::{GraphEdgeRef, GraphLookup};
use crate::model::NodeHit;
use crate::text::normalize;

pub struct CoreLookup<T: Send + Sync + 'static> {
    src: Arc<T>,
    atlas: fn(&T) -> &Atlas,
    graph: fn(&T) -> Option<&Graph>,
}

impl<T: Send + Sync + 'static> CoreLookup<T> {
    pub fn new(src: Arc<T>, atlas: fn(&T) -> &Atlas, graph: fn(&T) -> Option<&Graph>) -> Self {
        Self { src, atlas, graph }
    }

    fn atlas(&self) -> &Atlas {
        (self.atlas)(&self.src)
    }

    fn graph(&self) -> Option<&Graph> {
        (self.graph)(&self.src)
    }
}

fn kind_str(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Disease => "disease",
        NodeKind::Gene => "gene",
        NodeKind::Phenotype => "phenotype",
        NodeKind::Pathway => "pathway",
        NodeKind::Paper => "paper",
        NodeKind::Study => "study",
        NodeKind::Grant => "grant",
        NodeKind::Person => "person",
        NodeKind::Organisation => "organisation",
        NodeKind::Asset => "asset",
    }
}

fn graph_hit(g: &Graph, key: NodeKey, exact: bool) -> NodeHit {
    let r = g.node_ref(key);
    let urls = match key.kind {
        NodeKind::Organisation => {
            let o = g.org(key.idx);
            o.url.iter().chain(&o.contact_url).cloned().collect()
        }
        _ => Vec::new(),
    };
    NodeHit {
        id: r.id,
        kind: kind_str(key.kind).into(),
        label: r.label,
        exact,
        urls,
    }
}

impl<T: Send + Sync + 'static> GraphLookup for CoreLookup<T> {
    fn node(&self, id: &str) -> Option<NodeHit> {
        let atlas = self.atlas();
        if let Some(d) = atlas.disease(id) {
            return Some(NodeHit {
                id: d.id.clone(),
                kind: "disease".into(),
                label: d.name.clone(),
                exact: true,
                urls: Vec::new(),
            });
        }
        if let Some(gi) = atlas.gene(id) {
            let g = atlas.gene_at(gi);
            if g.id() == id {
                return Some(NodeHit {
                    id: g.id().to_owned(),
                    kind: "gene".into(),
                    label: g.symbol.clone(),
                    exact: true,
                    urls: Vec::new(),
                });
            }
        }
        let g = self.graph()?;
        g.node(id).map(|key| graph_hit(g, key, true))
    }

    fn search(&self, text: &str, kinds: &[&str], limit: usize) -> Vec<NodeHit> {
        let want = |k: &str| kinds.is_empty() || kinds.contains(&k);
        let mut out: Vec<NodeHit> = Vec::new();
        let atlas = self.atlas();
        if want("disease") || want("gene") || want("phenotype") {
            let opts = SearchOptions {
                limit: limit * 4,
                ..SearchOptions::default()
            };
            for hit in atlas.search().search(text, opts) {
                let kind = kind_str(hit.node.kind);
                if !want(kind) || out.iter().any(|h| h.id == atlas.node_ref(hit.node).id) {
                    continue;
                }
                let r = atlas.node_ref(hit.node);
                out.push(NodeHit {
                    id: r.id,
                    kind: kind.into(),
                    label: r.label,
                    exact: hit.tier == Tier::Exact,
                    urls: Vec::new(),
                });
            }
        }
        if let Some(g) = self.graph() {
            let key = normalize(text);
            let padded = format!(" {key} ");
            let data = g.data();
            let mut scan = |kind: NodeKind, names: Vec<(u32, Vec<&str>)>| {
                if !want(kind_str(kind)) || key.is_empty() {
                    return;
                }
                for (idx, ns) in names {
                    let norm: Vec<String> = ns.iter().map(|n| normalize(n)).collect();
                    let exact = norm.contains(&key);
                    let partial = !exact && norm.iter().any(|n| format!(" {n} ").contains(&padded));
                    if exact || partial {
                        out.push(graph_hit(g, NodeKey { kind, idx }, exact));
                    }
                }
            };
            scan(
                NodeKind::Organisation,
                data.orgs
                    .iter()
                    .enumerate()
                    .map(|(i, o)| (i as u32, vec![o.name.as_str()]))
                    .collect(),
            );
            scan(
                NodeKind::Study,
                data.studies
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as u32, vec![s.title.as_str(), s.id.as_str()]))
                    .collect(),
            );
            scan(
                NodeKind::Person,
                data.people
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        (
                            i as u32,
                            std::iter::once(p.name.as_str())
                                .chain(p.name_variants.iter().map(String::as_str))
                                .collect(),
                        )
                    })
                    .collect(),
            );
        }
        // Exact matches first, stable otherwise.
        out.sort_by_key(|h| !h.exact);
        out.truncate(limit);
        out
    }

    fn edges_of(&self, id: &str) -> Vec<GraphEdgeRef> {
        let Some(g) = self.graph() else { return Vec::new() };
        g.incident(id)
            .map(|inc| GraphEdgeRef {
                from: inc.edge.from.clone(),
                relation: inc.edge.relation.as_str().into(),
                to: inc.edge.to.clone(),
                kind: inc.edge.kind.as_str().into(),
            })
            .collect()
    }
}
