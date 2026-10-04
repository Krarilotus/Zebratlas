//! Works: PMID = DOI = PMCID for the papers in our caches. PubMed and Europe PMC records assert the ids;
//! NCBI's PMC-ids file (streamed, filtered to our works) confirms them, adds missing PMCIDs and exposes
//! disagreements as conflicts.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use super::*;

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (k, v) in [
        ("PMID", "https://pubmed.ncbi.nlm.nih.gov/"),
        ("DOI", "https://doi.org/"),
        ("PMCID", "https://www.ncbi.nlm.nih.gov/pmc/articles/"),
        ("PPR", "https://europepmc.org/article/PPR/"),
        ("AGR", "https://europepmc.org/article/AGR/"),
        ("CBA", "https://europepmc.org/article/CBA/"),
        ("ETH", "https://europepmc.org/article/ETH/"),
        ("CTX", "https://europepmc.org/article/CTX/"),
        ("HIR", "https://europepmc.org/article/HIR/"),
    ] {
        m.insert(k.into(), v.into());
    }
    m
}

fn doi(s: &str) -> Option<String> {
    let d = s
        .trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("doi:")
        .to_lowercase();
    super::safety::valid_doi(&d).then_some(format!("DOI:{d}"))
}

fn pmcid(s: &str) -> Option<String> {
    let p = s.trim().to_uppercase();
    let p = if p.starts_with("PMC") { p } else { format!("PMC{p}") };
    (p.len() > 3 && p[3..].chars().all(|c| c.is_ascii_digit())).then_some(format!("PMCID:{p}"))
}

