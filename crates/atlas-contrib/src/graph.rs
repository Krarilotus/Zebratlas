//! What the checks need from the curated graph, as a small trait so this crate does not depend on
//! how the graph is stored. The server implements it over atlas-core (feature `core`:
//! [`crate::core_lookup::CoreLookup`]); tests use [`MemoryGraph`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::NodeHit;
use crate::text::normalize;

/// A curated edge, as far as duplicate and conflict detection needs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdgeRef {
    pub from: String,
    pub relation: String,
    pub to: String,
    /// `observed`, `extracted`, `inferred`, `hypothesis`.
    pub kind: String,
}

impl GraphEdgeRef {
    pub fn id(&self) -> String {
        edge_id(&self.from, &self.relation, &self.to)
    }

    pub fn other(&self, id: &str) -> &str {
        if self.from == id { &self.to } else { &self.from }
    }
}

/// `from|relation|to` (same scheme as atlas-core `node::edge_id`).
pub fn edge_id(from: &str, relation: &str, to: &str) -> String {
    format!("{from}|{relation}|{to}")
}

pub fn parse_edge_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut it = id.splitn(3, '|');
    Some((it.next()?, it.next()?, it.next()?))
}

pub trait GraphLookup: Send + Sync {
    /// The node with exactly this id (any kind).
    fn node(&self, id: &str) -> Option<NodeHit>;
    /// Nodes whose names match `text` (search index: ids, names, synonyms, typos), best first.
    /// `kinds` limits the node kinds (empty = any).
    fn search(&self, text: &str, kinds: &[&str], limit: usize) -> Vec<NodeHit>;
    /// Curated edges touching `id`.
    fn edges_of(&self, id: &str) -> Vec<GraphEdgeRef>;
}

/// No graph loaded: every lookup is empty (identity checks then say "not found").
pub struct NoGraph;

impl GraphLookup for NoGraph {
    fn node(&self, _: &str) -> Option<NodeHit> {
        None
    }
    fn search(&self, _: &str, _: &[&str], _: usize) -> Vec<NodeHit> {
        Vec::new()
    }
    fn edges_of(&self, _: &str) -> Vec<GraphEdgeRef> {
        Vec::new()
    }
}

/// A small in-memory graph: tests and demos. Search is exact on normalised names, then token
/// containment.
#[derive(Clone, Debug, Default)]
pub struct MemoryGraph {
    nodes: Vec<NodeHit>,
    names: HashMap<String, Vec<usize>>,
    edges: Vec<GraphEdgeRef>,
}

impl MemoryGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node with its name and synonyms.
    pub fn node(mut self, id: &str, kind: &str, label: &str, synonyms: &[&str], urls: &[&str]) -> Self {
        let idx = self.nodes.len();
        self.nodes.push(NodeHit {
            id: id.into(),
            kind: kind.into(),
            label: label.into(),
            exact: true,
            urls: urls.iter().map(|u| u.to_string()).collect(),
        });
        for name in std::iter::once(&label).chain(synonyms) {
            self.names.entry(normalize(name)).or_default().push(idx);
        }
        self
    }

    pub fn edge(mut self, from: &str, relation: &str, to: &str, kind: &str) -> Self {
        self.edges.push(GraphEdgeRef {
            from: from.into(),
            relation: relation.into(),
            to: to.into(),
            kind: kind.into(),
        });
        self
    }
}

impl GraphLookup for MemoryGraph {
    fn node(&self, id: &str) -> Option<NodeHit> {
        self.nodes.iter().find(|n| n.id == id).cloned()
    }

    fn search(&self, text: &str, kinds: &[&str], limit: usize) -> Vec<NodeHit> {
        let key = normalize(text);
        let ok = |n: &NodeHit| kinds.is_empty() || kinds.contains(&n.kind.as_str());
        let mut out: Vec<NodeHit> = self
            .names
            .get(&key)
            .into_iter()
            .flatten()
            .map(|&i| &self.nodes[i])
            .filter(|n| ok(n))
            .cloned()
            .collect();
        out.dedup_by(|a, b| a.id == b.id);
        if out.is_empty() && !key.is_empty() {
            let padded = format!(" {key} ");
            for n in self.nodes.iter().filter(|n| ok(n)) {
                let name = format!(" {} ", normalize(&n.label));
                if name.contains(&padded) || padded.contains(&name) {
                    out.push(NodeHit {
                        exact: false,
                        ..n.clone()
                    });
                }
            }
        }
        out.truncate(limit);
        out
    }

    fn edges_of(&self, id: &str) -> Vec<GraphEdgeRef> {
        self.edges
            .iter()
            .filter(|e| e.from == id || e.to == id)
            .cloned()
            .collect()
    }
}
