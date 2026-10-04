//! Mechanism assertions as evidence [`Statement`]s, for [`crate::evidence`] (contradiction
//! detection, explained confidence, experiment proposals).
//!
//! - **G2P**: one statement per gene-disease model and atlas condition it is filed under (its own
//!   `G2P:` node, else the resolved MONDO/OMIM node). `inferred` mechanism support stays `Inferred`;
//!   only `evidence` support is `Observed`. The G2P confidence is kept as source classification and
//!   never promoted to mechanism certainty.
//! - **Pathways**: the *direct* Reactome (lowest level) and GO-BP (non-IEA) annotations of a
//!   disease's causal genes, each citing the annotation file and the gene link it travels through.
//!
//! Orphanet LoF/GoF statements come from [`crate::evidence::statements_from_atlas`] already, so they
//! are not repeated here.

use atlas_core::mechanism::{self as mech, Effect, ProcessKind};
use atlas_core::node::EdgeKind;

use crate::evidence::{Citation, Context, Curation, Relation, Review, Statement, Tier, Value, analyze_atlas};
use crate::similarity::SimilarityIndex;

fn value(effect: Option<Effect>) -> Value {
    match effect {
        Some(Effect::LoF) => Value::LossOfFunction,
        Some(Effect::GoF) => Value::GainOfFunction,
        Some(Effect::DN) => Value::DominantNegative,
        None => Value::Unknown,
    }
}

/// Atlas gene id (HGNC, else NCBIGene, else symbol): the object the base adapter uses.
fn gene_id(index: &SimilarityIndex, symbol: &str) -> String {
    let atlas = index.atlas();
    atlas
        .gene(symbol)
        .map(|g| atlas.gene_at(g).id().to_owned())
        .or_else(|| index.mechanism_data().hgnc.get(symbol).map(|g| g.hgnc_id.clone()))
        .unwrap_or_else(|| symbol.to_owned())
}

/// Every G2P model (all confidence levels; disputed/refuted are marked so) as mechanism statements.
/// `diseases`: keep only statements about these canonical ids.
pub fn g2p_statements(index: &SimilarityIndex, diseases: Option<&[String]>) -> Vec<Statement> {
    let m = index.mechanism_data();
    let entity = m
        .entity_by_file(mech::G2P)
        .map(|e| m.provenance.entity(e).clone())
        .unwrap_or_default();
    let mut out = Vec::new();
    for r in &m.g2p {
        let conditions = r.conditions();
        for c in conditions {
            if diseases.is_some_and(|ds| !ds.contains(c)) {
                continue;
            }
            let id = if conditions.len() == 1 {
                r.g2p_id.clone()
            } else {
                format!("{}@{c}", r.g2p_id)
            };
            out.push(Statement {
                id,
                subject: c.clone(),
                relation: Relation::Mechanism,
                object: gene_id(index, &r.symbol),
                value: value(r.mechanism),
                raw_value: format!(
                    "{}; support={}; disease={}",
                    r.mechanism_raw, r.mechanism_support, r.disease_name
                ),
                kind: if r.mechanism_support == "evidence" {
                    EdgeKind::Observed
                } else {
                    EdgeKind::Inferred
                },
                tier: Tier::Curated,
                curation: match r.confidence.as_str() {
                    "disputed" => Curation::Disputed,
                    "refuted" => Curation::Refuted,
                    _ => Curation::Reviewed,
                },
                source_classification: Some(r.confidence.clone()),
                context: Context {
                    source_disease: r
                        .disease_ids
                        .first()
                        .cloned()
                        .unwrap_or_else(|| format!("G2P:{}", r.g2p_id)),
                    inheritance: Some(r.allelic_requirement.clone()),
                    ..Context::default()
                },
                citations: vec![Citation {
                    entity: entity.clone(),
                    locator: format!("{}; g2p id={}", r.record.locator, r.g2p_id),
                    references: r.publications.clone(),
                }],
                reviewed_year: r.reviewed.get(..4).and_then(|y| y.parse().ok()),
                quote: None,
            });
        }
    }
    out
}

/// Direct Reactome / GO-BP annotations of the causal genes of `diseases` (active canonical ids).
pub fn pathway_statements(index: &SimilarityIndex, diseases: &[String]) -> Vec<Statement> {
    let m = index.mechanism_data();
    let atlas = index.atlas();
    let entity = |file: &str| {
        m.entity_by_file(file)
            .map(|e| m.provenance.entity(e).clone())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for id in diseases {
        let Some(d) = index.resolve(id) else { continue };
        let disease = &atlas.disease_at(d).id;
        let Some(profile) = index.profile(d) else { continue };
        for g in &profile.genes {
            let Some(hg) = m.hgnc.get(&g.symbol) else { continue };
            let link = g.sources.first();
            for &kind in index.params().processes.kinds() {
                let o = m.processes(kind);
                let Some(key) = m.gene_key(kind, hg) else { continue };
                let (file, url) = match kind {
                    ProcessKind::Reactome => (mech::REACTOME_GENES, "reactome"),
                    ProcessKind::GoBp => (mech::GO_GAF, "go"),
                };
                for &p in o.by_gene.get(key).into_iter().flatten() {
                    let mut citations = vec![Citation {
                        entity: entity(file),
                        locator: format!("gene={key}; term={}", o.id(p)),
                        references: vec![match kind {
                            ProcessKind::Reactome => format!("https://reactome.org/content/detail/{}", o.id(p)),
                            ProcessKind::GoBp => format!("https://amigo.geneontology.org/amigo/term/{}", o.id(p)),
                        }],
                    }];
                    if let Some(l) = link {
                        citations.push(Citation {
                            entity: if l.source == "G2P" {
                                entity(mech::G2P)
                            } else {
                                atlas
                                    .provenance
                                    .entities
                                    .iter()
                                    .find(|e| l.record.starts_with(&e.file))
                                    .cloned()
                                    .unwrap_or_default()
                            },
                            locator: l.record.clone(),
                            references: l.publications.clone(),
                        });
                    }
                    out.push(Statement {
                        id: format!("{disease}|{}|{url}:{}", g.symbol, o.id(p)),
                        subject: disease.clone(),
                        relation: Relation::Pathway,
                        object: o.id(p).to_owned(),
                        value: Value::Present,
                        raw_value: format!("{} via {}", o.name(p), g.symbol),
                        kind: EdgeKind::Inferred,
                        tier: Tier::Curated,
                        curation: Curation::Unknown,
                        source_classification: None,
                        context: Context {
                            source_disease: disease.clone(),
                            ..Context::default()
                        },
                        citations,
                        reviewed_year: None,
                        quote: None,
                    });
                }
            }
        }
    }
    out
}

/// Evidence review of `diseases` (or every active disease) with the mechanism layer's G2P
/// statements and, for a disease selection, its pathway statements.
pub fn review(index: &SimilarityIndex, diseases: Option<&[String]>, as_of_year: u16) -> Result<Review, String> {
    let canonical: Option<Vec<String>> = diseases.map(|ids| {
        ids.iter()
            .filter_map(|id| index.atlas().disease_idx(id))
            .map(|i| index.atlas().disease_at(i).id.clone())
            .collect()
    });
    let mut additional = g2p_statements(index, canonical.as_deref());
    if let Some(ids) = &canonical {
        additional.extend(pathway_statements(index, ids));
    }
    analyze_atlas(index.atlas(), diseases, &additional, as_of_year)
}
