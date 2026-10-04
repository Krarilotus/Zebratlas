//! D26 language contract, enforced: every user-facing text the API returns is a catalog message
//! (`{key, params, fallback}`), never bare English. A plain-text field may stay next to its message
//! only as the message's fallback (`x` beside `x_msg`, `text` beside `msg`), for older clients;
//! `x_msg: null` declares `x` as data for that item.
//!
//! Allowed bare strings are data, not copy: ids, codes, names, titles, URLs and the verbatim text of
//! source records (definitions, descriptions, record statements in `facts`). See
//! docs/reviews/COPY-AUDIT.md ("Backend") for conversion and integration details.

use serde_json::Value;

/// Fields whose strings are source data or identifiers, never our own copy.
const DATA_KEYS: &[&str] = &[
    "id",
    "label",
    "name",
    "title",
    "symbol",
    "url",
    "definition",
    "statement",
    "quote",
    "locator",
    "record",
    "sponsor",
    "organisation",
    "affiliation",
    "affiliations",
    "description",
    "source",
    "interventions",
    "officials",
    "sites",
    "covers",
    "reason",
    "plain_label",
    "file",
    "query",
    "search_text", // Verbatim query data in an official RePORTER API request, not UI copy.
    "matched",
    "synonyms",
    "inheritance",
    "onset",
    "clinical_course",
    "qualification",                  // Orphanet prevalence source vocabulary (e.g. "Value and class").
    "haploinsufficiency_description", // ClinGen dosage classifications copied from source.
    "triplosensitivity_description",
    "association", // Recorded gene association vocabulary, e.g. "loss of function".
    "conditions",  // Arrays of source condition names or IDs.
    "institution",
    "licence",
    "cites",
    "edge_id",
    "for_edge_ids",
    "evidence_code",
    "rdfs:label",
    "prov:wasDerivedFrom",
];

/// Gap parts kept beside the gap's message (`missing[].msg`) for the current web, which joins them.
/// Deprecated: remove when the web renders `msg` (copy audit, deliverable 2).
const LEGACY_FALLBACKS: [&str; 3] = ["what", "why", "how_to_close"];

/// Rules and run metadata are machine diagnostics and provenance, rather than UI copy.
const DATA_SUBTREES: &[&str] = &["rule", "provenance", "parameters", "validation", "activities", "calls"];

/// Words that only English prose has (in this API's data, ids and names never contain them alone).
const ENGLISH: [&str; 24] = [
    "the", "is", "are", "was", "of", "for", "no", "not", "this", "that", "with", "and", "to", "by", "from", "its",
    "which", "has", "have", "we", "our", "your", "found", "recorded",
];

fn is_prose(s: &str) -> bool {
    let words: Vec<String> = s
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .collect();
    words.len() >= 3 && !s.starts_with("http") && words.iter().any(|w| ENGLISH.contains(&w.as_str()))
}

/// JSON paths of bare English prose in an API response.
pub fn offenders(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(v, "$", &mut out);
    out
}

