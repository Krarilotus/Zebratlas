//! Breadth-first path over condition–gene edges (atlas) plus the connected graph.

use std::collections::{HashMap, VecDeque};

use atlas_core::{Atlas, DiseaseIdx, Graph};

use crate::genes::condition_gene_edges;

/// One step of a path.
#[derive(Clone, Debug)]
pub enum Hop {
    /// Connected-graph edge index.
    Graph(u32),
    /// Condition–gene edge: (condition, gene symbol).
    Gene(DiseaseIdx, String),
}

pub const MAX_DEPTH: usize = 5;
pub const MAX_VISITED: usize = 400_000;

/// Nodes one step from `id`, with the hop that reaches them.
pub fn neighbours(atlas: &Atlas, graph: &Graph, id: &str) -> Vec<(String, Hop)> {
    let mut out: Vec<(String, Hop)> = graph
        .incident(id)
        .map(|i| (i.other.to_owned(), Hop::Graph(i.idx)))
        .collect();
    if let Some(d) = atlas.disease_idx(id).filter(|_| graph.node(id).is_none()) {
        for e in condition_gene_edges(atlas, d) {
            out.push((e.gene_id, Hop::Gene(d, e.symbol.to_owned())));
        }
    } else if let Some(g) = atlas.gene(id) {
        let gene = atlas.gene_at(g);
        for &d in &gene.diseases {
            out.push((atlas.disease_at(d).id.clone(), Hop::Gene(d, gene.symbol.clone())));
        }
    }
    out
}

/// Shortest chain of at most [`MAX_DEPTH`] hops from `start` to `goal` (visiting at most
/// [`MAX_VISITED`] nodes), in order; `None` if there is none.
pub fn shortest_path(atlas: &Atlas, graph: &Graph, start: &str, goal: &str) -> Option<Vec<Hop>> {
    let mut prev: HashMap<String, (String, Hop)> = HashMap::new();
    let mut depth: HashMap<String, usize> = HashMap::from([(start.to_owned(), 0)]);
    let mut queue = VecDeque::from([start.to_owned()]);
    let mut found = false;
    'bfs: while let Some(id) = queue.pop_front() {
        let dep = depth[&id];
        if dep == MAX_DEPTH {
            continue;
        }
        for (next, hop) in neighbours(atlas, graph, &id) {
            if depth.contains_key(&next) {
                continue;
            }
            depth.insert(next.clone(), dep + 1);
            prev.insert(next.clone(), (id.clone(), hop));
            if next == goal {
                found = true;
                break 'bfs;
            }
            if depth.len() > MAX_VISITED {
                break 'bfs;
            }
            queue.push_back(next);
        }
    }
    if !found {
        return None;
    }
    let mut hops = Vec::new();
    let mut at = goal.to_owned();
    while let Some((from, hop)) = prev.get(&at) {
        hops.push(hop.clone());
        at = from.clone();
    }
    hops.reverse();
    Some(hops)
}
