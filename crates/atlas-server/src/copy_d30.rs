//! Catalog messages added for D30 (identity candidates, live ranking claim), kept apart from
//! `copy.rs` while that file is being edited; fold them into it later. Each message is a key,
//! its params and the English fallback rendered from the same params (D26). The web keys are in
//! `web/messages/d30/en.json`.

use serde_json::{Value, json};

/// `{key, params, fallback}`.
fn msg(key: &str, params: Value, fallback: String) -> Value {
    json!({ "key": key, "params": params, "fallback": fallback })
}

/// "Possibly the same as …": a label-nominated MONDO candidate, never shown as the same node.
pub fn candidate_same_as(target: &str, target_label: &str, matched: &[String]) -> Value {
    let fallback = format!(
        "Possibly the same as {target_label} ({target}): the names match, but no authoritative mapping confirms it."
    );
    msg(
        "identity.candidate_same_as",
        json!({ "target": target, "target_label": target_label, "matched": matched }),
        fallback,
    )
}

/// Incoming candidate on a MONDO page: another source's entry whose name matches this one.
pub fn candidate_source(source_id: &str) -> Value {
    let fallback = format!(
        "{source_id} has the same name and is possibly the same condition; no authoritative mapping confirms it."
    );
    msg("identity.candidate_source", json!({ "source_id": source_id }), fallback)
}

/// The audited ranker-v3 sentence (docs/brief/LEARN.md §5a point 4), with its numbers as params.
pub fn ranker_v3_claim(run: &str) -> Value {
    let p = json!({
        "cases": 387, "atlas_top1": "23.8%", "resnik_top1": "27.1%", "fusion_top1": "29.5%", "run": run,
    });
    let fallback = "On 387 published cases with no detected direct PMID overlap, a preregistered rank fusion that \
        mainly follows the Resnik ontology baseline and uses the atlas score to break its first-place ties raised \
        exact top-1 from 23.8% (atlas) and 27.1% (Resnik) to 29.5%, with positive publication-grouped 95% intervals \
        for the gain; top-10 did not improve. Retrospective evidence on a benchmark whose earlier results informed \
        the design; not clinical validation."
        .to_owned();
    msg("match.claim.ranker_v3", p, fallback)
}
