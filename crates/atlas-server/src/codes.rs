//! ICD-10 / ICD-11 codes of a condition as **typed links, never merges**: Orphanet en_product1
//! `ExternalReference` rows (Source ICD-10 / ICD-11, with mapping relation and validation status)
//! and MONDO xrefs (`ICD10CM:`, `icd11.foundation:`, with their MONDO source qualifiers).
//! Read once from `data/raw` (warmed at start-up). Orphanet relations are stated from the code's
//! side: NTBT (ORPHAcode narrower than the code) → `code_broader`; BTNT → `code_narrower`.
//! A condition without an exact code of its own is a policy-relevant gap (invisible in health
//! statistics); `icd_gap` says so.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use atlas_core::provenance::EntityIdx;
use atlas_core::{Atlas, DiseaseIdx};
use serde_json::{Value, json};

pub struct Codes {
    by_id: HashMap<String, Vec<Value>>,
}

fn orphanet_relation(r: &str) -> &'static str {
    match r {
        "E" => "exact",
        "NTBT" => "code_broader",
        "BTNT" => "code_narrower",
        "ND" => "not_decided",
        "W" => "wrong_deprecated",
        _ => "unknown",
    }
}

fn entity_json(atlas: &Atlas, file: &str) -> Value {
    atlas
        .provenance
        .entities
        .iter()
        .find(|e| e.file.ends_with(file))
        .map_or(json!({ "file": file }), |e| {
            json!({ "id": e.id, "file": e.file, "url": e.url, "version": e.version, "retrieved_at": e.retrieved_at, "sha256": e.sha256 })
        })
}

fn build(atlas: &Atlas, data: &Path) -> Codes {
    let raw = atlas_ingest::raw_dir(data);
    let mut by_id: HashMap<String, Vec<Value>> = HashMap::new();
    let orpha_entity = entity_json(atlas, "en_product1.xml");
    match atlas_ingest::orphanet::read_disorders(&raw.join("en_product1.xml"), EntityIdx(0)) {
        Ok((disorders, _)) => {
            for (orpha, d) in disorders {
                for m in d.mappings.iter().filter(|m| m.source.starts_with("ICD-1")) {
                    let code = json!({
                        "system": m.source, "code": m.reference, "relation": orphanet_relation(&m.relation),
                        "raw_relation": m.relation, "validated": m.validated, "source": "Orphanet", "via": orpha,
                        "locator": format!("Disorder[OrphaCode={}]/ExternalReferenceList", orpha.trim_start_matches("ORPHA:")),
                        "entity": orpha_entity,
                    });
                    by_id.entry(orpha.clone()).or_default().push(code.clone());
                    by_id
                        .entry(orpha.replace("ORPHA:", "Orphanet:"))
                        .or_default()
                        .push(code);
                }
            }
        }
        Err(e) => eprintln!("icd codes: orphanet unavailable: {e}"),
    }
    let mondo_entity = entity_json(atlas, "mondo.obo");
    match atlas_ingest::obo::read_obo(&raw.join("mondo.obo")) {
        Ok(obo) => {
            for t in obo.terms.iter().filter(|t| t.id.starts_with("MONDO:") && !t.obsolete) {
                for x in &t.xrefs {
                    let (system, code) = match x.id.split_once(':') {
                        Some(("ICD10CM" | "icd10cm", c)) => ("ICD-10-CM", c),
                        Some(("icd11.foundation", c)) => ("ICD-11 Foundation", c),
                        Some(("ICD10WHO" | "icd10who", c)) => ("ICD-10", c),
                        _ => continue,
                    };
                    let exact = x.sources.iter().any(|s| s == "MONDO:equivalentTo");
                    by_id.entry(t.id.clone()).or_default().push(json!({
                        "system": system, "code": code,
                        "relation": if exact { "exact" } else { "related" },
                        "raw_relation": x.sources, "validated": Value::Null, "source": "MONDO", "via": t.id,
                        "locator": format!("[Term] id: {} xref: {}", t.id, x.id), "entity": mondo_entity,
                    }));
                }
            }
        }
        Err(e) => eprintln!("icd codes: mondo unavailable: {e}"),
    }
    Codes { by_id }
}

pub fn load(atlas: &Atlas, data: &Path) -> &'static Codes {
    static C: OnceLock<Codes> = OnceLock::new();
    C.get_or_init(|| build(atlas, data))
}

