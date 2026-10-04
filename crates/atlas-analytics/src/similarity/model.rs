//! Parameters, profiles and the explained results of disease similarity.

use std::collections::{BTreeMap, BTreeSet};

use atlas_core::TermIdx;
use atlas_core::mechanism::{Effect, ProcessKind};
use atlas_core::node::NodeRef;
use atlas_core::provenance::{Activity, SourceEntity};
use serde::Serialize;

/// HPO roots left out of phenotype closures (as in the prototype).
pub const PHENOTYPE_ROOTS: [&str; 2] = ["HP:0000001", "HP:0000118"];
/// Orphanet / HPO association types that count as causal.
pub const CAUSAL_PREFIXES: [&str; 2] = ["Disease-causing", "MENDELIAN"];

/// Which process annotations form the mechanism profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSet {
    Reactome,
    GoBp,
    Both,
}

impl ProcessSet {
    pub fn kinds(self) -> &'static [ProcessKind] {
        match self {
            Self::Reactome => &[ProcessKind::Reactome],
            Self::GoBp => &[ProcessKind::GoBp],
            Self::Both => &[ProcessKind::Reactome, ProcessKind::GoBp],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reactome => "reactome",
            Self::GoBp => "go_bp",
            Self::Both => "reactome+go_bp",
        }
    }
}

/// How variant effects per (disease, gene) are chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectRule {
    /// Established G2P mechanism if any, else Orphanet (default).
    G2pThenOrphanet,
    /// Union of both (the prototype).
    Union,
    /// No effects: every shared gene is compatible (hold-out runs).
    Ignore,
}

#[derive(Clone, Debug, Serialize)]
pub struct SimilarityParams {
    /// combined = alpha * phenotype + (1 - alpha) * mechanism.
    pub alpha: f64,
    pub processes: ProcessSet,
    pub effects: EffectRule,
    /// Quantile of random-pair scores above which a signal counts as "strong".
    pub strong_quantile: f64,
    /// Random disease pairs sampled for the background distribution.
    pub background_pairs: usize,
    pub background_seed: u64,
    /// Leave the query's MONDO ancestors and descendants out of `related` (same disease, other grain).
    pub exclude_hierarchy: bool,
    /// Neighbours with more causal genes are umbrella classes (e.g. "complex neurodevelopmental
    /// disorder", 150 genes) where one shared gene says little; left out of `related`.
    pub max_neighbour_genes: usize,
    /// Shared terms / processes listed per explanation.
    pub shown: usize,
}

impl Default for SimilarityParams {
    fn default() -> Self {
        Self {
            alpha: 0.5,
            processes: ProcessSet::Both,
            effects: EffectRule::G2pThenOrphanet,
            strong_quantile: 0.95,
            background_pairs: 20_000,
            background_seed: 7,
            exclude_hierarchy: true,
            max_neighbour_genes: 20,
            shown: 8,
        }
    }
}

impl SimilarityParams {
    /// The Python prototype: Reactome only, effects unioned.
    pub fn prototype() -> Self {
        Self {
            processes: ProcessSet::Reactome,
            effects: EffectRule::Union,
            ..Self::default()
        }
    }

    pub fn as_map(&self) -> BTreeMap<String, String> {
        atlas_core::provenance::params([
            ("alpha", self.alpha.to_string()),
            ("processes", self.processes.as_str().to_owned()),
            ("effects", format!("{:?}", self.effects)),
            ("strong_quantile", self.strong_quantile.to_string()),
            ("background_pairs", self.background_pairs.to_string()),
            ("background_seed", self.background_seed.to_string()),
            ("exclude_hierarchy", self.exclude_hierarchy.to_string()),
            ("max_neighbour_genes", self.max_neighbour_genes.to_string()),
            ("phenotype", "simGIC over HPO closures minus HP:0000001/HP:0000118".to_owned()),
            (
                "mechanism",
                "1.0 for a shared causal gene with compatible effects, else simGIC over process closures without conflicting genes".to_owned(),
            ),
        ])
    }
}

