//! Thin adapter: the core owns ranking; analytics owns mechanism compatibility.
use crate::routes::AppState;
use atlas_core::search::SearchOptions;
use atlas_core::search::domain::{Hit, MechanismCandidate, PhenotypeCandidate, Reason, Source};

pub fn ranked(s: &AppState, query: &str, opts: SearchOptions) -> Vec<Hit> {
    if opts.limit == 0 {
        return vec![];
    }
    let atlas = s.atlas();
    let context = s
        .search
        .lexical_context(atlas, &s.graph, query, SearchOptions { limit: 100, ..opts });
    from_context(s, context, opts)
}

pub fn from_context(s: &AppState, context: atlas_core::search::domain::QueryMatches, opts: SearchOptions) -> Vec<Hit> {
    let atlas = s.atlas();
    let hits = with_phenotypes(s, context, SearchOptions { limit: 100, ..opts });
    let Some(Ok(similarity)) = s.related.get() else {
        return hits.into_iter().take(opts.limit).collect();
    };
    let seeds: Vec<_> = hits
        .iter()
        .filter(|h| h.key.kind == atlas_core::node::NodeKind::Disease && (h.strong || h.reason == Reason::CausalGene))
        .take(2)
        .map(|h| h.key.idx)
        .collect();
    let mut candidates = Vec::new();
    for seed in seeds {
        let Ok(report) = similarity.related(&atlas.disease_at(seed).id, 20) else {
            continue;
        };
        for related in report.items {
            let mechanism = related.mechanism;
            if !mechanism.effect_conflicts.is_empty() || mechanism.score <= 0.0 {
                continue;
            }
            let shared = if let Some(gene) = mechanism.shared_genes.first() {
                // Unknown effects do not warrant a compatibility assertion.
                if gene.effects_a.is_empty() || gene.effects_b.is_empty() {
                    continue;
                }
                let Some(effect) = gene.effects_a.intersection(&gene.effects_b).next() else {
                    continue;
                };
                match effect {
                    atlas_core::mechanism::Effect::LoF => format!("reduced {} activity", gene.symbol),
                    atlas_core::mechanism::Effect::GoF => format!("increased {} activity", gene.symbol),
                    atlas_core::mechanism::Effect::DN => format!("interference by the {} protein", gene.symbol),
                }
            } else if let Some(process) = mechanism.shared_processes.first() {
                process.name.clone()
            } else {
                continue;
            };
            let Some(disease) = atlas.disease_idx(&related.neighbour.id) else {
                continue;
            };
            let mech = similarity.mechanism_data();
            let sources = mech
                .provenance
                .entities
                .iter()
                .map(|entity| Source {
                    source_url: entity.url.clone(),
                    retrieved_at: entity.retrieved_at.clone(),
                    version: entity.version.clone(),
                    sha256: entity.sha256.clone(),
                    locator: entity.file.clone(),
                })
                .collect();
            candidates.push(MechanismCandidate {
                source: seed,
                disease,
                score: mechanism.score,
                shared,
                sources,
            });
        }
    }
    s.search.with_mechanisms(atlas, &s.graph, hits, candidates, opts)
}

/// Shared clinical scorer for lexical and model-linked HPO profiles.
pub fn with_phenotypes(
    s: &AppState,
    context: atlas_core::search::domain::QueryMatches,
    opts: SearchOptions,
) -> Vec<Hit> {
    let atlas = s.atlas();
    let hits = if !context.present.is_empty() {
        match atlas_analytics::ranking::rank_all(
            &s.matcher,
            atlas_analytics::Scorer::Fusion,
            &context.present,
            &context.excluded,
        ) {
            Ok(Some(ranking)) => {
                let candidates = ranking
                    .order
                    .iter()
                    .map(|&(slot, rank)| PhenotypeCandidate {
                        disease: ranking.scored.disease(slot),
                        midrank: rank.mid,
                    })
                    .collect();
                s.search.with_phenotypes(
                    atlas,
                    &s.graph,
                    context,
                    candidates,
                    SearchOptions { limit: 100, ..opts },
                )
            }
            _ => context.hits,
        }
    } else {
        context.hits
    };
    hits.into_iter().take(opts.limit).collect()
}
