//! Current GARD assertions, plus MONDO xrefs as candidates. Legacy IDs are never numerically migrated.
use super::*;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn gard_id(value: &str) -> Option<String> {
    let local = value.strip_prefix("GARD:").unwrap_or(value);
    if local.is_empty() || local.len() > 7 || !local.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = local.parse().ok()?;
    (n > 0).then(|| format!("GARD:{n:07}"))
}

/// Verified immutable HTTP input; raw response hash and parsed body must agree with its wrapper.
pub fn http_input(set: &mut MappingSet, e: &Value, license: &str) -> Result<Option<usize>> {
    if e["status"].as_u64() != Some(200) {
        return Ok(None);
    }
    let p = Path::new(e["raw_path"].as_str().context("HTTP raw_path absent")?);
    let mut input = Input::from_file(
        "public API response",
        p,
        e["url"].as_str().context("HTTP URL absent")?,
        e["version"].as_str().context("HTTP version absent")?,
        license,
    )?;
    ensure!(
        Some(input.sha256.as_str()) == e["sha256"].as_str(),
        "HTTP evidence hash mismatch"
    );
    let value: Value = serde_json::from_slice(&std::fs::read(p)?)?;
    ensure!(value == e["response"], "HTTP wrapper body differs from hashed response");
    input.retrieved_at = Some(e["retrieved_at"].as_str().context("HTTP retrieval time absent")?.into());
    Ok(Some(set.add_input(input)))
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let mut set = MappingSet {
        id: "gard-xrefs".into(),
        description: "Current GARD asserted disease identifiers; uncorroborated MONDO GARD xrefs remain candidates"
            .into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: base_curies(),
        ..Default::default()
    };
    for (k, u) in [
        ("MONDO", "http://purl.obolibrary.org/obo/MONDO_"),
        ("ORPHA", "http://www.orpha.net/ORDO/Orphanet_"),
        ("OMIM", "https://omim.org/entry/"),
        ("GARD", "https://rarediseases.info.nih.gov/diseases/"),
    ] {
        set.curie_map.insert(k.into(), u.into());
    }
    let current = latest(data, "gard", "diseases.trimmed.json")?;
    let index: Value = serde_json::from_slice(&std::fs::read(&current)?)?;
    set.add_input(Input::from_file(
        "current GARD index (ids only)",
        &current,
        "https://rarediseases.info.nih.gov/assets/diseases.trimmed.json",
        "current-index",
        "unknown: identifiers and links only",
    )?);
    let live: HashSet<String> = index
        .as_array()
        .context("GARD index is not an array")?
        .iter()
        .filter_map(|r| gard_id(&r["id"].to_string()))
        .collect();
    let obo_path = data.join("raw/mondo.obo");
    let i_obo = set.add_input(Input::from_file(
        "MONDO OBO xrefs",
        &obo_path,
        "http://purl.obolibrary.org/obo/mondo.obo",
        "atlas-pinned",
        "CC-BY-4.0",
    )?);
    let (mut id, mut label) = (String::new(), String::new());
    let mut retired = HashSet::new();
    let mut xrefs = Vec::new();
    for (n, l) in lines(&obo_path)?.lines().enumerate() {
        let l = l?;
        if l.starts_with('[') {
            id.clear();
            label.clear();
        }
        if let Some(s) = l.strip_prefix("id: MONDO:") {
            id = format!("MONDO:{s}");
        }
        if let Some(s) = l.strip_prefix("name: ") {
            label = s.into();
        }
        if l == "is_obsolete: true" {
            retired.insert(id.clone());
        }
        if let Some(g) = l
            .strip_prefix("xref: GARD:")
            .and_then(|s| gard_id(s.split_whitespace().next().unwrap_or("")))
            && !id.is_empty()
        {
            xrefs.push((id.clone(), label.clone(), g, n + 1));
        }
    }
    let mut asserted: HashMap<String, HashSet<String>> = HashMap::new();
    // Existing GARD details and freshly acquired bounded details both use GARD's explicit Xref_IDs__c.
    let mut details: Vec<(Value, usize, String)> = Vec::new();
    for entry in std::fs::read_dir(current.parent().unwrap())? {
        let p = entry?.path();
        if !p.file_name().unwrap().to_string_lossy().starts_with("disease-") {
            continue;
        }
        let v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
        let gid = v["id"].as_u64().context("GARD detail id absent")?;
        let url = format!("https://rarediseases.info.nih.gov/assets/singles/{gid}.json");
        let i = set.add_input(Input::from_file(
            "GARD detail explicit identifiers",
            &p,
            &url,
            "current-detail",
            "unknown: identifiers and links only",
        )?);
        details.push((v, i, url));
    }
    let p = data.join("cache/align/gard/details.json");
    if p.exists() {
        let v: Value = serde_json::from_slice(&std::fs::read(p)?)?;
        for r in v["records"].as_array().into_iter().flatten() {
            let e = &r["evidence"];
            if let Some(i) = http_input(&mut set, e, "unknown: identifiers and links only")? {
                details.push((e["response"].clone(), i, e["url"].as_str().unwrap().into()));
            } else {
                set.exclude("GARD detail unavailable/blocked", r["gard_id"].to_string());
            }
        }
    }
    let syntax = regex::Regex::new(r"^(MONDO:[0-9]{7}|ORPHA:[0-9]+|OMIM:[0-9]{6})$")?;
    let mut seen = HashSet::new();
    for (v, i, url) in details {
        let Some(g) = gard_id(v["DiseaseID__c"].as_str().unwrap_or("")) else {
            set.exclude_from(Some(i), "invalid GARD detail id", url);
            continue;
        };
        if gard_id(&v["id"].to_string()).as_ref() != Some(&g) {
            set.exclude_from(Some(i), "detail id and DiseaseID__c disagree", url);
            continue;
        }
        for s in v["Xref_IDs__c"]
            .as_str()
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .filter(|s| syntax.is_match(s))
        {
            if !seen.insert((s.to_string(), g.clone())) {
                continue;
            }
            let mut row = Row::xref(s, &g, i, "/Xref_IDs__c");
            row.evidence_url = url.clone();
            row.subject_source = format!("infores:{}", prefix(s).to_lowercase());
            row.object_source = "infores:gard".into();
            row.comment = "GARD itself asserts the disease cross-reference in Xref_IDs__c; current index checked; identifiers only".into();
            if !live.contains(&g) || v["IsDeleted"].as_bool() == Some(true) || retired.contains(s) {
                row.demote("retired");
            }
            if v["IsDeleted"].as_bool().is_none() {
                row.demote("current_status_unverified");
            }
            asserted.entry(s.into()).or_default().insert(g.clone());
            set.rows.push(row);
        }
    }
    for (m, label, g, n) in xrefs {
        let direct = asserted.get(&m).is_some_and(|gs| gs.contains(&g));
        let mut row = Row::link(&m, CLOSE, &g, i_obo, format!("line {n}"));
        row.subject_label = label;
        row.confidence = 0.8;
        row.justification = XREF.into();
        row.subject_source = "infores:mondo".into();
        row.object_source = "infores:gard".into();
        row.evidence_url = format!("http://purl.obolibrary.org/obo/{}", m.replace(':', "_"));
        row.comment = "MONDO-only GARD assertion: candidate until current GARD or independent sources confirm".into();
        if direct {
            row.predicate = EXACT.into();
            row.asserted = EXACT.into();
            row.confidence = 1.0;
            row.comment = "MONDO GARD xref corroborated by current GARD's own explicit MONDO identifier".into();
        }
        if retired.contains(&m) {
            row.demote("retired");
        }
        if !live.contains(&g) {
            row.demote("current_id_unverified");
        }
        if asserted.get(&m).is_some_and(|gs| !gs.contains(&g)) {
            row.demote("cross_source");
        }
        set.rows.push(row);
    }
    // A source read as non-exact is not a contradiction when it is merely awaiting corroboration.
    check_cardinality(&mut set);
    check_clusters_ordered(&mut [&mut set], &[], &["GARD", "OMIM", "ORPHA", "MONDO"]);
    check_background(&mut set, data, &["disease-xrefs", "disease-orphanet-xrefs"], "GARD")?;
    set.parameters = json!({"exact": "current GARD explicitly asserts MONDO/ORPHA/OMIM in Xref_IDs__c; or two independent assertions agreeing on current id", "current": "id in GARD current index and IsDeleted false", "wikidata": "P4317 is CC0 but copied MONDO references must not count as independent", "legacy_ids": "no arithmetic renumbering; preserve old candidate and flag disagreement"});
    set.extra.insert(
        "rights_scope".into(),
        json!("GARD identifiers and links only; no GARD text/labels released"),
    );
    set.notes.push("2021+ GARD identifier renumbering/migration is a known task constraint, not an arithmetic transformation. Current details and legacy MONDO can disagree; absent or conflicting IDs remain candidates. No authoritative old-to-new table was verified in this run.".into());
    Ok(vec![set])
}
