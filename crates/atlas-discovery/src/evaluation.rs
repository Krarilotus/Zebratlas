//! Small, transparent audit: conditional ID alignment precision, not biological truth.
use crate::{Bundle, Decision, sha256};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Serialize, Deserialize)]
pub struct Audit {
    pub reviewer: String,
    pub review_kind: String,
    pub reviewed_at: String,
    pub selection: String,
    pub source_hashes: std::collections::BTreeMap<String, String>,
    pub rows: Vec<GoldRow>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct GoldRow {
    pub source: String,
    pub locator: String,
    pub symbol: String,
    pub expected_hgnc: String,
    pub crosscheck: String,
}

pub fn evaluate(b: &Bundle, audit: &Audit, audit_bytes: &[u8]) -> Result<Value> {
    for (id, hash) in &audit.source_hashes {
        ensure!(
            b.sources.sources.iter().any(|s| &s.id == id && &s.sha256 == hash),
            "audit source snapshot mismatch: {id}"
        );
    }
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for source in ["panelapp", "uniprot"] {
        let mut correct = 0_u64;
        let mut automatic = 0_u64;
        let rows: Vec<_> = audit.rows.iter().filter(|r| r.source == source).collect();
        ensure!(!rows.is_empty(), "empty audit source");
        for gold in &rows {
            ensure!(seen.insert((&gold.source, &gold.locator)), "duplicate audit row");
            let matches: Vec<_> = b
                .records
                .iter()
                .filter(|r| r.source == source && r.locator.locator.to_string() == gold.locator)
                .collect();
            ensure!(matches.len() == 1, "audit locator not uniquely present");
            if let Some(a) = &matches[0].alignment
                && a.decision == Decision::Exact
            {
                automatic += 1;
                if a.targets == [gold.expected_hgnc.clone()] && a.mention.label == gold.symbol {
                    correct += 1;
                }
            }
        }
        // Wilson interval is descriptive only: this purposive tiny slice is not an IID sample.
        let (precision, interval) = if automatic > 0 {
            let n = automatic as f64;
            let p = correct as f64 / n;
            let z = 1.96_f64;
            let denominator = 1.0 + z * z / n;
            let center = (p + z * z / (2.0 * n)) / denominator;
            let half = z * ((p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / denominator;
            (json!(p), json!([center - half, (center + half).min(1.0)]))
        } else {
            (Value::Null, Value::Null)
        };
        results.push(
            json!({"source":source,"audited":rows.len(),"automatic_audited":automatic,
            "correct":correct,"precision":precision,"wilson_95_descriptive_only":interval,
            "all_automatic_in_source":b.metrics[&format!("{source}.automatic")]}),
        );
    }
    Ok(
        json!({"schema":"atlas.discovery.audit-result","version":1,"audit_sha256":sha256(audit_bytes),
        "bundle_manifest_sha256":b.manifest_sha256,"reviewer":audit.reviewer,"review_kind":audit.review_kind,
        "reviewed_at":audit.reviewed_at,"selection":audit.selection,"results":results,
        "limitations":"Agent-inspected identifier consistency, not independent human adjudication; tiny purposive slice; no discovery recall, disease mechanism or clinical validity claim."}),
    )
}