/// One source asserting a gene for a disease, with the effect it states.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneSource {
    /// `G2P`, `Orphanet` or the gene-link source (`OMIM/MedGen` via HPO).
    pub source: String,
    pub effect: Option<Effect>,
    /// `file#locator` (Orphanet/HPO) or the G2P id.
    pub record: String,
    pub url: Option<String>,
    pub confidence: Option<String>,
    pub allelic_requirement: Option<String>,
    /// G2P mechanism support: `evidence` / `inferred`.
    pub support: Option<String>,
    pub association: Option<String>,
    pub publications: Vec<String>,
}

/// ClinGen dosage sensitivity, shown as independent support for a loss-of-function mechanism.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Dosage {
    pub haploinsufficiency: String,
    pub haploinsufficiency_description: String,
    pub triplosensitivity: String,
    pub url: String,
    pub record: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneProfile {
    pub symbol: String,
    pub hgnc: Option<String>,
    /// Effects under the active [`EffectRule`].
    pub effects: BTreeSet<Effect>,
    pub sources: Vec<GeneSource>,
    pub dosage: Option<Dosage>,
}

/// Precomputed per-disease features.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// HPO closure (minus roots) with IC, sorted by term.
    pub phenotype: Vec<(TermIdx, f64)>,
    pub genes: Vec<GeneProfile>,
    /// Process closure (encoded keys, see [`SimilarityIndex::process_ref`]) with IC, sorted.
    pub processes: Vec<(u32, f64)>,
}

impl Profile {
    pub fn gene(&self, symbol: &str) -> Option<&GeneProfile> {
        self.genes.iter().find(|g| g.symbol == symbol)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SharedTerm {
    pub term: NodeRef,
    pub ic: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhenotypeSimilarity {
    pub score: f64,
    /// Number of shared closure terms.
    pub shared_count: usize,
    /// Most specific shared terms, by IC (first `shown`).
    pub shared: Vec<SharedTerm>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SharedProcess {
    pub id: String,
    pub name: String,
    /// `reactome` / `go_bp`.
    pub source: &'static str,
    pub ic: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SharedGene {
    pub symbol: String,
    pub effects_a: BTreeSet<Effect>,
    pub effects_b: BTreeSet<Effect>,
    pub sources_a: Vec<GeneSource>,
    pub sources_b: Vec<GeneSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EffectConflict {
    pub gene: SharedGene,
    /// LoF vs GoF (or DN vs GoF); otherwise only different (e.g. LoF vs DN).
    pub opposite: bool,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MechanismSimilarity {
    pub score: f64,
    /// Shared causal genes with compatible effects.
    pub shared_genes: Vec<SharedGene>,
    pub effect_conflicts: Vec<EffectConflict>,
    /// simGIC over process closures (conflicting genes left out).
    pub process_score: f64,
    pub shared_process_count: usize,
    /// Most specific shared processes, by IC (first `shown`).
    pub shared_processes: Vec<SharedProcess>,
    /// Both diseases have at least one gene with process annotations.
    pub known: bool,
}

/// Background thresholds from random pairs (not tuned on any evaluation).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Thresholds {
    pub phenotype_strong: f64,
    pub process_strong: f64,
    pub phenotype_median: f64,
    pub process_median: f64,
    pub pairs: usize,
}

/// Scores only (no explanations): the fast path for ranking and clustering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairScore {
    pub phenotype: f64,
    pub mechanism: f64,
    pub process: f64,
    pub compatible_gene: bool,
    pub conflict: bool,
}

impl PairScore {
    pub fn combined(&self, alpha: f64) -> f64 {
        alpha * self.phenotype + (1.0 - alpha) * self.mechanism
    }
}

/// Self-contained PROV-O record of one computation: the activity plus the entities it used
/// (`activity.used` indexes into `inputs`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunProvenance {
    pub activity: Activity,
    pub inputs: Vec<SourceEntity>,
}
