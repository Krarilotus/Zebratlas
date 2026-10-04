//! Clusters of a disease slice: kNN similarity graph -> Louvain communities -> per-cluster label,
//! members, the edges that hold it together, bridges, counterexamples and stability.
//!
//! Port of `explore_clusters.py` (k = 8 nearest neighbours by combined score, undirected union,
//! Louvain at resolution 1). Louvain is implemented here (deterministic, seeded node order);
//! networkx's RNG cannot be reproduced, so cluster ids differ from the prototype, sizes are close.
//!
//! Stability (reported, never tuned on):
//! - `seed`: mean best-match Jaccard of each cluster over Louvain runs with other seeds;
//! - `bootstrap`: Hennig's clusterwise Jaccard over runs on random 80 % node subsamples (the kNN
//!   graph is rebuilt on each subsample). > 0.75 stable, 0.6-0.75 a pattern, < 0.5 dissolved.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use atlas_core::DiseaseIdx;
use atlas_core::mechanism::ProcessKind;
use atlas_core::node::NodeRef;
use serde::Serialize;

use crate::similarity::{RunProvenance, SimilarityIndex, SplitMix};

mod graph;
mod slice;

pub use graph::{WGraph, canonical_labels, knn_graph, louvain, modularity};
use graph::{groups, jaccard};
pub use slice::{ACTIVITY_SLICE, DEE_ROOTS, SEED_GENE, Slice, VESICLE_CYCLE, dee_slice};

pub const ACTIVITY_CLUSTERS: &str = "activity:similarity-clusters";

#[derive(Clone, Debug, Serialize)]
pub struct ClusterParams {
    pub k: usize,
    pub resolution: f64,
    pub seed: u64,
    pub stability_runs: usize,
    pub bootstrap_fraction: f64,
    /// Edges / bridges listed per cluster.
    pub shown: usize,
}

impl Default for ClusterParams {
    fn default() -> Self {
        Self {
            k: 8,
            resolution: 1.0,
            seed: 7,
            stability_runs: 30,
            bootstrap_fraction: 0.8,
            shown: 8,
        }
    }
}

/// Pairwise scores of a slice (dense, symmetric).
#[derive(Clone, Debug)]
pub struct PairMatrix {
    pub members: Vec<DiseaseIdx>,
    pub phenotype: Vec<f64>,
    pub mechanism: Vec<f64>,
    pub conflict: Vec<bool>,
}

