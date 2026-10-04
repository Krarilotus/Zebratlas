//! The demo slice: DEE subtrees plus diseases of genes sharing STXBP1's vesicle-release processes.

use std::collections::{BTreeMap, BTreeSet};

use atlas_core::DiseaseIdx;
use serde::Serialize;

use crate::similarity::{RunProvenance, SimilarityIndex};

pub const ACTIVITY_SLICE: &str = "activity:slice-dee-vesicle";
/// The demo slice: DEE subtrees plus diseases of genes sharing STXBP1's vesicle-release processes.
pub const DEE_ROOTS: [&str; 2] = ["MONDO:0100062", "MONDO:0800490"];
pub const SEED_GENE: &str = "STXBP1";
/// GO "synaptic vesicle cycle": STXBP1's annotations that are, or regulate, a process below it
/// define the vesicle-release processes.
pub const VESICLE_CYCLE: &str = "GO:0099504";

#[derive(Clone, Debug, Serialize)]
pub struct Slice {
    pub members: Vec<DiseaseIdx>,
    /// Plain statement of the rule, for the UI and the docs.
    pub rule: Vec<String>,
    pub roots: Vec<String>,
    /// STXBP1 vesicle-release processes (GO ids and names).
    pub seed_processes: Vec<(String, String)>,
    /// Genes annotated to any seed process.
    pub seed_genes: Vec<String>,
    pub from_dee: usize,
    pub from_vesicle_genes: usize,
    pub provenance: RunProvenance,
}

impl Slice {
    /// Arbitrary member set (e.g. for tests), with the given rule text.
    pub fn custom(index: &SimilarityIndex, members: Vec<DiseaseIdx>, rule: &str) -> Self {
        let provenance = index.run_provenance(
            ACTIVITY_SLICE,
            "Custom slice",
            None,
            BTreeMap::from([("rule".to_owned(), rule.to_owned())]),
            BTreeMap::from([("members".to_owned(), members.len() as u64)]),
        );
        Self {
            members,
            rule: vec![rule.to_owned()],
            roots: Vec::new(),
            seed_processes: Vec::new(),
            seed_genes: Vec::new(),
            from_dee: 0,
            from_vesicle_genes: 0,
            provenance,
        }
    }
}

/// Build the DEE + vesicle-release slice (rule in [`Slice::rule`]).
pub fn dee_slice(index: &SimilarityIndex) -> Slice {
    let started = SimilarityIndex::now();
    let m = index.mechanism_data();
    let go = &m.go_bp;
    let eligible = |d: DiseaseIdx| {
        index.profile(d).is_some_and(|p| {
            !p.phenotype.is_empty() && !p.genes.is_empty() && p.genes.len() <= index.params().max_neighbour_genes
        })
    };
    let mut members: BTreeSet<DiseaseIdx> = BTreeSet::new();
    for r in DEE_ROOTS {
        members.extend(index.subtree(r).into_iter().filter(|&d| {
            index
                .profile(d)
                .is_some_and(|p| !p.phenotype.is_empty() && !p.genes.is_empty())
        }));
    }
    let from_dee = members.len();

    let cycle = go.get(VESICLE_CYCLE);
    let under_cycle = |p: u32| cycle.is_some_and(|c| go.ancestors(p).binary_search(&c).is_ok());
    let regulated = |t: u32| -> Vec<u32> { go.regulates.get(t as usize).cloned().unwrap_or_default() };
    // the processes STXBP1 takes part in, or regulates, below "synaptic vesicle cycle"
    let mut seeds: BTreeSet<u32> = BTreeSet::new();
    for &t in go.by_gene.get(SEED_GENE).into_iter().flatten() {
        if under_cycle(t) {
            seeds.insert(t);
        }
        for &a in go.ancestors(t) {
            seeds.extend(regulated(a).into_iter().filter(|&r| under_cycle(r)));
        }
    }
    let seeds: Vec<u32> = seeds.into_iter().collect();
    let hits = |p: u32| seeds.iter().any(|s| go.ancestors(p).binary_search(s).is_ok());
    // genes annotated to a seed process (or a part / subtype of it) or to its regulation
    let mut seed_genes: BTreeSet<String> = BTreeSet::new();
    for (gene, terms) in &go.by_gene {
        if terms.iter().any(|&t| {
            go.ancestors(t)
                .iter()
                .any(|&a| hits(a) || regulated(a).into_iter().any(hits))
        }) {
            seed_genes.insert(gene.clone());
        }
    }
    let before = members.len();
    let atlas = index.atlas();
    for (d, _) in atlas.active() {
        if !members.contains(&d)
            && eligible(d)
            && index
                .profile(d)
                .is_some_and(|p| p.genes.iter().any(|g| seed_genes.contains(&g.symbol)))
        {
            members.insert(d);
        }
    }
    let from_vesicle_genes = members.len() - before;
    let seed_processes: Vec<(String, String)> = seeds
        .iter()
        .map(|&p| (go.id(p).to_owned(), go.name(p).to_owned()))
        .collect();
    let rule = vec![
        format!(
            "All active diseases in the MONDO subtrees of {} (developmental and epileptic encephalopathy) and {} that have phenotype annotations and at least one causal gene (Orphanet/HPO causal links or established G2P records), as in explore_clusters.py.",
            DEE_ROOTS[0], DEE_ROOTS[1]
        ),
        format!(
            "Plus every active disease with phenotype annotations, at most {} causal genes, and a causal gene annotated (GO-BP, non-IEA, is_a/part_of descendants) to one of {SEED_GENE}'s vesicle-release processes, or to their regulation. The vesicle-release processes are the GO-BP processes below {VESICLE_CYCLE} 'synaptic vesicle cycle' that {SEED_GENE}'s own annotations are, or regulate (GO regulates relations).",
            index.params().max_neighbour_genes
        ),
    ];
    let mut params = BTreeMap::new();
    params.insert("roots".into(), DEE_ROOTS.join(","));
    params.insert("seed_gene".into(), SEED_GENE.into());
    params.insert("vesicle_cycle".into(), VESICLE_CYCLE.into());
    params.insert(
        "seed_processes".into(),
        seed_processes
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>()
            .join(","),
    );
    let counts = BTreeMap::from([
        ("from_dee".to_owned(), from_dee as u64),
        ("from_vesicle_genes".to_owned(), from_vesicle_genes as u64),
        ("seed_genes".to_owned(), seed_genes.len() as u64),
        ("members".to_owned(), members.len() as u64),
    ]);
    let provenance = index.run_provenance(
        ACTIVITY_SLICE,
        "Select the DEE + vesicle-release disease slice",
        started,
        params,
        counts,
    );
    Slice {
        members: members.into_iter().collect(),
        rule,
        roots: DEE_ROOTS.iter().map(|s| (*s).to_owned()).collect(),
        seed_processes,
        seed_genes: seed_genes.into_iter().collect(),
        from_dee,
        from_vesicle_genes,
        provenance,
    }
}
