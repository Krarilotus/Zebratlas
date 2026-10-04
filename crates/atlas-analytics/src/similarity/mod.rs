//! Disease similarity with two independent signals, each returned with its explanation
//! (port of `mechanism.py`, extended with GO biological process and G2P-first variant effects):
//!
//! - **phenotype**: simGIC over HPO closures (sum IC(shared) / sum IC(union)), plus the most
//!   specific shared terms (no shared term's ancestors), by IC;
//! - **mechanism**: 1.0 for a shared causal gene with compatible effects; a shared gene whose effects
//!   are disjoint (e.g. LoF vs GoF) is a *conflict*; otherwise simGIC over process closures
//!   (Reactome and/or GO-BP), with the most specific shared processes. Conflicting genes are left out
//!   of the process profiles of that pair, so a gene acting in the opposite direction cannot make two
//!   diseases look mechanistically identical (the prototype scored KCNQ2 DEE vs KCNQ2 benign
//!   neonatal epilepsy 1.0 through the gene's own pathways).
//!
//! Effects per (disease, gene): G2P records of confidence definitive/strong/moderate with a determined
//! mechanism, else the Orphanet association type (LoF/GoF), see [`EffectRule`].
//! Build one [`SimilarityIndex`] at start-up; it precomputes every active disease's profile.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::SystemTime;

use atlas_core::mechanism::{self as mech, Effect, MechanismData, ProcessKind};
use atlas_core::provenance::{Activity, Agent, EntityIdx, SourceEntity};
use atlas_core::{Atlas, DiseaseIdx, TermIdx};

mod model;
mod score;

pub use model::*;
pub use score::{SplitMix, sim_gic};
use score::{quantile, sim_gic_score};

pub struct SimilarityIndex {
    atlas: Arc<Atlas>,
    mech: Arc<MechanismData>,
    params: SimilarityParams,
    profiles: HashMap<DiseaseIdx, Profile>,
    /// Gene symbol -> encoded process closure.
    gene_processes: HashMap<String, Vec<u32>>,
    /// MONDO hierarchy among atlas diseases.
    children: HashMap<String, Vec<DiseaseIdx>>,
    thresholds: Thresholds,
    roots: Vec<TermIdx>,
    reactome_len: u32,
    built_at: Option<String>,
}

/// Sorted (key, IC) features of one disease.
pub type Features = Vec<(u32, f64)>;
/// Orphanet-side effects, G2P effects, sources (while building a profile).
type GeneDraft = (BTreeSet<Effect>, BTreeSet<Effect>, Vec<GeneSource>);

fn now() -> Option<String> {
    Some(atlas_core::provenance::rfc3339(SystemTime::now()))
}

impl SimilarityIndex {
    pub fn new(atlas: Arc<Atlas>, mech: Arc<MechanismData>, params: SimilarityParams) -> Self {
        let roots = PHENOTYPE_ROOTS.iter().filter_map(|r| atlas.hpo.get(r)).collect();
        let mut idx = Self {
            reactome_len: mech.reactome.len() as u32,
            atlas,
            mech,
            params,
            profiles: HashMap::new(),
            gene_processes: HashMap::new(),
            children: HashMap::new(),
            thresholds: Thresholds::default(),
            roots,
            built_at: now(),
        };
        let active: Vec<DiseaseIdx> = idx.atlas.active().map(|(i, _)| i).collect();
        for &i in &active {
            for p in &idx.atlas.disease_at(i).parents {
                idx.children.entry(p.clone()).or_default().push(i);
            }
        }
        let profiles: HashMap<DiseaseIdx, Profile> = active.iter().map(|&i| (i, idx.build_profile(i))).collect();
        idx.profiles = profiles;
        idx.thresholds = idx.background();
        idx
    }

    pub fn atlas(&self) -> &Arc<Atlas> {
        &self.atlas
    }

    pub fn mechanism_data(&self) -> &Arc<MechanismData> {
        &self.mech
    }

    pub fn params(&self) -> &SimilarityParams {
        &self.params
    }

    pub fn thresholds(&self) -> &Thresholds {
        &self.thresholds
    }

    pub fn profile(&self, d: DiseaseIdx) -> Option<&Profile> {
        self.profiles.get(&d)
    }

    /// Active disease by any id (canonical or merged source id).
    pub fn resolve(&self, id: &str) -> Option<DiseaseIdx> {
        self.atlas.disease_idx(id).filter(|i| self.profiles.contains_key(i))
    }

