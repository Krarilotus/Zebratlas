//! Sponsors and grant institutions to ROR: API and metadata evidence retrieve candidates only.
use super::*;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Corroboration ranks a candidate; API choice, scores and inspection never establish identity.
pub fn chosen_corroborated(chosen: bool, active: bool, countries_agree: bool, websites_agree: bool) -> bool {
    chosen && active && countries_agree && websites_agree
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let mut set = MappingSet {
        id: "org-ror-affiliation".into(),
        description: "ClinicalTrials.gov sponsors/collaborators and RePORTER/CORDIS institutions to ROR"
            .into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: base_curies(),
        ..Default::default()
    };
    for (p, u) in [
        ("ROR", "https://ror.org/"),
        ("cordis.org", "https://cordis.europa.eu/organisation/"),
        (
            "reporter.org",
            "https://w3id.org/rare-atlas/reporter-organisation/",
        ),
        ("ctgov.org", "https://w3id.org/rare-atlas/ctgov-organisation/"),
    ] {
        set.curie_map.insert(p.into(), u.into());
    }
    let lookup_path = data.join("cache/align/affiliation/lookups.json");
    if !lookup_path.exists() {
        set.notes
            .push("No cached affiliation requests; nothing invented".into());
        return Ok(vec![set]);
    }
    let mut dirs: Vec<_> = std::fs::read_dir(data.join("raw/ror"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("extracted"))
        .collect();
    dirs.sort();
    let csv = std::fs::read_dir(dirs.last().context("ROR dump absent")?)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "csv"))
        .context("ROR CSV absent")?;
    let idx = org::load_ror(&csv)?;
    let i_ror = set.add_input(Input::from_file(
        "ROR pinned dump",
        &csv,
        "https://zenodo.org/communities/ror-data",
        "pinned-dump",
        "CC0-1.0",
    )?);
    let lookup_input = Input::from_file(
        "affiliation request plan and responses",
        &lookup_path,
        "https://api.ror.org/v2/organizations?affiliation=",
        "1",
        "identifiers only",
    )?;
    let lookup_hash = lookup_input.sha256.clone();
    set.add_input(lookup_input);
    let gate_path = data.join("cache/align/affiliation/precision-gate.json");
    let gate = if gate_path.exists() {
        let g: Value = serde_json::from_slice(&std::fs::read(&gate_path)?)?;
        set.add_input(Input::from_file(
            "agent-inspected precision gate",
            &gate_path,
            "local:agent-inspection",
            "1",
            "CC-BY-4.0",
        )?);
        g["lookups_sha256"].as_str() == Some(&lookup_hash)
            && g["sample_size"].as_u64().unwrap_or(0) >= 20
            && g["all_correct"].as_bool() == Some(true)
            && g["inspection"].as_str() == Some("agent-inspected")
    } else {
        false
    };
    let v: Value = serde_json::from_slice(&std::fs::read(lookup_path)?)?;
    let mut seen = HashSet::new();
    let ror_syntax = regex::Regex::new(r"^0[a-z0-9]{6}[0-9]{2}$")?;
    for m in v["records"].as_array().into_iter().flatten() {
        let id = m["id"].as_str().unwrap_or("");
        let name = m["name"].as_str().unwrap_or("");
        if id.is_empty() || name.is_empty() {
            set.exclude("missing source organisation identifier/name", "unusable mention");
            continue;
        }
        // Rehash original source: a generated query is not evidence for the source organisation.
        let source_path = Path::new(
            m["source_path"]
                .as_str()
                .context("organisation source path absent")?,
        );
        ensure!(
            source_path
                .canonicalize()?
                .starts_with(data.join("cache").canonicalize()?),
            "organisation input outside shared cache"
        );
        let mut input = Input::from_file(
            "source organisation mention",
            source_path,
            m["source_url"].as_str().unwrap_or(""),
            "cache-v1",
            "identifier facts",
        )?;
        ensure!(
            Some(input.sha256.as_str()) == m["source_sha256"].as_str(),
            "organisation source hash mismatch"
        );
        input.retrieved_at = m["source_retrieved_at"].as_str().map(String::from);
        let i_source = set.add_input(input);
        let country = org::iso2(m["country"].as_str().unwrap_or(""));
        let host = org::host(m["website"].as_str().unwrap_or(""));
        let e = &m["evidence"];
        if let Some(i_api) = gard::http_input(&mut set, e, "CC0-1.0")? {
            let items = e["response"]["items"]
                .as_array()
                .context("ROR affiliation items absent")?;
            let chosen_count = items
                .iter()
                .filter(|r| r["chosen"].as_bool() == Some(true))
                .count();
            for (n, item) in items.iter().enumerate() {
                let r = &item["organization"];
                let rid = r["id"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("https://ror.org/");
                if !ror_syntax.is_match(rid) {
                    set.exclude("invalid ROR identifier", format!("{id} items[{n}]"));
                    continue;
                }
                let cc = r["locations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|l| l["geonames_details"]["country_code"].as_str() == Some(&country));
                let hosts: Vec<String> = r["links"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|l| l["type"] == "website")
                    .filter_map(|l| l["value"].as_str())
                    .map(org::host)
                    .collect();
                let web = !host.is_empty() && hosts.contains(&host);
                let chosen = item["chosen"].as_bool() == Some(true) && chosen_count == 1;
                let corroborated =
                    chosen_corroborated(chosen, r["status"] == "active", !country.is_empty() && cc, web);
                let mut row = Row::link(id, CLOSE, &format!("ROR:{rid}"), i_api, format!("/items/{n}"));
                row.subject_label = name.into();
                row.justification = "semapv:CompositeMatching".into();
                row.confidence = if corroborated { 0.95 } else { 0.6 };
                row.subject_source = format!("infores:{}", prefix(id));
                row.object_source = "infores:ror".into();
                row.evidence_url = e["url"].as_str().unwrap_or("").into();
                row.comment = format!(
                    "candidate only, no explicit source equivalence; chosen={chosen}; country_check={cc}; website_check={web}; inspected_gate={gate} (not identity authorization); source input {i_source} {}",
                    m["source_locator"]
                );
                if r["status"] != "active" {
                    row.demote("retired");
                }
                if !country.is_empty() && !cc {
                    row.demote("country_mismatch");
                }
                if !host.is_empty() && !hosts.is_empty() && !web {
                    row.demote("website_mismatch");
                }
                if chosen_count > 1 {
                    row.demote("ambiguous_chosen");
                }
                set.rows.push(row);
            }
        } else {
            // Acquisition blocked: dump name matches are candidates regardless of perfect metadata.
            let matches = idx
                .by_name
                .get(&org::norm_name(name))
                .cloned()
                .unwrap_or_default();
            let ambiguous = matches.len() > 1;
            if matches.is_empty() {
                set.exclude("API blocked/unavailable and no dump name candidate", id);
            }
            for n in matches {
                let r = &idx.recs[n];
                if !seen.insert((id.to_string(), r.id.clone())) {
                    continue;
                }
                let mut row = Row::link(
                    id,
                    CLOSE,
                    &r.id,
                    i_source,
                    m["source_locator"].as_str().unwrap_or(""),
                );
                row.subject_label = name.into();
                row.object_label = r.display.clone();
                row.justification = LEXICAL.into();
                row.confidence = 0.6;
                row.subject_source = format!("infores:{}", prefix(id));
                row.object_source = "infores:ror".into();
                row.evidence_url = m["source_url"].as_str().unwrap_or("").into();
                row.comment = format!(
                    "ROR API blocked/unavailable; name-only dump candidate; ROR input {i_ror}, id {}; source country {}; ROR country {}; website checked={}; chosen unverified",
                    r.id,
                    country,
                    r.country,
                    !host.is_empty() && r.hosts.contains(&host)
                );
                if !r.active {
                    row.demote("retired");
                }
                if ambiguous {
                    row.demote("ambiguous_name");
                }
                if !country.is_empty() && !r.countries.contains(&country) {
                    row.demote("country_mismatch");
                }
                if !host.is_empty() && !r.hosts.is_empty() && !r.hosts.contains(&host) {
                    row.demote("website_mismatch");
                }
                set.rows.push(row);
            }
        }
    }
    check_cardinality(&mut set);
    check_clusters(&mut [&mut set], &[]);
    check_background(&mut set, data, &["org-ror", "funder-ror"], "ROR")?;
    set.parameters = json!({"exact":"none: no explicit source equivalence; API chosen, metadata and agent inspection retrieve candidates only", "country":"sponsor/organisation country only, never study location", "fallback":"dump name equality is candidate only", "scope":"sha256 source id, stratified 40 CT.gov / 40 RePORTER / 20 CORDIS; eligible/selected counts in cache/align/affiliation/scope.json"});
    set.extra
        .insert("precision_candidate_sample_size".into(), json!(20));
    set.notes.push("ROR acquisition blocked at robots HTTP 403; final block, no retry or alternate route. Existing ROR CC0 dump used for candidates only.".into());
    Ok(vec![set])
}