/// `codes` (deduplicated by system, code, relation, source) and the `icd_gap` flag.
pub fn for_condition(atlas: &Atlas, codes: &Codes, d: DiseaseIdx) -> (Vec<Value>, Value) {
    let dis = atlas.disease_at(d);
    let mut out: Vec<Value> = Vec::new();
    let ids = std::iter::once(&dis.id).chain(dis.source_ids.iter());
    for id in ids {
        for c in codes.by_id.get(id).into_iter().flatten() {
            let dup = out.iter().any(|o| {
                o["system"] == c["system"]
                    && o["code"] == c["code"]
                    && o["relation"] == c["relation"]
                    && o["source"] == c["source"]
            });
            if !dup {
                out.push(c.clone());
            }
        }
    }
    // Own code = an exact (Orphanet E / MONDO equivalentTo) WHO ICD-10 or ICD-11 mapping only.
    let exact = out.iter().any(|c| {
        c["relation"] == "exact"
            && c["system"]
                .as_str()
                .is_some_and(|s| s == "ICD-10" || s.starts_with("ICD-11"))
    });
    for c in &mut out {
        let (sys, code) = (
            c["system"].as_str().unwrap_or("").to_owned(),
            c["code"].as_str().unwrap_or("").to_owned(),
        );
        let (key, text) = match c["relation"].as_str().unwrap_or("") {
            "exact" => ("codes.icd.display.exact", format!("{sys} {code} (its own code)")),
            "code_broader" => (
                "codes.icd.display.classified_under",
                format!("classified under {sys} {code}"),
            ),
            "code_narrower" => ("codes.icd.display.narrower", format!("{sys} {code} covers part of it")),
            _ => (
                "codes.icd.display.related",
                format!("{sys} {code} (relation not decided)"),
            ),
        };
        c["display"] =
            json!({ "text": text, "msg": { "key": key, "params": { "system": sys, "code": code }, "fallback": text } });
    }
    let under: Vec<String> = out
        .iter()
        .filter(|c| c["relation"] == "code_broader")
        .filter_map(|c| Some(format!("{} {}", c["system"].as_str()?, c["code"].as_str()?)))
        .collect();
    let text = if exact {
        "Has its own ICD code.".to_owned()
    } else if out.is_empty() {
        "Orphanet and Mondo list no ICD code for this condition, so health statistics cannot count it separately."
            .to_owned()
    } else {
        format!(
            "Has no ICD code of its own{}, so health statistics count it together with other conditions.",
            if under.is_empty() {
                String::new()
            } else {
                format!(" (it falls under {})", under.join(", "))
            }
        )
    };
    let key = if exact {
        "codes.icd.own"
    } else if out.is_empty() {
        "codes.icd.none"
    } else {
        "codes.icd.only_broader"
    };
    let gap = json!({ "has_own_code": exact, "gap": !exact, "classified_under": under,
        "rule": "own code = exact (Orphanet E or MONDO equivalentTo) WHO ICD-10/ICD-11 mapping",
        "msg": { "key": key, "params": { "classified_under": under.join(", ") }, "fallback": text }, "text": text });
    (out, gap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphanet_relations_are_from_the_code_side() {
        assert_eq!(orphanet_relation("E"), "exact");
        assert_eq!(orphanet_relation("NTBT"), "code_broader");
        assert_eq!(orphanet_relation("BTNT"), "code_narrower");
    }

    /// Real snapshot + raw files only (read, never written).
    #[test]
    fn stxbp1_codes_are_typed_links() {
        let data = atlas_ingest::data_dir();
        let snap = atlas_ingest::snapshot_path(&data);
        let raw = atlas_ingest::raw_dir(&data);
        if !snap.exists() || !raw.join("en_product1.xml").exists() || !raw.join("mondo.obo").exists() {
            eprintln!("skipped: data missing");
            return;
        }
        let atlas = atlas_core::snapshot::load(&snap).unwrap().0;
        let codes = load(&atlas, &data);
        let d = atlas.disease_idx("MONDO:0012812").unwrap();
        let (c, gap) = for_condition(&atlas, codes, d);
        assert!(!c.is_empty(), "DEE4 has ICD mappings");
        assert!(c.iter().all(|x| x["relation"].is_string() && x["entity"].is_object()));
        assert!(gap["has_own_code"].is_boolean());
    }
}