fn pmid(s: &str) -> Option<String> {
    let p = s.trim().trim_start_matches("PMID:");
    (!p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).then_some(format!("PMID:{p}"))
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let mut set = MappingSet {
        id: "work-ids".into(),
        description:
            "Works in our caches: PMID = DOI = PMCID (PubMed / Europe PMC records), confirmed against NCBI PMC-ids"
                .into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let cache = data.join("cache");
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut push =
        |set: &mut MappingSet, s: String, o: String, input: usize, loc: String, src: &str, url: &str, label: &str| {
            if !seen.insert((s.clone(), o.clone())) {
                return;
            }
            let mut r = Row::xref(&s, &o, input, loc);
            r.subject_label = label.chars().take(200).collect();
            r.subject_source = src.into();
            r.object_source = src.into();
            r.evidence_url = url.into();
            r.comment = format!("ids of one record in {src}");
            set.rows.push(r);
        };
    let dir = cache.join("pubmed");
    if dir.exists() {
        let mut files: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for p in files {
            let i = set.add_input(Input::from_file(
                "PubMed cache",
                &p,
                "https://eutils.ncbi.nlm.nih.gov/",
                "2026-10-03",
                "public-domain (NLM metadata)",
            )?);
            let v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
            for (k, r) in v["records"].as_array().into_iter().flatten().enumerate() {
                let Some(pm) = r["id"].as_str().and_then(pmid) else {
                    set.exclude_from(Some(i), "invalid/missing PMID", format!("records[{k}].id"));
                    continue;
                };
                let (url, title) = (r["url"].as_str().unwrap_or(""), r["title"].as_str().unwrap_or(""));
                if let Some(d) = r["doi"].as_str().and_then(doi) {
                    push(
                        &mut set,
                        pm.clone(),
                        d,
                        i,
                        format!("records[{k}].doi"),
                        "infores:pubmed",
                        url,
                        title,
                    );
                } else if r["doi"].as_str().is_some_and(|s| !s.trim().is_empty()) {
                    set.exclude_from(
                        Some(i),
                        "invalid DOI syntax",
                        format!("records[{k}].doi original={}", r["doi"]),
                    );
                }
                if let Some(c) = r["pmcid"].as_str().and_then(pmcid) {
                    push(
                        &mut set,
                        pm.clone(),
                        c,
                        i,
                        format!("records[{k}].pmcid"),
                        "infores:pubmed",
                        url,
                        title,
                    );
                }
            }
        }
    }
    for f in ["europepmc_preprints", "europepmc_other"] {
        let p = cache.join("research_intl").join(format!("{f}.json"));
        if !p.exists() {
            continue;
        }
        let i = set.add_input(Input::from_file(
            "Europe PMC cache",
            &p,
            "https://www.ebi.ac.uk/europepmc/webservices/rest/",
            "2026-10-03",
            "CC0-1.0 (Europe PMC metadata)",
        )?);
        let v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
        for (k, r) in v["records"].as_array().into_iter().flatten().enumerate() {
            let Some(id) = r["id"].as_str().map(String::from) else {
                continue;
            };
            let (url, title) = (r["url"].as_str().unwrap_or(""), r["title"].as_str().unwrap_or(""));
            for (field, val) in [
                ("doi", r["doi"].as_str().and_then(doi)),
                ("pmid", r["pmid"].as_str().and_then(pmid)),
                ("pmcid", r["pmcid"].as_str().and_then(pmcid)),
            ] {
                if let Some(o) = val {
                    push(
                        &mut set,
                        id.clone(),
                        o,
                        i,
                        format!("records[{k}].{field}"),
                        "infores:europepmc",
                        url,
                        title,
                    );
                } else if r[field].as_str().is_some_and(|s| !s.trim().is_empty()) {
                    set.exclude_from(
                        Some(i),
                        "invalid work identifier syntax",
                        format!("records[{k}].{field} original={}", r[field]),
                    );
                }
            }
        }
    }

    // NCBI PMC-ids: stream, keep rows touching our PMIDs/DOIs.
    let ours_pmid: HashSet<String> = set
        .rows
        .iter()
        .filter(|r| r.subject_id.starts_with("PMID:"))
        .map(|r| r.subject_id.clone())
        .collect();
    let ours_doi: HashSet<String> = set
        .rows
        .iter()
        .filter(|r| r.object_id.starts_with("DOI:"))
        .map(|r| r.object_id.clone())
        .collect();
    let mut asserted: HashMap<(String, &'static str), String> = HashMap::new();
    for r in &set.rows {
        if r.subject_id.starts_with("PMID:") {
            let kind = if r.object_id.starts_with("DOI:") {
                "DOI"
            } else {
                "PMCID"
            };
            asserted.insert((r.subject_id.clone(), kind), r.object_id.clone());
        }
    }
    let pos: HashMap<(String, String), usize> = set
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| ((r.subject_id.clone(), r.object_id.clone()), i))
        .collect();
    let mut stats: BTreeMap<&str, u64> = BTreeMap::new();
    if let Ok(p) = latest(data, "pmc-ids", "PMC-ids.csv.gz") {
        let i = set.add_input(Input::from_file("NCBI PMC-ids", &p, "", "", "public-domain")?);
        let mut header: Vec<String> = vec![];
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(lines(&p)?);
        for (n, rec) in rdr.records().enumerate() {
            let rec = rec?;
            if header.is_empty() {
                header = rec.iter().map(String::from).collect();
                continue;
            }
            let col = |name: &str| {
                header
                    .iter()
                    .position(|h| h == name)
                    .and_then(|x| rec.get(x))
                    .unwrap_or("")
            };
            let pm = pmid(col("PMID"));
            let d = doi(col("DOI"));
            let c = pmcid(col("PMCID"));
            let ours =
                pm.as_ref().is_some_and(|x| ours_pmid.contains(x)) || d.as_ref().is_some_and(|x| ours_doi.contains(x));
            if !ours {
                continue;
            }
            *stats.entry("pmc_rows_for_our_works").or_default() += 1;
            let loc = format!("line {}", n + 1);
            let url = c
                .as_ref()
                .map(|c| {
                    format!(
                        "https://www.ncbi.nlm.nih.gov/pmc/articles/{}",
                        c.trim_start_matches("PMCID:")
                    )
                })
                .unwrap_or_default();
            let Some(pm) = pm else { continue };
            for (kind, val) in [("DOI", d), ("PMCID", c)] {
                let Some(o) = val else { continue };
                match asserted.get(&(pm.clone(), kind)) {
                    Some(prev) if *prev == o => {
                        *stats
                            .entry(if kind == "DOI" {
                                "doi_confirmed"
                            } else {
                                "pmcid_confirmed"
                            })
                            .or_default() += 1;
                        if let Some(&ix) = pos.get(&(pm.clone(), o.clone())) {
                            set.rows[ix].comment.push_str("; NCBI PMC-ids confirms");
                        }
                    }
                    Some(prev) => {
                        *stats.entry("disagreements").or_default() += 1;
                        let prev = prev.clone();
                        if let Some(&ix) = pos.get(&(pm.clone(), prev.clone())) {
                            set.rows[ix].demote("cross_source");
                            set.rows[ix].comment.push_str(&format!("; NCBI PMC-ids gives {o}"));
                        }
                        let mut r = Row::xref(&pm, &o, i, loc.clone());
                        r.demote("cross_source");
                        r.object_source = "infores:pmc".into();
                        r.evidence_url = url.clone();
                        r.comment = format!("NCBI PMC-ids; our record gives {prev}");
                        set.rows.push(r);
                    }
                    None => {
                        *stats
                            .entry(if kind == "DOI" { "doi_added" } else { "pmcid_added" })
                            .or_default() += 1;
                        let mut r = Row::xref(&pm, &o, i, loc.clone());
                        r.subject_source = "infores:pmc".into();
                        r.object_source = "infores:pmc".into();
                        r.evidence_url = url.clone();
                        r.comment = "NCBI PMC-ids (our record had no such id)".into();
                        set.rows.push(r);
                    }
                }
            }
        }
    }
    check_cardinality(&mut set);
    let cluster = check_clusters(&mut [&mut set], &[]);
    set.parameters = json!({"normalisation": "DOI lower-cased without resolver prefix; PMCID upper-case with PMC prefix", "rule": "exact = ids of one record (PubMed / Europe PMC / PMC-ids), not contradicted, 1:1, one id per prefix per cluster"});
    set.extra.insert("pmc_ids_stats".into(), json!(stats));
    set.extra.insert("cluster_conflict_rows".into(), json!(cluster));
    set.extra.insert("our_pmids".into(), json!(ours_pmid.len()));
    Ok(vec![set])
}
