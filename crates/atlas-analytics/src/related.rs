//! Related conditions (J2.1, J2.2, J3.2): the k nearest diseases by combined phenotype + mechanism
//! similarity, each with its explanation and a verdict, plus explicit counterexamples:
//! look-alikes with a different mechanism, and the same gene acting through a different mechanism.
//!
//! Verdicts use background thresholds (the `strong_quantile` of random disease pairs), never
//! values tuned on an evaluation:
//! - `same_gene_different_mechanism`: every shared gene has disjoint effects (and nothing else is strong);
//! - `same_mechanism`: mechanism strong (compatible shared gene or process score >= strong) and
//!   phenotype strong;
//! - `mechanism_only` / `phenotype_only`: only one of the two is strong;
//! - `weak`: neither is strong (still among the nearest; shown with its reasons).

use std::collections::BTreeMap;

use atlas_core::DiseaseIdx;
use atlas_core::mechanism::Effect;
use atlas_core::node::NodeRef;
use serde::Serialize;

use crate::similarity::{
    GeneProfile, GeneSource, MechanismSimilarity, PhenotypeSimilarity, RunProvenance, SimilarityIndex, Thresholds,
};

pub const ACTIVITY_RELATED: &str = "activity:similarity-related";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    SameMechanism,
    MechanismOnly,
    PhenotypeOnly,
    SameGeneDifferentMechanism,
    Weak,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Related {
    pub neighbour: NodeRef,
    /// alpha * phenotype + (1 - alpha) * mechanism.
    pub score: f64,
    pub phenotype: PhenotypeSimilarity,
    pub mechanism: MechanismSimilarity,
    pub verdict: Verdict,
    /// Plain-language reasons, most important first.
    pub why: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterexampleKind {
    /// Shares a causal gene, but the gene's effect differs (e.g. KCNQ2 LoF vs GoF).
    SameGeneDifferentMechanism,
    /// G2P curates the same gene for this very condition with different mechanisms
    /// (e.g. UNC13A: biallelic LoF vs monoallelic GoF under one MONDO id).
    MechanismSplitWithinCondition,
    /// Phenotype similarity is strong, mechanism similarity no better than a random pair.
    LookalikeDifferentMechanism,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Counterexample {
    pub kind: CounterexampleKind,
    pub neighbour: Option<Related>,
    pub gene: Option<String>,
    /// The curations behind a within-condition split.
    pub records: Vec<GeneSource>,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RelatedReport {
    pub query: NodeRef,
    /// The query's causal genes with effects, sources and ClinGen dosage.
    pub genes: Vec<GeneProfile>,
    /// The query has more causal genes than `max_neighbour_genes`: a broad class, so "related"
    /// mostly reflects single member genes; the UI should say so.
    pub umbrella: bool,
    pub items: Vec<Related>,
    pub counterexamples: Vec<Counterexample>,
    pub thresholds: Thresholds,
    pub provenance: RunProvenance,
}

#[derive(Debug, thiserror::Error)]
pub enum RelatedError {
    #[error("unknown or inactive condition: {0}")]
    UnknownCondition(String),
}

fn effects(e: &std::collections::BTreeSet<Effect>) -> String {
    if e.is_empty() {
        "effect not stated".into()
    } else {
        e.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("/")
    }
}

impl SimilarityIndex {
    pub fn verdict(&self, ph: &PhenotypeSimilarity, me: &MechanismSimilarity) -> Verdict {
        let t = self.thresholds();
        let mech_strong = !me.shared_genes.is_empty() || me.process_score >= t.process_strong;
        let ph_strong = ph.score >= t.phenotype_strong;
        match (mech_strong, ph_strong) {
            (true, true) => Verdict::SameMechanism,
            _ if !me.effect_conflicts.is_empty() && me.shared_genes.is_empty() && !mech_strong => {
                Verdict::SameGeneDifferentMechanism
            }
            (true, false) => Verdict::MechanismOnly,
            (false, true) => Verdict::PhenotypeOnly,
            (false, false) => Verdict::Weak,
        }
    }

    /// Full explanation of one pair.
    pub fn explain(&self, a: DiseaseIdx, b: DiseaseIdx) -> Related {
        let s = self.score(a, b);
        let phenotype = self.phenotype(a, b);
        let mechanism = self.mechanism(a, b);
        let verdict = self.verdict(&phenotype, &mechanism);
        let mut why = Vec::new();
        for g in &mechanism.shared_genes {
            why.push(format!(
                "Same causal gene {} ({} here, {} there)",
                g.symbol,
                effects(&g.effects_a),
                effects(&g.effects_b)
            ));
        }
        for c in &mechanism.effect_conflicts {
            why.push(format!("Same gene, different mechanism: {}", c.why));
        }
        if !mechanism.shared_processes.is_empty() && mechanism.process_score > 0.0 {
            let names: Vec<&str> = mechanism
                .shared_processes
                .iter()
                .take(3)
                .map(|p| p.name.as_str())
                .collect();
            why.push(format!(
                "Shared biological processes (score {:.2}): {}",
                mechanism.process_score,
                names.join("; ")
            ));
        }
        if !phenotype.shared.is_empty() {
            let names: Vec<&str> = phenotype.shared.iter().take(3).map(|t| t.term.label.as_str()).collect();
            why.push(format!(
                "Shared informative symptoms (score {:.2}): {}",
                phenotype.score,
                names.join("; ")
            ));
        }
        if !mechanism.known {
            why.push("Mechanism unknown for at least one of the two (no annotated causal gene)".into());
        }
        Related {
            neighbour: self.atlas().disease_ref(b),
            score: s.combined(self.params().alpha),
            phenotype,
            mechanism,
            verdict,
            why,
        }
    }

    /// The `k` nearest conditions to `condition_id` plus counterexamples.
    pub fn related(&self, condition_id: &str, k: usize) -> Result<RelatedReport, RelatedError> {
        let started = Self::now();
        let q = self
            .resolve(condition_id)
            .ok_or_else(|| RelatedError::UnknownCondition(condition_id.to_owned()))?;
        let alpha = self.params().alpha;
        let hierarchy = if self.params().exclude_hierarchy {
            let mut h = self.ancestors(q);
            h.extend(self.descendants(q));
            h
        } else {
            Default::default()
        };
        let max_genes = self.params().max_neighbour_genes;
        let mut umbrellas = 0u64;
        let candidates: Vec<DiseaseIdx> = self
            .atlas()
            .active()
            .map(|(i, _)| i)
            .filter(|&i| i != q && !hierarchy.contains(&i))
            .filter(|&i| {
                let p = self.profile(i).expect("active");
                let umbrella = p.genes.len() > max_genes;
                umbrellas += u64::from(umbrella);
                !umbrella && (!p.phenotype.is_empty() || !p.genes.is_empty())
            })
            .collect();
        let mut scored: Vec<(f64, DiseaseIdx, crate::similarity::PairScore)> = candidates
            .iter()
            .map(|&i| {
                let s = self.score(q, i);
                (s.combined(alpha), i, s)
            })
            .collect();
        let atlas = self.atlas();
        scored.sort_by(|x, y| {
            y.0.total_cmp(&x.0)
                .then_with(|| atlas.disease_at(x.1).id.cmp(&atlas.disease_at(y.1).id))
        });
        let items: Vec<Related> = scored.iter().take(k).map(|&(_, i, _)| self.explain(q, i)).collect();

        let t = self.thresholds().clone();
        let mut counterexamples = Vec::new();
        // same gene, different mechanism (all of them: they are few and always informative)
        for &(_, i, s) in &scored {
            if s.conflict {
                let r = self.explain(q, i);
                let why = r
                    .mechanism
                    .effect_conflicts
                    .iter()
                    .map(|c| c.why.clone())
                    .collect::<Vec<_>>()
                    .join("; ");
                counterexamples.push(Counterexample {
                    kind: CounterexampleKind::SameGeneDifferentMechanism,
                    gene: r.mechanism.effect_conflicts.first().map(|c| c.gene.symbol.clone()),
                    neighbour: Some(r),
                    records: Vec::new(),
                    why: format!("Same gene, but {why}: a different disease mechanism, so findings may not transfer"),
                });
            }
        }
        // the query's own gene curated with different mechanisms (G2P, by allelic requirement)
        let profile = self.profile(q).expect("active");
        for g in &profile.genes {
            let g2p: Vec<&GeneSource> = g
                .sources
                .iter()
                .filter(|s| s.source == "G2P" && s.effect.is_some())
                .collect();
            let mut distinct: Vec<Effect> = g2p.iter().filter_map(|s| s.effect).collect();
            distinct.sort();
            distinct.dedup();
            if distinct.len() > 1 {
                let parts: Vec<String> = g2p
                    .iter()
                    .map(|s| {
                        format!(
                            "{} {} ({})",
                            s.allelic_requirement.as_deref().unwrap_or("?"),
                            s.effect.map_or("?", Effect::as_str),
                            s.record
                        )
                    })
                    .collect();
                counterexamples.push(Counterexample {
                    kind: CounterexampleKind::MechanismSplitWithinCondition,
                    neighbour: None,
                    gene: Some(g.symbol.clone()),
                    records: g2p.into_iter().cloned().collect(),
                    why: format!(
                        "G2P curates {} for this condition with different mechanisms: {}; which one applies depends on the variant",
                        g.symbol,
                        parts.join(" vs ")
                    ),
                });
            }
        }
        // look-alikes: strong phenotype similarity, mechanism no closer than a random pair
        let mut lookalikes: Vec<_> = scored
            .iter()
            .filter(|(_, i, s)| {
                s.phenotype >= t.phenotype_strong
                    && !s.compatible_gene
                    && s.process <= t.process_median
                    && !self.profile(*i).expect("active").processes.is_empty()
                    && !profile.processes.is_empty()
            })
            .collect();
        lookalikes.sort_by(|x, y| y.2.phenotype.total_cmp(&x.2.phenotype));
        for &&(_, i, _) in lookalikes.iter().take(3) {
            let r = self.explain(q, i);
            let genes: Vec<&str> = self
                .profile(i)
                .expect("active")
                .genes
                .iter()
                .map(|g| g.symbol.as_str())
                .collect();
            let why = format!(
                "Symptoms look alike (phenotype {:.2}, top {:.0}% of random pairs) but the cause differs: {} acts in other processes (mechanism {:.2}, no closer than a random pair)",
                r.phenotype.score,
                (1.0 - self.params().strong_quantile) * 100.0,
                genes.join(", "),
                r.mechanism.process_score
            );
            counterexamples.push(Counterexample {
                kind: CounterexampleKind::LookalikeDifferentMechanism,
                neighbour: Some(r),
                gene: None,
                records: Vec::new(),
                why,
            });
        }

        let mut params = BTreeMap::new();
        params.insert("query".into(), atlas.disease_at(q).id.clone());
        params.insert("query_input".into(), condition_id.to_owned());
        params.insert("k".into(), k.to_string());
        params.insert("phenotype_strong".into(), t.phenotype_strong.to_string());
        params.insert("process_strong".into(), t.process_strong.to_string());
        params.insert("process_median".into(), t.process_median.to_string());
        let mut counts = BTreeMap::new();
        counts.insert("candidates".into(), candidates.len() as u64);
        counts.insert("excluded:mondo-hierarchy".into(), hierarchy.len() as u64);
        counts.insert(format!("excluded:umbrella-more-than-{max_genes}-genes"), umbrellas);
        counts.insert("items".into(), items.len() as u64);
        counts.insert("counterexamples".into(), counterexamples.len() as u64);
        let provenance = self.run_provenance(
            ACTIVITY_RELATED,
            "Rank related conditions by phenotype + mechanism similarity",
            started,
            params,
            counts,
        );
        Ok(RelatedReport {
            query: atlas.disease_ref(q),
            genes: profile.genes.clone(),
            umbrella: profile.genes.len() > max_genes,
            items,
            counterexamples,
            thresholds: t,
            provenance,
        })
    }
}

/// One G2P curation of a gene, with the atlas conditions it resolves to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneCuration {
    pub g2p_id: String,
    pub url: String,
    pub disease_name: String,
    pub conditions: Vec<NodeRef>,
    pub allelic_requirement: String,
    pub confidence: String,
    pub effect: Option<Effect>,
    pub mechanism: String,
    pub support: String,
    pub publications: Vec<String>,
}

/// Mechanisms curated for one gene (J2.2 at gene level: UNC13A, KCNQ2).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneMechanisms {
    pub gene: String,
    pub hgnc: Option<String>,
    pub curations: Vec<GeneCuration>,
    /// Orphanet / HPO conditions with the association type (and its effect, if stated).
    pub other_links: Vec<(NodeRef, String, Option<Effect>)>,
    pub dosage: Option<crate::similarity::Dosage>,
    /// Established curations disagree on the mechanism.
    pub split: bool,
    pub why: Option<String>,
}

impl SimilarityIndex {
    /// G2P curations and other gene-disease links of `symbol`, flagging a mechanism split.
    pub fn gene_mechanisms(&self, symbol: &str) -> GeneMechanisms {
        let atlas = self.atlas();
        let m = self.mechanism_data();
        let node = |id: &String| atlas.disease_idx(id).map(|i| atlas.disease_ref(i));
        let curations: Vec<GeneCuration> = m
            .g2p_for_gene(symbol)
            .map(|r| GeneCuration {
                g2p_id: r.g2p_id.clone(),
                url: r.url(),
                disease_name: r.disease_name.clone(),
                conditions: r.conditions().iter().filter_map(node).collect(),
                allelic_requirement: r.allelic_requirement.clone(),
                confidence: r.confidence.clone(),
                effect: r.mechanism,
                mechanism: r.mechanism_raw.clone(),
                support: r.mechanism_support.clone(),
                publications: r.publications.clone(),
            })
            .collect();
        let mut other_links = Vec::new();
        if let Some(g) = atlas.gene(symbol) {
            for &d in &atlas.gene_at(g).diseases {
                for link in atlas.disease_at(d).genes.iter().filter(|l| l.symbol == symbol) {
                    other_links.push((
                        atlas.disease_ref(d),
                        link.association.clone(),
                        Effect::from_orphanet(&link.association),
                    ));
                }
            }
        }
        let established: Vec<&GeneCuration> = curations
            .iter()
            .filter(|c| atlas_core::mechanism::ESTABLISHED.contains(&c.confidence.as_str()) && c.effect.is_some())
            .collect();
        let mut effects: Vec<Effect> = established.iter().filter_map(|c| c.effect).collect();
        effects.sort();
        effects.dedup();
        let split = effects.len() > 1;
        let why = split.then(|| {
            let parts: Vec<String> = established
                .iter()
                .map(|c| {
                    format!(
                        "{} {} in {} ({})",
                        c.allelic_requirement,
                        c.effect.map_or("?", Effect::as_str),
                        c.disease_name,
                        c.g2p_id
                    )
                })
                .collect();
            format!("Same gene, different mechanisms: {}", parts.join(" vs "))
        });
        GeneMechanisms {
            gene: symbol.to_owned(),
            hgnc: m.hgnc.get(symbol).map(|g| g.hgnc_id.clone()),
            curations,
            other_links,
            dosage: m.clingen(symbol).map(|c| crate::similarity::Dosage {
                haploinsufficiency: c.haploinsufficiency.raw.clone(),
                haploinsufficiency_description: c.haploinsufficiency.description.clone(),
                triplosensitivity: c.triplosensitivity.raw.clone(),
                url: c.url(),
                record: m.provenance.cite(&c.record),
            }),
            split,
            why,
        }
    }
}
