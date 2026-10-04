//! Weighted graphs, kNN construction, Louvain communities, modularity and partition overlap.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::similarity::SplitMix;

/// Weighted undirected graph over `0..n`.
#[derive(Clone, Debug, Default)]
pub struct WGraph {
    pub n: usize,
    pub adj: Vec<BTreeMap<usize, f64>>,
}

impl WGraph {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            adj: vec![BTreeMap::new(); n],
        }
    }

    pub fn add_edge(&mut self, a: usize, b: usize, w: f64) {
        if a != b && w > 0.0 {
            self.adj[a].insert(b, w);
            self.adj[b].insert(a, w);
        }
    }

    pub fn edges(&self) -> impl Iterator<Item = (usize, usize, f64)> + '_ {
        self.adj
            .iter()
            .enumerate()
            .flat_map(|(a, m)| m.iter().filter(move |(b, _)| a < **b).map(move |(&b, &w)| (a, b, w)))
    }

    pub(crate) fn degrees(&self) -> Vec<f64> {
        self.adj.iter().map(|m| m.values().sum()).collect()
    }
}

/// kNN graph from a dense weight matrix (row-major `n*n`), ties broken by larger index first.
pub fn knn_graph(w: &[f64], n: usize, nodes: &[usize], k: usize) -> WGraph {
    let mut g = WGraph::new(nodes.len());
    for (a, &x) in nodes.iter().enumerate() {
        let mut nb: Vec<(f64, usize)> = nodes
            .iter()
            .enumerate()
            .filter(|&(b, _)| b != a)
            .map(|(b, &y)| (w[x * n + y], b))
            .collect();
        nb.sort_by(|p, q| q.0.total_cmp(&p.0).then(q.1.cmp(&p.1)));
        for &(wt, b) in nb.iter().take(k) {
            g.add_edge(a, b, wt);
        }
    }
    g
}

/// Louvain modularity optimisation; returns a community id per node (0-based, by first node).
pub fn louvain(g: &WGraph, resolution: f64, seed: u64) -> Vec<usize> {
    let mut rng = SplitMix(seed);
    let k0 = g.degrees();
    let m2: f64 = k0.iter().sum();
    let mut assign: Vec<usize> = (0..g.n).collect();
    if m2 <= 0.0 {
        return assign;
    }
    let mut adj: Vec<BTreeMap<usize, f64>> = g.adj.clone();
    let mut k = k0;
    loop {
        let n = adj.len();
        let mut comm: Vec<usize> = (0..n).collect();
        let mut tot: Vec<f64> = k.clone();
        let mut improved = false;
        loop {
            let mut moved = false;
            let mut order: Vec<usize> = (0..n).collect();
            rng.shuffle(&mut order);
            for &i in &order {
                let ci = comm[i];
                let ki = k[i];
                let mut nw: BTreeMap<usize, f64> = BTreeMap::new();
                for (&j, &w) in &adj[i] {
                    if j != i {
                        *nw.entry(comm[j]).or_default() += w;
                    }
                }
                tot[ci] -= ki;
                let gain = |c: usize, w: f64| w - resolution * tot[c] * ki / m2;
                let mut best = ci;
                let mut best_gain = gain(ci, nw.get(&ci).copied().unwrap_or(0.0));
                for (&c, &w) in &nw {
                    let gc = gain(c, w);
                    if gc > best_gain + 1e-12 {
                        best = c;
                        best_gain = gc;
                    }
                }
                tot[best] += ki;
                if best != ci {
                    comm[i] = best;
                    moved = true;
                    improved = true;
                }
            }
            if !moved {
                break;
            }
        }
        if !improved {
            break;
        }
        // renumber and aggregate
        let mut ids: HashMap<usize, usize> = HashMap::new();
        for &c in &comm {
            let next = ids.len();
            ids.entry(c).or_insert(next);
        }
        let nc = ids.len();
        for a in assign.iter_mut() {
            *a = ids[&comm[*a]];
        }
        let mut nadj: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); nc];
        let mut nk = vec![0.0; nc];
        for i in 0..n {
            let ci = ids[&comm[i]];
            nk[ci] += k[i];
            for (&j, &w) in &adj[i] {
                let cj = ids[&comm[j]];
                if ci != cj {
                    *nadj[ci].entry(cj).or_default() += w;
                }
            }
        }
        if nc == n {
            break;
        }
        adj = nadj;
        k = nk;
    }
    canonical_labels(&assign)
}

/// Relabel so that communities are numbered by first appearance.
pub fn canonical_labels(a: &[usize]) -> Vec<usize> {
    let mut ids: HashMap<usize, usize> = HashMap::new();
    a.iter()
        .map(|c| {
            let next = ids.len();
            *ids.entry(*c).or_insert(next)
        })
        .collect()
}

/// Newman modularity with resolution.
pub fn modularity(g: &WGraph, comm: &[usize], resolution: f64) -> f64 {
    let k = g.degrees();
    let m2: f64 = k.iter().sum();
    if m2 <= 0.0 {
        return 0.0;
    }
    let nc = comm.iter().max().map_or(0, |m| m + 1);
    let (mut inside, mut tot) = (vec![0.0; nc], vec![0.0; nc]);
    for (a, b, w) in g.edges() {
        if comm[a] == comm[b] {
            inside[comm[a]] += 2.0 * w;
        }
    }
    for (i, &c) in comm.iter().enumerate() {
        tot[c] += k[i];
    }
    (0..nc)
        .map(|c| inside[c] / m2 - resolution * (tot[c] / m2).powi(2))
        .sum()
}

pub(crate) fn jaccard(a: &BTreeSet<usize>, b: &BTreeSet<usize>) -> f64 {
    let i = a.intersection(b).count();
    let u = a.len() + b.len() - i;
    if u == 0 { 0.0 } else { i as f64 / u as f64 }
}

pub(crate) fn groups(comm: &[usize], nodes: &[usize]) -> Vec<BTreeSet<usize>> {
    let nc = comm.iter().max().map_or(0, |m| m + 1);
    let mut out = vec![BTreeSet::new(); nc];
    for (i, &c) in comm.iter().enumerate() {
        out[c].insert(nodes[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn louvain_splits_two_cliques() {
        let mut g = WGraph::new(8);
        for a in 0..4 {
            for b in a + 1..4 {
                g.add_edge(a, b, 1.0);
                g.add_edge(a + 4, b + 4, 1.0);
            }
        }
        g.add_edge(3, 4, 0.1);
        let c = louvain(&g, 1.0, 7);
        assert_eq!(c, vec![0, 0, 0, 0, 1, 1, 1, 1]);
        let q = modularity(&g, &c, 1.0);
        assert!(q > 0.45 && q < 0.5, "{q}");
    }
}
