//! ClinVar variant counts and functional-data coverage per gene, from `clinvar.variants` v1
//! (`data/cache/clinvar/<GENE>.json`; SOURCES.md). Counts by the record's aggregate clinical
//! significance as written; functional data = records with submitted `FunctionalData`. Excluded
//! records are not counted. Counts describe ClinVar submissions, not patients or families.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

/// Summary for one gene, or `None` when the file is absent/unreadable.
pub fn gene_summary(data: &Path, gene: &str) -> Option<Value> {
    static C: OnceLock<Mutex<HashMap<String, Option<Value>>>> = OnceLock::new();
    let cache = C.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(gene).cloned()) {
        return v;
    }
    let path = data.join("cache/clinvar").join(format!("{gene}.json"));
    let summary = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(|v| v["schema"] == "clinvar.variants")
        .map(|v| {
            let records = v["records"].as_array().cloned().unwrap_or_default();
            let mut by: BTreeMap<String, u64> = BTreeMap::new();
            let (mut total, mut functional) = (0u64, 0u64);
            for r in records.iter().filter(|r| r["excluded"] != true) {
                total += 1;
                let sig = r["clinical_significance"].as_str().unwrap_or("not provided").to_owned();
                *by.entry(sig).or_default() += 1;
                if r["functional_consequences"].as_array().is_some_and(|a| !a.is_empty()) {
                    functional += 1;
                }
            }
            json!({
                "gene": gene, "variants": total, "by_significance": by, "with_functional_data": functional,
                "complete": v["header"]["complete"], "source": {
                    "file": format!("data/cache/clinvar/{gene}.json"), "schema": "clinvar.variants v1",
                    "retrieved_at": v["header"]["retrieved_at"], "sha256": v["header"]["sha256"],
                    "url": format!("https://www.ncbi.nlm.nih.gov/clinvar/?term={gene}%5Bgene%5D"),
                },
                "note": crate::copy_extra::msg("questions.clinvar_counts", json!({}))["fallback"],
                "note_msg": crate::copy_extra::msg("questions.clinvar_counts", json!({})),
            })
        });
    if let Ok(mut c) = cache.lock() {
        c.insert(gene.to_owned(), summary.clone());
    }
    summary
}

/// Unknown-evidence sentence: how few variants carry functional data (variant-level caveat).
pub fn coverage_sentence(s: &Value) -> Value {
    let (gene, n, f) = (
        s["gene"].as_str().unwrap_or(""),
        s["variants"].as_u64().unwrap_or(0),
        s["with_functional_data"].as_u64().unwrap_or(0),
    );
    crate::copy::sentence(
        "questions.unknown.functional_coverage",
        json!({ "gene": gene, "with_functional_data": f, "variants": n }),
        vec![format!("clinvar:{gene}")],
    )
}
