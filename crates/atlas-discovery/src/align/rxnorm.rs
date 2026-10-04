//! Public RxNav UNII attributes, never UMLS-licensed bulk downloads or drug-name merges.
use super::*;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

pub fn ingredient(tty: &str) -> bool {
    matches!(tty, "IN" | "PIN")
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let mut set = MappingSet {
        id: "rxnorm-xrefs".into(),
        description: "Public RxNav UNII to active ingredient RxCUIs; products/formulations are typed candidates".into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: base_curies(),
        ..Default::default()
    };
    for (p, u) in [
        ("UNII", "https://precision.fda.gov/uniisearch/srs/unii/"),
        ("RXNORM", "https://rxnav.nlm.nih.gov/REST/rxcui/"),
    ] {
        set.curie_map.insert(p.into(), u.into());
    }
    let p = data.join("cache/align/rxnorm/lookups.json");
    if !p.exists() {
        set.notes
            .push("No cached public RxNav evidence; no mapping invented".into());
        return Ok(vec![set]);
    }
    let v: Value = serde_json::from_slice(&std::fs::read(p)?)?;
    if let Some(r) = v["records"].as_array().and_then(|rs| rs.first())
        && let Some(i) = gard::http_input(&mut set, &r["rxnorm_version"], "public-domain (NLM)")?
    {
        set.extra.insert(
            "rxnorm_database_version".into(),
            r["rxnorm_version"]["response"].clone(),
        );
        set.extra.insert("version_input".into(), json!(i));
    }
    let props_path = data.join("cache/align/rxnorm/properties.json");
    let props: Value = if props_path.exists() {
        serde_json::from_slice(&std::fs::read(props_path)?)?
    } else {
        json!({"records":[]})
    };
    let mut properties = std::collections::HashMap::new();
    for p in props["records"].as_array().into_iter().flatten() {
        if let Some(i) = gard::http_input(&mut set, &p["evidence"], "public-domain (NLM non-proprietary RxNorm)")? {
            properties.insert(
                p["rxcui"].as_str().unwrap_or("").to_string(),
                (p["evidence"]["response"]["properties"].clone(), i),
            );
        }
    }
    let syntax = regex::Regex::new(r"^[A-Z0-9]{10}$")?;
    let mut seen = HashSet::new();
    for r in v["records"].as_array().into_iter().flatten() {
        let unii = r["unii"].as_str().unwrap_or("");
        if !syntax.is_match(unii) {
            set.exclude("invalid UNII syntax", unii);
            continue;
        }
        let Some(i) = gard::http_input(&mut set, &r["evidence"], "public-domain (NLM non-proprietary RxNorm)")? else {
            set.exclude("RxNav response unavailable/blocked", unii);
            continue;
        };
        let ids: Vec<&str> = r["evidence"]["response"]["idGroup"]["rxnormId"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if ids.is_empty() {
            set.exclude_from(Some(i), "no active RxCUI returned by public UNII lookup", unii);
        }
        for id in ids {
            if id.is_empty()
                || !id.bytes().all(|b| b.is_ascii_digit())
                || !seen.insert((unii.to_string(), id.to_string()))
            {
                continue;
            }
            let mut row = Row::xref(&format!("UNII:{unii}"), &format!("RXNORM:{id}"), i, "/idGroup/rxnormId");
            row.evidence_url = r["evidence"]["url"].as_str().unwrap_or("").into();
            row.subject_source = "infores:fda-srs".into();
            row.object_source = "infores:rxnorm".into();
            row.comment = "RxNav findRxcuiById UNII_CODE, allsrc=0 (active); source-asserted identifier".into();
            match properties.get(id) {
                Some((p, pi))
                    if ingredient(p["tty"].as_str().unwrap_or(""))
                        && p["rxcui"].as_str() == Some(id)
                        && p["suppress"].as_str() == Some("N") =>
                {
                    row.comment
                        .push_str(&format!("; ingredient properties input {pi}, /properties"));
                }
                Some((p, _)) => {
                    row.predicate = CLOSE.into();
                    row.asserted = CLOSE.into();
                    row.confidence = 0.8;
                    row.comment.push_str(&format!("; term type {} is not an active IN/PIN ingredient (product association, never chemical identity)", p["tty"]));
                }
                None => {
                    row.predicate = CLOSE.into();
                    row.asserted = CLOSE.into();
                    row.confidence = 0.8;
                    row.comment.push_str("; term type unverified: candidate only");
                }
            }
            set.rows.push(row);
        }
    }
    check_cardinality(&mut set);
    check_clusters(&mut [&mut set], &[]);
    check_background(&mut set, data, &["drug-xrefs"], "RXNORM")?;
    set.parameters = json!({"public_api":"https://rxnav.nlm.nih.gov/REST/rxcui.json?idtype=UNII_CODE&id=<UNII>&allsrc=0", "exact":"RxNav asserts UNII to active IN/PIN ingredient; 1:1 and cluster-consistent", "requests_per_second_max":2, "terms":"https://lhncbc.nlm.nih.gov/RxNav/TermsofService.html", "wikidata":"P3345 CC0 corroboration optional; alone never exact"});
    set.notes.push("NLM attribution: This product uses publicly available data from the U.S. National Library of Medicine (NLM), National Institutes of Health, Department of Health and Human Services; NLM is not responsible for the product and does not endorse or recommend it. No licensed UMLS or proprietary endpoint used.".into());
    set.extra.insert("precision_candidate_sample_size".into(), json!(20));
    Ok(vec![set])
}
