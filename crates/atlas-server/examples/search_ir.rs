//! Evaluation driver. Reads shared inputs; writes derived caches only under cache/search-ir/.
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use atlas_core::search::{SearchOptions, domain::Index};
use atlas_core::snapshot;
use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "../src/resolve.rs"]
mod resolve;
#[path = "search_ir/spelling.rs"]
mod spelling;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(std::env::var("RARE_ATLAS_DATA")?);
    let cache = std::env::var_os("SEARCH_IR_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("cache/search-ir"));
    let atlas_signature = atlas_ingest::sources::signature(&data.join("raw"))?;
    let atlas_path = [data.join("cache/atlas.snapshot"), cache.join("atlas.snapshot")]
        .into_iter()
        .find(|p| snapshot::signature(p).is_ok_and(|s| s == atlas_signature));
    let mut atlas = match atlas_path.and_then(|p| snapshot::load(&p).ok()) {
        Some((atlas, _)) => atlas,
        None => {
            let atlas = atlas_ingest::build(&data.join("raw"))?;
            std::fs::create_dir_all(&cache)?;
            snapshot::save(&cache.join("atlas.snapshot"), &atlas, &atlas_signature)?;
            atlas
        }
    };
    let old = std::env::args().any(|a| a == "--hpoa-2024");
    if old {
        let path = data.join("raw/phenopackets/phenotype_2024-01-16.hpoa");
        let (terms, identity, provenance, diseases) = atlas.parts();
        let mut diseases = diseases.to_vec();
        let mut provenance = provenance.clone();
        let source = provenance.add_entity(atlas_core::provenance::SourceEntity {
            id: "source:search-ir-hpoa-2024".into(),
            file: "phenopackets/phenotype_2024-01-16.hpoa".into(),
            url:
                "https://github.com/obophenotype/human-phenotype-ontology/releases/download/v2024-01-16/phenotype.hpoa"
                    .into(),
            version: atlas_ingest::hpoa::hpoa_version(&path)?,
            sha256: Some(atlas_ingest::sources::sha256(&path)?),
            ..atlas_core::provenance::SourceEntity::default()
        });
        for disease in &mut diseases {
            disease.phenotypes.clear();
            disease.excluded.clear();
        }
        for row in atlas_ingest::hpoa::read_hpoa(&path, source)? {
            if row.aspect == "P"
                && let Some(term) = atlas.hpo.canonical(&row.hpo_id)
                && let Some(disease) = atlas.disease_idx(&row.disease_id)
            {
                diseases[disease as usize].annotate(term, row);
            }
        }
        atlas = atlas_core::Atlas::new(terms.to_vec(), identity.clone(), provenance, diseases);
    }
    let graph_path = cache.join("graph.snapshot");
    let graph_signature = atlas_ingest::graph::signature(&data)?;
    let mut graph = if old {
        atlas_core::Graph::default()
    } else {
        let path = [data.join("cache/graph.snapshot"), graph_path.clone()]
            .into_iter()
            .find(|p| snapshot::graph_signature(p).is_ok_and(|s| s == graph_signature));
        match path.and_then(|p| snapshot::load_graph(&p).ok()) {
            Some((graph, _)) => graph,
            None => {
                eprintln!("building private graph from shared source caches");
                let graph_data = atlas_ingest::graph::build(&data, &atlas)?;
                std::fs::create_dir_all(graph_path.parent().unwrap())?;
                snapshot::save_graph(&graph_path, &graph_data, &graph_signature)?;
                atlas_core::Graph::new(graph_data)
            }
        }
    };
    // This driver never bypasses the same fail-closed core filter used in serving.
    let suppression = std::fs::read(data.join("suppression.json")).ok();
    let quarantine = std::fs::read(data.join("cache/quarantine.json")).ok();
    graph.set_withhold(atlas_core::withhold::Withhold::from_lists(
        atlas_core::withhold::Salt::new(
            &std::env::var("ATLAS_SUPPRESSION_SALT").unwrap_or_else(|_| atlas_core::withhold::DEV_SALT.into()),
        ),
        suppression.as_deref(),
        quarantine.as_deref(),
    )?);
    let atlas = Arc::new(atlas);
    let matcher = atlas_analytics::Matcher::new(atlas.clone(), atlas_analytics::ScoringParams::default());
    let started = Instant::now();
    let index = Index::build(&atlas, &graph);
    eprintln!("index built in {} ms", started.elapsed().as_millis());
    if std::env::args().any(|a| a == "--typos") {
        println!("{}", spelling::evaluate(&atlas, &graph, &index));
        return Ok(());
    }
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        let start = Instant::now();
        let mut output = if let Some(query) = input["query"].as_str() {
            let limit = input["limit"].as_u64().unwrap_or(100) as usize;
            let options = SearchOptions {
                limit,
                include_retired: false,
            };
            let context = index.lexical_context(&atlas, &graph, query, options);
            let hits = if context.present.is_empty() {
                context.hits
            } else {
                let ranking = atlas_analytics::ranking::rank_all(
                    &matcher,
                    atlas_analytics::Scorer::Fusion,
                    &context.present,
                    &context.excluded,
                )?;
                if let Some(ranking) = ranking {
                    let candidates = ranking
                        .order
                        .iter()
                        .map(|&(slot, rank)| atlas_core::search::domain::PhenotypeCandidate {
                            disease: ranking.scored.disease(slot),
                            midrank: rank.mid,
                        })
                        .collect();
                    index.with_phenotypes(&atlas, &graph, context, candidates, options)
                } else {
                    context.hits
                }
            };
            let legacy = atlas.search().search(
                query,
                SearchOptions {
                    limit,
                    include_retired: false,
                },
            );
            let resolution = resolve::from_hits(&atlas, query, hits.clone());
            let body = resolve::body(&atlas, query, "en", &resolution);
            let compact: Vec<_> = hits
                .iter()
                .map(|h| json!({"node": h.node, "why": h.why, "reason": h.reason}))
                .collect();
            json!({"hits": compact, "legacy_ids": legacy.iter().map(|h| atlas.node_ref(h.node).id).collect::<Vec<_>>(),
                "resolve": { "status": body["status"], "target": body["target"], "choices": body["choices"].as_array().map(|choices|
                    choices.iter().map(|c| json!({"node": c["node"], "why": c["why"]})).collect::<Vec<_>>()) }})
        } else {
            let terms = |key: &str| -> Vec<u32> {
                input[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| atlas.hpo.canonical(id.as_str()?))
                    .collect()
            };
            let present = terms("present");
            let excluded = terms("excluded");
            let target = atlas.disease_idx(input["target"].as_str().unwrap_or(""));
            let ablation = target.and_then(|target| index.phenotype_rank(&atlas, &present, &excluded, target));
            let ranking =
                atlas_analytics::ranking::rank_all(&matcher, atlas_analytics::Scorer::Fusion, &present, &excluded)?;
            let rank = target.and_then(|target| ranking.and_then(|r| r.of(target).map(|r| r.0)));
            json!({"rank": rank.map(|r| r.mid), "rank_best": rank.map(|r| r.best), "rank_worst": rank.map(|r| r.worst),
                "simgic_ablation_rank": ablation.map(|r| r.0)})
        };
        output["id"] = input["id"].clone();
        output["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
        println!("{}", output);
    }
    Ok(())
}