    pub fn children(&self, id: &str) -> &[DiseaseIdx] {
        self.children.get(id).map_or(&[], Vec::as_slice)
    }

    /// MONDO descendants (excluding the node itself).
    pub fn descendants(&self, d: DiseaseIdx) -> HashSet<DiseaseIdx> {
        let mut out = HashSet::new();
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            for &c in self.children(&self.atlas.disease_at(x).id) {
                if out.insert(c) {
                    stack.push(c);
                }
            }
        }
        out.remove(&d);
        out
    }

    /// MONDO ancestors present as atlas nodes (excluding the node itself).
    pub fn ancestors(&self, d: DiseaseIdx) -> HashSet<DiseaseIdx> {
        let mut out = HashSet::new();
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            for p in &self.atlas.disease_at(x).parents {
                if let Some(pi) = self.atlas.disease_idx(p)
                    && self.atlas.disease_at(pi).id == *p
                    && out.insert(pi)
                {
                    stack.push(pi);
                }
            }
        }
        out.remove(&d);
        out
    }

    /// Union of the subtrees of `roots` (as `subtree()` in `explore_clusters.py`).
    pub fn subtree(&self, root: &str) -> BTreeSet<DiseaseIdx> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<DiseaseIdx> = self.children(root).to_vec();
        stack.extend(self.resolve(root).filter(|&r| self.atlas.disease_at(r).id == root));
        while let Some(x) = stack.pop() {
            if out.insert(x) {
                stack.extend(self.children(&self.atlas.disease_at(x).id));
            }
        }
        out
    }

    // -- process keys --

    fn encode(&self, kind: ProcessKind, p: u32) -> u32 {
        match kind {
            ProcessKind::Reactome => p,
            ProcessKind::GoBp => self.reactome_len + p,
        }
    }

    fn decode(&self, key: u32) -> (ProcessKind, u32) {
        if key < self.reactome_len {
            (ProcessKind::Reactome, key)
        } else {
            (ProcessKind::GoBp, key - self.reactome_len)
        }
    }

    pub fn process_ref(&self, key: u32) -> SharedProcess {
        let (kind, p) = self.decode(key);
        let o = self.mech.processes(kind);
        SharedProcess {
            id: o.id(p).to_owned(),
            name: o.name(p).to_owned(),
            source: kind.as_str(),
            ic: o.ic(p),
        }
    }

    pub fn process_ic(&self, key: u32) -> f64 {
        let (kind, p) = self.decode(key);
        self.mech.processes(kind).ic(p)
    }

    fn process_ancestors(&self, key: u32) -> impl Iterator<Item = u32> + '_ {
        let (kind, p) = self.decode(key);
        self.mech
            .processes(kind)
            .ancestors(p)
            .iter()
            .map(move |&a| self.encode(kind, a))
    }

    /// Encoded process closure of one gene (approved HGNC symbol) under the active process set.
    pub fn gene_closure(&self, symbol: &str) -> Vec<u32> {
        if let Some(c) = self.gene_processes.get(symbol) {
            return c.clone();
        }
        self.compute_gene_closure(symbol)
    }

    fn compute_gene_closure(&self, symbol: &str) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .params
            .processes
            .kinds()
            .iter()
            .flat_map(|&k| self.mech.gene_processes(k, symbol).into_iter().map(move |p| (k, p)))
            .map(|(k, p)| self.encode(k, p))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn processes_of<'a>(&self, genes: impl Iterator<Item = &'a str>) -> Vec<(u32, f64)> {
        let mut keys: Vec<u32> = genes.flat_map(|g| self.gene_closure(g)).collect();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter().map(|k| (k, self.process_ic(k))).collect()
    }

    // -- profiles --

    fn build_profile(&mut self, i: DiseaseIdx) -> Profile {
        let atlas = Arc::clone(&self.atlas);
        let d = atlas.disease_at(i);
        let mut terms: Vec<TermIdx> = d
            .phenotypes
            .iter()
            .flat_map(|e| atlas.hpo.ancestors(e.term).iter().copied())
            .filter(|t| !self.roots.contains(t))
            .collect();
        terms.sort_unstable();
        terms.dedup();
        let phenotype = terms.into_iter().map(|t| (t, atlas.hpo.ic(t))).collect();

        // symbol -> (orphanet-ish effects, g2p effects, sources); BTreeMap: stable gene order
        let mut genes: BTreeMap<String, GeneDraft> = BTreeMap::new();
        for link in &d.genes {
            if !CAUSAL_PREFIXES.iter().any(|p| link.association.starts_with(p)) || !is_symbol(&link.symbol) {
                continue;
            }
            let effect = Effect::from_orphanet(&link.association);
            let e = genes.entry(link.symbol.clone()).or_default();
            e.0.extend(effect);
            e.2.push(GeneSource {
                source: link.source.clone(),
                effect,
                record: atlas.provenance.cite(&link.record),
                url: None,
                confidence: link
                    .assessed
                    .map(|a| if a { "assessed" } else { "not yet assessed" }.to_owned()),
                allelic_requirement: None,
                support: None,
                association: Some(link.association.clone()),
                publications: link.pmids.clone(),
            });
        }
        for rec in self.mech.g2p_for_disease(&d.id) {
            if !rec.established() || !is_symbol(&rec.symbol) {
                continue;
            }
            let e = genes.entry(rec.symbol.clone()).or_default();
            e.1.extend(rec.mechanism);
            e.2.push(GeneSource {
                source: "G2P".into(),
                effect: rec.mechanism,
                record: rec.g2p_id.clone(),
                url: Some(rec.url()),
                confidence: Some(rec.confidence.clone()),
                allelic_requirement: Some(rec.allelic_requirement.clone()),
                support: Some(rec.mechanism_support.clone()),
                association: Some(rec.mechanism_raw.clone()),
                publications: rec.publications.clone(),
            });
        }
        let rule = self.params.effects;
        let genes: Vec<GeneProfile> = genes
            .into_iter()
            .map(|(symbol, (orpha, g2p, sources))| {
                let effects = match rule {
                    EffectRule::G2pThenOrphanet if !g2p.is_empty() => g2p,
                    EffectRule::G2pThenOrphanet => orpha,
                    EffectRule::Union => orpha.union(&g2p).copied().collect(),
                    EffectRule::Ignore => BTreeSet::new(),
                };
                let dosage = self.mech.clingen(&symbol).map(|c| Dosage {
                    haploinsufficiency: c.haploinsufficiency.raw.clone(),
                    haploinsufficiency_description: c.haploinsufficiency.description.clone(),
                    triplosensitivity: c.triplosensitivity.raw.clone(),
                    url: c.url(),
                    record: self.mech.provenance.cite(&c.record),
                });
                GeneProfile {
                    hgnc: self.mech.hgnc.get(&symbol).map(|g| g.hgnc_id.clone()),
                    symbol,
                    effects,
                    sources,
                    dosage,
                }
            })
            .collect();
        for g in &genes {
            if !self.gene_processes.contains_key(&g.symbol) {
                let c = self.compute_gene_closure(&g.symbol);
                self.gene_processes.insert(g.symbol.clone(), c);
            }
        }
        let processes = self.processes_of(genes.iter().map(|g| g.symbol.as_str()));
        Profile {
            phenotype,
            genes,
            processes,
        }
    }

    fn background(&self) -> Thresholds {
        let mut eligible: Vec<DiseaseIdx> = self
            .profiles
            .iter()
            .filter(|(_, p)| !p.phenotype.is_empty() && !p.processes.is_empty())
            .map(|(&i, _)| i)
            .collect();
        eligible.sort_unstable();
        if eligible.len() < 2 {
            return Thresholds::default();
        }
        let mut rng = SplitMix(self.params.background_seed);
        let (mut ph, mut pr) = (Vec::new(), Vec::new());
        for _ in 0..self.params.background_pairs {
            let a = eligible[rng.below(eligible.len())];
            let b = eligible[rng.below(eligible.len())];
            if a == b {
                continue;
            }
            let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
            ph.push(sim_gic_score(&pa.phenotype, &pb.phenotype));
            pr.push(sim_gic_score(&pa.processes, &pb.processes));
        }
        let q = self.params.strong_quantile;
        Thresholds {
            pairs: ph.len(),
            phenotype_strong: quantile(ph.clone(), q),
            process_strong: quantile(pr.clone(), q),
            phenotype_median: quantile(ph, 0.5),
            process_median: quantile(pr, 0.5),
        }
    }

    // -- pairs --

    /// Shared genes split into compatible ones and conflicts (symbols).
    fn gene_overlap<'a>(&self, a: &'a Profile, b: &'a Profile) -> (Vec<&'a str>, Vec<&'a str>) {
        let (mut ok, mut conflict) = (Vec::new(), Vec::new());
        for ga in &a.genes {
            if let Some(gb) = b.gene(&ga.symbol) {
                let disjoint = !ga.effects.is_empty() && !gb.effects.is_empty() && ga.effects.is_disjoint(&gb.effects);
                if disjoint {
                    conflict.push(ga.symbol.as_str());
                } else {
                    ok.push(ga.symbol.as_str());
                }
            }
        }
        (ok, conflict)
    }

    fn pair_processes(&self, a: &Profile, b: &Profile, conflict: &[&str]) -> (Features, Features) {
        let keep = |p: &Profile| {
            self.processes_of(
                p.genes
                    .iter()
                    .map(|g| g.symbol.as_str())
                    .filter(|s| !conflict.contains(s)),
            )
        };
        (keep(a), keep(b))
    }

    /// Fast scores for a pair of active diseases.
    pub fn score(&self, a: DiseaseIdx, b: DiseaseIdx) -> PairScore {
        let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
        let phenotype = sim_gic_score(&pa.phenotype, &pb.phenotype);
        let (ok, conflict) = self.gene_overlap(pa, pb);
        let process = if conflict.is_empty() {
            sim_gic_score(&pa.processes, &pb.processes)
        } else {
            let (x, y) = self.pair_processes(pa, pb, &conflict);
            sim_gic_score(&x, &y)
        };
        PairScore {
            phenotype,
            mechanism: if ok.is_empty() { process } else { 1.0 },
            process,
            compatible_gene: !ok.is_empty(),
            conflict: !conflict.is_empty(),
        }
    }

    pub fn phenotype(&self, a: DiseaseIdx, b: DiseaseIdx) -> PhenotypeSimilarity {
        let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
        let (score, shared) = sim_gic(&pa.phenotype, &pb.phenotype);
        let hpo = &self.atlas.hpo;
        let implied: HashSet<TermIdx> = shared
            .iter()
            .flat_map(|&t| hpo.ancestors(t).iter().copied().filter(move |&x| x != t))
            .collect();
        let mut specific: Vec<SharedTerm> = shared
            .iter()
            .filter(|t| !implied.contains(t))
            .map(|&t| SharedTerm {
                term: self.atlas.term_ref(t),
                ic: hpo.ic(t),
            })
            .collect();
        specific.sort_by(|x, y| y.ic.total_cmp(&x.ic).then_with(|| x.term.id.cmp(&y.term.id)));
        specific.truncate(self.params.shown);
        PhenotypeSimilarity {
            score,
            shared_count: shared.len(),
            shared: specific,
        }
    }

    /// Most specific shared phenotype terms, untruncated (for parity checks and labels).
    pub fn specific_shared_terms(&self, a: DiseaseIdx, b: DiseaseIdx) -> Vec<TermIdx> {
        let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
        let (_, shared) = sim_gic(&pa.phenotype, &pb.phenotype);
        let implied: HashSet<TermIdx> = shared
            .iter()
            .flat_map(|&t| self.atlas.hpo.ancestors(t).iter().copied().filter(move |&x| x != t))
            .collect();
        shared.into_iter().filter(|t| !implied.contains(t)).collect()
    }

    fn specific_processes(&self, shared: &[u32]) -> Vec<u32> {
        let implied: HashSet<u32> = shared
            .iter()
            .flat_map(|&k| self.process_ancestors(k).filter(move |&x| x != k))
            .collect();
        let mut out: Vec<u32> = shared.iter().copied().filter(|k| !implied.contains(k)).collect();
        out.sort_by(|&x, &y| {
            self.process_ic(y)
                .total_cmp(&self.process_ic(x))
                .then_with(|| self.process_ref(x).id.cmp(&self.process_ref(y).id))
        });
        out
    }

    pub fn mechanism(&self, a: DiseaseIdx, b: DiseaseIdx) -> MechanismSimilarity {
        let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
        let (ok, conflict) = self.gene_overlap(pa, pb);
        let (score_p, shared) = if conflict.is_empty() {
            sim_gic(&pa.processes, &pb.processes)
        } else {
            let (x, y) = self.pair_processes(pa, pb, &conflict);
            sim_gic(&x, &y)
        };
        let shared_gene = |s: &str| {
            let (ga, gb) = (pa.gene(s).expect("shared"), pb.gene(s).expect("shared"));
            SharedGene {
                symbol: s.to_owned(),
                effects_a: ga.effects.clone(),
                effects_b: gb.effects.clone(),
                sources_a: ga.sources.clone(),
                sources_b: gb.sources.clone(),
            }
        };
        let effect_conflicts = conflict
            .iter()
            .map(|&s| {
                let gene = shared_gene(s);
                let opposite = gene
                    .effects_a
                    .iter()
                    .any(|x| gene.effects_b.iter().any(|y| x.opposite(*y)));
                let fmt = |e: &BTreeSet<Effect>| e.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("/");
                let why = format!(
                    "{s}: {} here vs {} there ({})",
                    fmt(&gene.effects_a),
                    fmt(&gene.effects_b),
                    if opposite {
                        "opposite effect on the protein"
                    } else {
                        "different molecular mechanism"
                    }
                );
                EffectConflict { gene, opposite, why }
            })
            .collect();
        let specific = self.specific_processes(&shared);
        MechanismSimilarity {
            score: if ok.is_empty() { score_p } else { 1.0 },
            shared_genes: ok.iter().map(|s| shared_gene(s)).collect(),
            effect_conflicts,
            process_score: score_p,
            shared_process_count: shared.len(),
            shared_processes: specific
                .into_iter()
                .take(self.params.shown)
                .map(|k| self.process_ref(k))
                .collect(),
            known: !pa.processes.is_empty() && !pb.processes.is_empty(),
        }
    }

    /// Most specific shared processes, untruncated, as ids.
    pub fn specific_shared_processes(&self, a: DiseaseIdx, b: DiseaseIdx) -> Vec<String> {
        let (pa, pb) = (&self.profiles[&a], &self.profiles[&b]);
        let (_, conflict) = self.gene_overlap(pa, pb);
        let (x, y) = self.pair_processes(pa, pb, &conflict);
        let (_, shared) = sim_gic(&x, &y);
        self.specific_processes(&shared)
            .into_iter()
            .map(|k| self.process_ref(k).id)
            .collect()
    }

    /// PROV-O activity for a computation over this index: parameters plus the source entities of
    /// the atlas (HPO, MONDO, HPOA, Orphanet, genes_to_disease) and the mechanism sources in use.
    pub fn run_provenance(
        &self,
        id: &str,
        label: &str,
        started_at: Option<String>,
        mut parameters: BTreeMap<String, String>,
        counts: BTreeMap<String, u64>,
    ) -> RunProvenance {
        let mut inputs: Vec<SourceEntity> = self.atlas.provenance.entities.clone();
        let wanted: Vec<&str> = [mech::HGNC, mech::G2P, mech::CLINGEN]
            .into_iter()
            .chain(
                self.params
                    .processes
                    .kinds()
                    .iter()
                    .flat_map(|k| match k {
                        ProcessKind::Reactome => {
                            [mech::REACTOME_PATHWAYS, mech::REACTOME_RELATION, mech::REACTOME_GENES].as_slice()
                        }
                        ProcessKind::GoBp => [mech::GO_OBO, mech::GO_GAF].as_slice(),
                    })
                    .copied(),
            )
            .collect();
        inputs.extend(
            self.mech
                .provenance
                .entities
                .iter()
                .filter(|e| wanted.contains(&e.file.as_str()))
                .cloned(),
        );
        for (k, v) in self.params.as_map() {
            parameters.entry(k).or_insert(v);
        }
        parameters.insert("index_built_at".into(), self.built_at.clone().unwrap_or_default());
        let commit = self
            .mech
            .provenance
            .activities
            .first()
            .and_then(|a| a.agent.commit.clone());
        RunProvenance {
            activity: Activity {
                id: id.to_owned(),
                label: label.to_owned(),
                started_at,
                ended_at: now(),
                used: (0..inputs.len()).map(|i| EntityIdx(i as u16)).collect(),
                parameters,
                agent: Agent {
                    name: "atlas-analytics".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    commit,
                },
                counts,
            },
            inputs,
        }
    }

    pub(crate) fn now() -> Option<String> {
        now()
    }
}

/// Placeholder symbols (`-`, empty) in some gene-link rows are not genes.
fn is_symbol(s: &str) -> bool {
    !s.is_empty() && s != "-"
}