fn walk(v: &Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("key") && m.contains_key("fallback") {
                return; // a catalog message
            }
            for (k, x) in m {
                let beside_msg = m.get("msg").is_some_and(|m| !m.is_null());
                // `x_msg: null` declares `x` as data for this item (a researcher's affiliation as subtitle).
                let has_msg = m.contains_key(&format!("{k}_msg"))
                    || (beside_msg && (k == "text" || LEGACY_FALLBACKS.contains(&k.as_str())));
                if has_msg || DATA_KEYS.contains(&k.as_str()) || DATA_SUBTREES.contains(&k.as_str()) {
                    continue;
                }
                walk(x, &format!("{path}.{k}"), out);
            }
        }
        Value::Array(xs) => {
            for (i, x) in xs.iter().enumerate() {
                walk(x, &format!("{path}[{i}]"), out);
            }
        }
        Value::String(s) if is_prose(s) => out.push(format!("{path}: {s}")),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn detector_finds_prose_and_spares_messages_and_data() {
        let v = json!({
            "note": "No exact match for X in the sources searched.",
            "ok": { "key": "a.b", "params": {}, "fallback": "This is fine." },
            "why": { "text": "This study is registered for X.", "msg": { "key": "k", "fallback": "x" } },
            "channel": "Official website", "channel_msg": { "key": "channel.website", "fallback": "Official website" },
            "label": "Developmental and epileptic encephalopathy, 4",
            "request": {"criteria": {"search_text": "Rare genetic disorder with ophthalmic involvement"}},
            "status": "RECRUITING", "id": "MONDO:0012812", "list": ["No group for this condition"]
        });
        assert_eq!(offenders(&v).len(), 2, "{:?}", offenders(&v));
    }

    #[test]
    fn fact_paraphrases_are_checked_and_nested_messages_keep_parameters() {
        assert_eq!(
            offenders(&json!({"facts": [{"text": "A is a recorded symptom of B"}]})).len(),
            1
        );
        let f = crate::copy_extra::msg("facts.condition_symptom", json!({"arg0": "Seizures", "arg1": "DEE4"}));
        assert!(offenders(&json!({"facts": [{"text": f["fallback"], "msg": f.clone()}]})).is_empty());
        assert_eq!(f["params"]["arg0"], "Seizures");
        assert_eq!(f["params"]["arg1"], "DEE4");
    }

    /// Real snapshots only (read, never written): the family journey's endpoints (connections,
    /// gaps, coverage, ICD codes) return no bare English, for STXBP1 and a spread of conditions.
    #[test]
    fn journey_endpoints_return_messages_not_english() {
        let data = atlas_ingest::data_dir();
        let (snap, gsnap) = (
            atlas_ingest::snapshot_path(&data),
            atlas_ingest::graph::snapshot_path(&data),
        );
        if !snap.exists() || !gsnap.exists() {
            eprintln!("skipped: snapshots missing under {}", data.display());
            return;
        }
        let atlas = atlas_core::snapshot::load(&snap).unwrap().0;
        let graph = atlas_core::snapshot::load_graph(&gsnap).unwrap().0;
        let search = atlas_core::search::domain::Index::build(&atlas, &graph);
        let codes = crate::codes::load(&atlas, &data);
        let mut ids: Vec<_> = atlas.active().map(|(d, _)| d).step_by(997).take(40).collect();
        ids.extend(atlas.disease_idx("MONDO:0012812"));
        let mut bad = Vec::new();
        for d in ids {
            let conn = crate::connections::collect(&atlas, &graph, d);
            let all = crate::connections::expand_kinds(None);
            let (code_list, code_gap) = crate::codes::for_condition(&atlas, codes, d);
            for v in [
                crate::views::disease(&atlas, d),
                crate::resolve::body(
                    &atlas,
                    &conn.label,
                    "en",
                    &crate::resolve::resolve(&atlas, &graph, &search, &conn.label),
                ),
                crate::cards::connections(&atlas, &graph, &conn, None, 5),
                crate::cards::gaps(&atlas, &graph, &conn),
                crate::cards::coverage(&graph, &conn, &all),
                json!({ "codes": code_list, "gap": code_gap }),
            ] {
                bad.extend(offenders(&v).into_iter().map(|o| format!("{}: {o}", conn.label)));
            }
        }
        let integrity = crate::checks::integrity(&atlas, &graph);
        bad.extend(offenders(&integrity).into_iter().map(|o| format!("integrity: {o}")));
        for id in ["MONDO:0012812", "STXBP1", "HP:0001250"] {
            if let Some(v) = crate::checks::verify(&data, &atlas, &graph, id) {
                bad.extend(offenders(&v).into_iter().map(|o| format!("verify/{id}: {o}")));
            }
        }
        bad.sort();
        bad.dedup_by(|a, b| a.split(": ").nth(1) == b.split(": ").nth(1));
        assert!(
            bad.is_empty(),
            "{} bare English strings, e.g.:\n{}",
            bad.len(),
            bad[..bad.len().min(25)].join("\n")
        );
    }
}
