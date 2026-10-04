//! Research models for questions (A10 assets), from `models.assets` v1
//! (`data/cache/models/<GENE>.json`; SOURCES.md): human iPSC lines (Cellosaurus) and mouse / fish /
//! fly models (Alliance, IMPC). Excluded records stay out; orthology is an inferred link, and a
//! listed model is not proof that it reproduces the human disease or is available to order.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use atlas_core::{Graph, graph::RecordWithhold, node::NodeKind};
use serde_json::{Value, json};

/// Included annotations of one gene (empty when the file is absent).
fn gene_records(data: &Path, gene: &str) -> Vec<Value> {
    static C: OnceLock<Mutex<HashMap<String, Vec<Value>>>> = OnceLock::new();
    let cache = C.get_or_init(Default::default);
    let cache_key = format!("{}:{gene}", data.display());
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(&cache_key).cloned()) {
        return v;
    }
    let path = data.join("cache/models").join(format!("{gene}.json"));
    let records: Vec<Value> = atlas_ingest::graph::cache::read_envelope(&path, "models.assets", &[1])
        .ok()
        .filter(|env| env.header_verified)
        .map(|env| env.records)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r["excluded"] != true)
        .filter(|r| r["gene"].as_str() == Some(gene))
        .collect();
    if let Ok(mut c) = cache.lock() {
        c.insert(cache_key, records.clone());
    }
    records
}

/// Plain label (sources may carry HTML markup such as `<sup>`).
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in s.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Per gene: distinct model counts by type, and up to `per_gene` assets (iPSC lines first).
pub fn for_genes(data: &Path, graph: &Graph, genes: &[String], per_gene: usize) -> (Value, Vec<Value>) {
    let mut counts = serde_json::Map::new();
    let mut assets = Vec::new();
    for gene in genes.iter().collect::<BTreeSet<_>>() {
        let recs: Vec<Value> = gene_records(data, gene)
            .into_iter()
            .filter(|r| {
                let id = atlas_ingest::graph::model_id(r["model_id"].as_str().unwrap_or(""));
                graph
                    .node(&id)
                    .is_some_and(|k| k.kind == NodeKind::Asset && graph.node_withheld(k).is_none())
            })
            .collect();
        if recs.is_empty() {
            continue;
        }
        let mut by_type: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in &recs {
            let t = r["asset_type"].as_str().unwrap_or("unknown").to_owned();
            let id = r["model_id"]
                .as_str()
                .unwrap_or(r["id"].as_str().unwrap_or(""))
                .to_owned();
            by_type.entry(t).or_default().insert(id);
        }
        counts.insert(
            gene.clone(),
            json!(
                by_type
                    .iter()
                    .map(|(k, v)| (k.clone(), v.len()))
                    .collect::<BTreeMap<_, _>>()
            ),
        );
        let mut seen = BTreeSet::new();
        let mut ordered: Vec<&Value> = recs.iter().collect();
        ordered.sort_by_key(|r| {
            let rank = match (r["asset_type"].as_str(), r["organism"]["id"].as_str()) {
                (Some("cell_model"), _) => 0,
                (_, Some("NCBITaxon:10090")) => 1,
                _ => 2,
            };
            (rank, r["model_id"].as_str().unwrap_or("").to_owned())
        });
        for r in ordered {
            let Some(mid) = r["model_id"].as_str() else { continue };
            if !seen.insert(mid.to_owned()) {
                continue;
            }
            if seen.len() > per_gene {
                break;
            }
            assets.push(json!({
                "id": mid, "title": plain(r["label"].as_str().unwrap_or(mid)), "type": "model",
                "asset_type": r["asset_type"], "organism": r["organism"], "gene": gene,
                "zygosity": r["zygosity"], "gene_link": r["gene_link"]["kind"].clone(),
                "url": r["url"], "covers": [], "bridging": false,
                "verify": format!("/api/verify/{}", crate::nodes::encode_path(&atlas_ingest::graph::model_id(mid))),
                "sources": [{ "id": r["id"], "source": r["source"], "url": r["source_url"], "locator": r["record_locator"],
                              "retrieved_on": r["retrieved_at"], "sha256": r["sha256"], "tier": "moderate" }],
                "note": crate::copy_extra::msg("models.note", json!({}))["fallback"],
                "note_msg": crate::copy_extra::msg("models.note", json!({})),
            }));
        }
    }
    (Value::Object(counts), assets)
}