impl PairMatrix {
    pub fn new(index: &SimilarityIndex, members: &[DiseaseIdx]) -> Self {
        let n = members.len();
        let mut phenotype = vec![0.0; n * n];
        let mut mechanism = vec![0.0; n * n];
        let mut conflict = vec![false; n * n];
        for a in 0..n {
            for b in a + 1..n {
                let s = index.score(members[a], members[b]);
                for (x, y) in [(a, b), (b, a)] {
                    phenotype[x * n + y] = s.phenotype;
                    mechanism[x * n + y] = s.mechanism;
                    conflict[x * n + y] = s.conflict;
                }
            }
        }
        Self {
            members: members.to_vec(),
            phenotype,
            mechanism,
            conflict,
        }
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn combined(&self, alpha: f64) -> Vec<f64> {
        self.phenotype
            .iter()
            .zip(&self.mechanism)
            .map(|(p, m)| alpha * p + (1.0 - alpha) * m)
            .collect()
    }
}

/// Partition only (for validation runs and stability).
pub fn partition(weights: &[f64], n: usize, k: usize, resolution: f64, seed: u64) -> (Vec<usize>, f64) {
    let nodes: Vec<usize> = (0..n).collect();
    let g = knn_graph(weights, n, &nodes, k);
    let comm = louvain(&g, resolution, seed);
    let q = modularity(&g, &comm, resolution);
    (comm, q)
}

#[derive(Clone, Debug, Serialize)]
pub struct ClusterEdge {
    pub a: NodeRef,
    pub b: NodeRef,
    pub weight: f64,
    pub phenotype: f64,
    pub mechanism: f64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Bridge {
    pub to_cluster: String,
    pub via: ClusterEdge,
}

#[derive(Clone, Debug, Serialize)]
pub struct Enriched {
    pub id: String,
    pub name: String,
    pub source: &'static str,
    pub in_cluster: usize,
    pub elsewhere: usize,
    pub ic: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClusterCounterexample {
    pub member: NodeRef,
    pub other: NodeRef,
    pub other_cluster: String,
    pub why: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stability {
    /// Mean best-match Jaccard over Louvain seeds.
    pub seed: f64,
    /// Hennig clusterwise Jaccard over 80 % subsamples.
    pub bootstrap: f64,
    /// `stable` / `pattern` / `weak` / `dissolved` from the bootstrap value.
    pub verdict: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cluster {
    pub id: String,
    pub label: String,
    pub why: Vec<String>,
    pub members: Vec<NodeRef>,
    pub genes: Vec<(String, usize)>,
    pub processes: Vec<Enriched>,
    pub phenotypes: Vec<Enriched>,
    pub edges: Vec<ClusterEdge>,
    pub bridges: Vec<Bridge>,
    pub counterexamples: Vec<ClusterCounterexample>,
    pub stability: Stability,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClusterReport {
    pub slice_rule: Vec<String>,
    pub slice_size: usize,
    pub params: ClusterParams,
    pub alpha: f64,
    pub modularity: f64,
    pub clusters: Vec<Cluster>,
    /// Mean ARI of the seed runs / bootstrap runs against the reported partition.
    pub seed_ari: f64,
    pub bootstrap_ari: f64,
    pub provenance: RunProvenance,
    #[serde(skip)]
    pub assignment: Vec<usize>,
    #[serde(skip)]
    pub members: Vec<DiseaseIdx>,
}

impl ClusterReport {
    /// The cluster containing a disease (any id the atlas resolves).
    pub fn cluster_of(&self, index: &SimilarityIndex, id: &str) -> Option<&Cluster> {
        let d = index.resolve(id)?;
        let pos = self.members.iter().position(|&m| m == d)?;
        self.clusters
            .iter()
            .find(|c| c.id == format!("C{}", self.order_of(self.assignment[pos]) + 1))
    }

    fn order_of(&self, raw: usize) -> usize {
        // clusters are numbered by size; `assignment` keeps the raw Louvain ids
        let mut sizes: Vec<(usize, usize)> = (0..=self.assignment.iter().copied().max().unwrap_or(0))
            .map(|c| (self.assignment.iter().filter(|&&a| a == c).count(), c))
            .collect();
        sizes.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        sizes.iter().position(|&(_, c)| c == raw).unwrap_or(usize::MAX)
    }
}

fn enrich<F, I>(members: &BTreeSet<usize>, n: usize, keys_of: F, ic: I, min_ic: f64) -> Vec<(u32, usize, usize)>
where
    F: Fn(usize) -> Vec<u32>,
    I: Fn(u32) -> f64,
{
    let mut inside: HashMap<u32, usize> = HashMap::new();
    let mut outside: HashMap<u32, usize> = HashMap::new();
    for i in 0..n {
        let target = if members.contains(&i) {
            &mut inside
        } else {
            &mut outside
        };
        for key in keys_of(i) {
            *target.entry(key).or_default() += 1;
        }
    }
    let size = members.len().max(1) as f64;
    let rest = (n - members.len()).max(1) as f64;
    // coverage difference weighted by specificity: a specific process shared by most members
    // beats a broad one ("secretion") that is merely a bit more common inside
    let score = |k: u32, c_in: usize, c_out: usize| (c_in as f64 / size - c_out as f64 / rest) * ic(k);
    let mut v: Vec<(u32, usize, usize, f64)> = inside
        .into_iter()
        .filter(|&(k, c)| c >= 2 && ic(k) >= min_ic)
        .map(|(k, c)| {
            let o = outside.get(&k).copied().unwrap_or(0);
            (k, c, o, score(k, c, o))
        })
        .collect();
    v.sort_by(|a, b| b.3.total_cmp(&a.3).then(a.0.cmp(&b.0)));
    v.into_iter().map(|(k, c, o, _)| (k, c, o)).collect()
}

/// Cluster the slice and explain every cluster.
pub fn clusters(index: &SimilarityIndex, slice: &Slice, params: &ClusterParams) -> ClusterReport {
    let started = SimilarityIndex::now();
    let alpha = index.params().alpha;
    let pm = PairMatrix::new(index, &slice.members);
    let n = pm.len();
    let w = pm.combined(alpha);
    let nodes: Vec<usize> = (0..n).collect();
    let g = knn_graph(&w, n, &nodes, params.k);
    let comm = louvain(&g, params.resolution, params.seed);
    let q = modularity(&g, &comm, params.resolution);
    let reference = groups(&comm, &nodes);

    // stability: seeds
    let mut seed_j = vec![0.0; reference.len()];
    let mut seed_ari = 0.0;
    for r in 0..params.stability_runs {
        let c = louvain(&g, params.resolution, params.seed.wrapping_add(1000 + r as u64));
        let gs = groups(&c, &nodes);
        for (ci, rc) in reference.iter().enumerate() {
            seed_j[ci] += gs.iter().map(|x| jaccard(rc, x)).fold(0.0, f64::max);
        }
        seed_ari += crate::validation::ari(&comm, &c);
    }
    // stability: bootstrap subsamples
    let mut boot_j = vec![0.0; reference.len()];
    let mut boot_n = vec![0usize; reference.len()];
    let mut boot_ari = 0.0;
    let mut rng = SplitMix(params.seed ^ 0xB007);
    let take = ((n as f64) * params.bootstrap_fraction).round() as usize;
    for r in 0..params.stability_runs {
        let mut sample: Vec<usize> = (0..n).collect();
        rng.shuffle(&mut sample);
        sample.truncate(take);
        sample.sort_unstable();
        let sg = knn_graph(&w, n, &sample, params.k);
        let c = louvain(&sg, params.resolution, params.seed.wrapping_add(r as u64));
        let gs = groups(&c, &sample);
        let in_sample: BTreeSet<usize> = sample.iter().copied().collect();
        for (ci, rc) in reference.iter().enumerate() {
            let restricted: BTreeSet<usize> = rc.intersection(&in_sample).copied().collect();
            if restricted.is_empty() {
                continue;
            }
            boot_j[ci] += gs.iter().map(|x| jaccard(&restricted, x)).fold(0.0, f64::max);
            boot_n[ci] += 1;
        }
        let ref_restricted: Vec<usize> = sample.iter().map(|&i| comm[i]).collect();
        boot_ari += crate::validation::ari(&ref_restricted, &c);
    }
    let runs = params.stability_runs.max(1) as f64;

    // order clusters by size
    let mut order: Vec<usize> = (0..reference.len()).collect();
    order.sort_by(|&a, &b| reference[b].len().cmp(&reference[a].len()).then(a.cmp(&b)));
    let name_of = |raw: usize| format!("C{}", order.iter().position(|&c| c == raw).unwrap() + 1);

    let atlas = index.atlas();
    let profile = |i: usize| index.profile(pm.members[i]).expect("slice member");
    let edge = |a: usize, b: usize, wt: f64| {
        let r = index.explain(pm.members[a], pm.members[b]);
        ClusterEdge {
            a: atlas.disease_ref(pm.members[a]),
            b: atlas.disease_ref(pm.members[b]),
            weight: wt,
            phenotype: r.phenotype.score,
            mechanism: r.mechanism.score,
            reason: r.why.into_iter().next().unwrap_or_default(),
        }
    };
    let mut out = Vec::new();
    for &raw in &order {
        let set = &reference[raw];
        // genes
        let mut gc: BTreeMap<String, usize> = BTreeMap::new();
        for &i in set {
            for g in &profile(i).genes {
                *gc.entry(g.symbol.clone()).or_default() += 1;
            }
        }
        let mut genes: Vec<(String, usize)> = gc.into_iter().collect();
        genes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        genes.truncate(6);
        // processes: enriched, specific (IC >= 2), skipping ancestors of chosen ones
        let ranked = enrich(
            set,
            n,
            |i| profile(i).processes.iter().map(|p| p.0).collect(),
            |k| index.process_ic(k),
            2.0,
        );
        let mut chosen: Vec<u32> = Vec::new();
        let mut processes = Vec::new();
        for (key, inside, outside) in ranked {
            if processes.len() >= 3 {
                break;
            }
            let p = index.process_ref(key);
            if p.ic < 2.0 || chosen.iter().any(|&c| index.is_process_ancestor(key, c)) {
                continue;
            }
            chosen.push(key);
            processes.push(Enriched {
                id: p.id,
                name: p.name,
                source: p.source,
                in_cluster: inside,
                elsewhere: outside,
                ic: p.ic,
            });
        }
        // phenotypes
        let ranked = enrich(
            set,
            n,
            |i| profile(i).phenotype.iter().map(|p| p.0).collect(),
            |t| atlas.hpo.ic(t),
            2.0,
        );
        let mut chosen_t: Vec<u32> = Vec::new();
        let mut phenotypes = Vec::new();
        for (t, inside, outside) in ranked {
            if phenotypes.len() >= 3 {
                break;
            }
            let ic = atlas.hpo.ic(t);
            if ic < 2.0
                || chosen_t
                    .iter()
                    .any(|&c| atlas.hpo.ancestors(c).binary_search(&t).is_ok())
            {
                continue;
            }
            chosen_t.push(t);
            let term = atlas.hpo.term(t);
            phenotypes.push(Enriched {
                id: term.id.clone(),
                name: term.name.clone(),
                source: "hpo",
                in_cluster: inside,
                elsewhere: outside,
                ic,
            });
        }
        // edges inside and bridges
        let mut inner: Vec<(f64, usize, usize)> = Vec::new();
        let mut cross: Vec<(f64, usize, usize)> = Vec::new();
        for (a, b, wt) in g.edges() {
            match (set.contains(&a), set.contains(&b)) {
                (true, true) => inner.push((wt, a, b)),
                (true, false) => cross.push((wt, a, b)),
                (false, true) => cross.push((wt, b, a)),
                _ => {}
            }
        }
        inner.sort_by(|x, y| y.0.total_cmp(&x.0));
        cross.sort_by(|x, y| y.0.total_cmp(&x.0));
        let edges: Vec<ClusterEdge> = inner
            .iter()
            .take(params.shown)
            .map(|&(wt, a, b)| edge(a, b, wt))
            .collect();
        let bridges: Vec<Bridge> = cross
            .iter()
            .take(params.shown.min(4))
            .map(|&(wt, a, b)| Bridge {
                to_cluster: name_of(comm[b]),
                via: edge(a, b, wt),
            })
            .collect();
        // counterexamples: same gene, different mechanism, across clusters
        let mut counterexamples = Vec::new();
        for &a in set {
            for (b, &cb) in comm.iter().enumerate() {
                if cb != raw && pm.conflict[a * n + b] {
                    let r = index.mechanism(pm.members[a], pm.members[b]);
                    counterexamples.push(ClusterCounterexample {
                        member: atlas.disease_ref(pm.members[a]),
                        other: atlas.disease_ref(pm.members[b]),
                        other_cluster: name_of(cb),
                        why: r
                            .effect_conflicts
                            .iter()
                            .map(|c| c.why.clone())
                            .collect::<Vec<_>>()
                            .join("; "),
                    });
                }
            }
        }
        let bootstrap = if boot_n[raw] > 0 {
            boot_j[raw] / boot_n[raw] as f64
        } else {
            0.0
        };
        let stability = Stability {
            seed: seed_j[raw] / runs,
            bootstrap,
            verdict: match bootstrap {
                b if b > 0.75 => "stable",
                b if b >= 0.6 => "pattern",
                b if b >= 0.5 => "weak",
                _ => "dissolved",
            },
        };
        let gene_names: Vec<&str> = genes.iter().take(3).map(|g| g.0.as_str()).collect();
        let label = match processes.first() {
            Some(p) => format!("{} ({})", p.name, gene_names.join(", ")),
            None => gene_names.join(", "),
        };
        let mut why = Vec::new();
        for p in &processes {
            why.push(format!(
                "{} of {} members vs {} of {} other slice diseases involve '{}' ({})",
                p.in_cluster,
                set.len(),
                p.elsewhere,
                n - set.len(),
                p.name,
                p.source
            ));
        }
        if let Some(t) = phenotypes.first() {
            why.push(format!(
                "Typical symptom: '{}' in {} of {} members vs {} of {} elsewhere",
                t.name,
                t.in_cluster,
                set.len(),
                t.elsewhere,
                n - set.len()
            ));
        }
        why.push(format!(
            "Holds together in {:.0}% of subsample re-runs (Jaccard {:.2}: {})",
            stability.bootstrap * 100.0,
            stability.bootstrap,
            stability.verdict
        ));
        let mut members: Vec<NodeRef> = set.iter().map(|&i| atlas.disease_ref(pm.members[i])).collect();
        members.sort_by(|a, b| a.id.cmp(&b.id));
        out.push(Cluster {
            id: name_of(raw),
            label,
            why,
            members,
            genes,
            processes,
            phenotypes,
            edges,
            bridges,
            counterexamples,
            stability,
        });
    }
    let mut p = BTreeMap::new();
    p.insert("k".into(), params.k.to_string());
    p.insert("resolution".into(), params.resolution.to_string());
    p.insert("seed".into(), params.seed.to_string());
    p.insert("stability_runs".into(), params.stability_runs.to_string());
    p.insert("bootstrap_fraction".into(), params.bootstrap_fraction.to_string());
    p.insert(
        "algorithm".into(),
        "kNN graph (union) + Louvain (own implementation)".into(),
    );
    p.insert("slice_rule".into(), slice.rule.join(" "));
    p.insert("slice_activity".into(), slice.provenance.activity.id.clone());
    let counts = BTreeMap::from([
        ("members".to_owned(), n as u64),
        ("edges".to_owned(), g.edges().count() as u64),
        ("clusters".to_owned(), out.len() as u64),
    ]);
    let provenance = index.run_provenance(
        ACTIVITY_CLUSTERS,
        "Cluster the slice: kNN similarity graph + Louvain, labels, stability",
        started,
        p,
        counts,
    );
    ClusterReport {
        slice_rule: slice.rule.clone(),
        slice_size: n,
        params: params.clone(),
        alpha,
        modularity: q,
        clusters: out,
        seed_ari: seed_ari / runs,
        bootstrap_ari: boot_ari / runs,
        provenance,
        assignment: comm,
        members: pm.members.clone(),
    }
}

impl SimilarityIndex {
    /// Is process `a` an ancestor of (or equal to) process `b` (encoded keys)?
    pub fn is_process_ancestor(&self, a: u32, b: u32) -> bool {
        let (ka, ia) = self.decode_key(a);
        let (kb, ib) = self.decode_key(b);
        ka == kb
            && self
                .mechanism_data()
                .processes(kb)
                .ancestors(ib)
                .binary_search(&ia)
                .is_ok()
    }

    pub(crate) fn decode_key(&self, key: u32) -> (ProcessKind, u32) {
        let r = self.mechanism_data().reactome.len() as u32;
        if key < r {
            (ProcessKind::Reactome, key)
        } else {
            (ProcessKind::GoBp, key - r)
        }
    }
}
