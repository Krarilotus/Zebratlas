//! Organisations and funders → ROR.
//! `org-ror`: name, country and website agreement retrieve candidates, never exact identity.
//! Only an explicit source equivalence can authorize an organisation merge.
//! `funder-ror`: ROR's own FundRef cross-references (ROR-asserted), funder ids given by our grant
//! sources resolved through them, and funder names as candidates.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::*;

pub struct Ror {
    pub id: String,
    pub display: String,
    pub country: String,
    pub countries: Vec<String>,
    pub hosts: Vec<String>,
    pub fundref: Vec<String>,
    pub fundref_preferred: String,
    pub active: bool,
}

pub struct RorIndex {
    pub recs: Vec<Ror>,
    /// normalised name → record indices
    pub by_name: HashMap<String, Vec<usize>>,
    pub by_fundref: HashMap<String, Vec<usize>>,
}

/// Lower case, ASCII-folded where trivial, punctuation → space, collapsed, leading "the " removed.
pub fn norm_name(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Å' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ç' | 'Ç' => 'c',
            '&' => ' ',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect::<String>()
        .to_lowercase();
    let words: Vec<&str> = folded.split_whitespace().collect();
    let words = if words.first() == Some(&"the") {
        &words[1..]
    } else {
        &words[..]
    };
    words.join(" ")
}

/// Host of a URL without scheme, "www." and path; lower case.
pub fn host(url: &str) -> String {
    let u = url.trim().to_lowercase();
    let u = u.split("://").nth(1).unwrap_or(&u).to_string();
    let h = u
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string();
    h.split(':').next().unwrap_or("").to_string()
}

fn host_match(a: &str, b: &str) -> bool {
    !a.is_empty() && a == b
}

fn names_of(field: &str) -> Vec<String> {
    field
        .split("; ")
        .map(|n| n.split_once(": ").map_or(n, |(_, v)| v).trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

pub fn load_ror(path: &Path) -> Result<RorIndex> {
    let mut rdr = csv::Reader::from_path(path).with_context(|| format!("open {}", path.display()))?;
    let headers = rdr.headers()?.clone();
    let col = |n: &str| {
        headers
            .iter()
            .position(|h| h == n)
            .with_context(|| format!("ROR column {n}"))
    };
    let c_frp = col("external_ids.type.fundref.preferred")?;
    let (c_id, c_disp, c_lab, c_alias, c_cc, c_dom, c_web, c_fr, c_status) = (
        col("id")?,
        col("names.types.ror_display")?,
        col("names.types.label")?,
        col("names.types.alias")?,
        col("locations.geonames_details.country_code")?,
        col("domains")?,
        col("links.type.website")?,
        col("external_ids.type.fundref.all")?,
        col("status")?,
    );
    let mut idx = RorIndex {
        recs: vec![],
        by_name: HashMap::new(),
        by_fundref: HashMap::new(),
    };
    for row in rdr.records() {
        let row = row?;
        let i = idx.recs.len();
        let mut hosts: Vec<String> = row[c_dom]
            .split(';')
            .map(|d| host(d.trim()))
            .filter(|h| !h.is_empty())
            .collect();
        hosts.extend(
            row[c_web]
                .split(';')
                .map(|w| host(w.trim()))
                .filter(|h| !h.is_empty()),
        );
        let fundref: Vec<String> = row[c_fr]
            .split(';')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let mut names: HashSet<String> = HashSet::new();
        names.insert(norm_name(&row[c_disp]));
        for n in names_of(&row[c_lab]).into_iter().chain(names_of(&row[c_alias])) {
            names.insert(norm_name(&n));
        }
        for n in names.into_iter().filter(|n| n.len() >= 4) {
            idx.by_name.entry(n).or_default().push(i);
        }
        for f in &fundref {
            idx.by_fundref.entry(f.clone()).or_default().push(i);
        }
        idx.recs.push(Ror {
            id: format!("ROR:{}", row[c_id].trim_start_matches("https://ror.org/")),
            display: row[c_disp].to_string(),
            country: row[c_cc].split(';').next().unwrap_or("").trim().to_string(),
            countries: row[c_cc]
                .split(';')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(String::from)
                .collect(),
            hosts,
            fundref,
            fundref_preferred: row[c_frp].trim().to_string(),
            active: &row[c_status] == "active",
        });
    }
    Ok(idx)
}

fn curies() -> BTreeMap<String, String> {
    let mut m = base_curies();
    for (k, v) in [
        ("ROR", "https://ror.org/"),
        ("crossref.funder", "https://doi.org/10.13039/"),
        (
            "atlasorg",
            "https://github.com/Krarilotus/rare-disease-atlas/org/",
        ),
        ("epicare-centre", "https://epi-care.eu/ern-epicare-centres/#"),
        ("cordis.org", "https://cordis.europa.eu/organisation/"),
        ("gtr.org", "https://gtr.ukri.org/organisation/"),
        ("nih.ic", "https://w3id.org/rare-atlas/nih-ic/"),
    ] {
        m.insert(k.into(), v.into());
    }
    m
}

/// Country names used in our caches → ISO 3166-1 alpha-2.
pub(super) fn iso2(c: &str) -> String {
    let t = c.trim();
    if t.len() == 2 {
        return t.to_uppercase();
    }
    match t.to_lowercase().as_str() {
        "united states" | "usa" => "US",
        "united kingdom" | "uk" => "GB",
        "germany" => "DE",
        "france" => "FR",
        "italy" => "IT",
        "spain" => "ES",
        "netherlands" | "the netherlands" => "NL",
        "belgium" => "BE",
        "austria" => "AT",
        "switzerland" => "CH",
        "sweden" => "SE",
        "denmark" => "DK",
        "norway" => "NO",
        "finland" => "FI",
        "poland" => "PL",
        "portugal" => "PT",
        "ireland" => "IE",
        "czech republic" | "czechia" => "CZ",
        "hungary" => "HU",
        "greece" => "GR",
        "lithuania" => "LT",
        "latvia" => "LV",
        "estonia" => "EE",
        "slovenia" => "SI",
        "slovakia" => "SK",
        "croatia" => "HR",
        "romania" => "RO",
        "bulgaria" => "BG",
        "luxembourg" => "LU",
        "cyprus" => "CY",
        "malta" => "MT",
        "canada" => "CA",
        "australia" => "AU",
        "japan" => "JP",
        "china" => "CN",
        "israel" => "IL",
        _ => "",
    }
    .to_string()
}

struct OrgMention {
    id: String,
    label: String,
    names: Vec<String>,
    country: String,
    website: String,
    input: usize,
    locator: String,
    url: String,
}

/// Retrieve every alternative; even a unique name/country/host conjunction remains a candidate.
fn match_org(set: &mut MappingSet, ror: &RorIndex, m: &OrgMention) {
    let mut by_name: Vec<usize> = m
        .names
        .iter()
        .flat_map(|n| ror.by_name.get(&norm_name(n)).cloned().unwrap_or_default())
        .collect();
    by_name.sort();
    by_name.dedup();
    let h = host(&m.website);
    let in_country: Vec<usize> = by_name
        .iter()
        .copied()
        .filter(|&i| !m.country.is_empty() && ror.recs[i].countries.contains(&m.country))
        .collect();
    let full: Vec<usize> = in_country
        .iter()
        .copied()
        .filter(|&i| ror.recs[i].hosts.iter().any(|x| host_match(x, &h)))
        .collect();
    // Website alone (host equality) is evidence too, but never sufficient for exact.
    let by_host: Vec<usize> = if h.is_empty() {
        vec![]
    } else {
        ror.recs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.hosts.iter().any(|x| x == &h))
            .map(|(i, _)| i)
            .collect()
    };
    let alternatives: Vec<_> = by_name.iter().chain(&by_host).copied().collect();
    let (mut cands, pred, conf, why): (Vec<usize>, &str, f32, &str) = if full.len() == 1 {
        (
            full,
            CLOSE,
            0.95,
            "name + country + website host agree with one ROR record; no source equivalence: candidate",
        )
    } else if in_country.len() == 1 {
        (
            in_country,
            CLOSE,
            0.8,
            "name + country agree with one ROR record; website does not confirm: candidate",
        )
    } else if by_name.len() == 1 {
        (
            by_name,
            CLOSE,
            0.6,
            "name agrees with one ROR record; country/website do not confirm: candidate",
        )
    } else if by_host.len() == 1 {
        (
            by_host,
            CLOSE,
            0.6,
            "website host equals one ROR record's; name differs: candidate",
        )
    } else {
        let mut alternatives = by_name;
        alternatives.extend(by_host);
        alternatives.sort();
        alternatives.dedup();
        if alternatives.is_empty() {
            set.exclude_from(
                Some(m.input),
                "no ROR record matches name or website",
                format!("{} {}", m.id, m.locator),
            );
            return;
        }
        (
            alternatives,
            CLOSE,
            0.0,
            "ambiguous ROR alternatives retained for review",
        )
    };
    let selected: HashSet<_> = cands.iter().copied().collect();
    cands.extend(alternatives);
    cands.sort();
    cands.dedup();
    for i in cands {
        let r = &ror.recs[i];
        let pred = if selected.contains(&i) { pred } else { CLOSE };
        let mut row = Row::link(&m.id, pred, &r.id, m.input, m.locator.clone());
        row.asserted = pred.into();
        row.justification = if selected.contains(&i) && conf == 0.95 {
            "semapv:CompositeMatching".into()
        } else {
            LEXICAL.into()
        };
        row.confidence = if selected.contains(&i) { conf } else { 0.0 };
        row.subject_label = m.label.clone();
        row.object_label = r.display.clone();
        row.object_source = "infores:ror".into();
        row.evidence_url = m.url.clone();
        let row_why = if selected.contains(&i) {
            why
        } else {
            "retrieved alternative; full conjunction not established"
        };
        row.comment = format!("{row_why}; ROR country {}, hosts {:?}", r.country, r.hosts);
        if !selected.contains(&i) {
            row.comment
                .push_str("; alternative retained without full corroboration");
        }
        if !r.active {
            row.demote("retired");
        }
        set.rows.push(row);
    }
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    let ror_csv = latest(data, "ror", "extracted/v2.13-2026-09-22-ror-data.csv").or_else(|_| {
        // any extracted CSV of the newest release
        let dir = data.join("raw/ror");
        let mut v: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("extracted"))
            .collect();
        v.sort();
        let d = v.pop().context("no extracted ROR release")?;
        std::fs::read_dir(&d)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "csv"))
            .context("no ROR csv")
    })?;
    let ror = load_ror(&ror_csv)?;
    let ror_input = Input::from_file(
        "ROR data dump (CSV, unzipped)",
        &ror_csv,
        "https://zenodo.org/communities/ror-data",
        "v2.13",
        "CC0-1.0",
    )?;

    // ---- org-ror
    let mut orgs = MappingSet {
        id: "org-ror".into(),
        description: "Organisations in our caches (patient organisations, ERN EpiCARE centres, CORDIS and GtR organisations) to ROR".into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    orgs.add_input(ror_input.clone());
    let mut mentions: Vec<OrgMention> = Vec::new();
    let cache = data.join("cache");
    let read = |p: &Path| -> Result<Value> { Ok(serde_json::from_slice(&std::fs::read(p)?)?) };
    let recs = |v: &Value| -> Vec<Value> {
        v.get("records")
            .and_then(Value::as_array)
            .or_else(|| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let p = cache.join("orgs/organisations.json");
    if p.exists() {
        let i = orgs.add_input(Input::from_file(
            "curated patient organisations",
            &p,
            "",
            "2026-10-03",
            "CC-BY-4.0 (our curation)",
        )?);
        for (k, r) in recs(&read(&p)?).iter().enumerate() {
            let s = |f: &str| r[f].as_str().unwrap_or("").to_string();
            mentions.push(OrgMention {
                id: s("id"),
                label: s("name_original"),
                names: vec![s("name_original"), s("name_en")],
                country: iso2(&s("country")),
                website: s("url"),
                input: i,
                locator: format!("records[{k}]"),
                url: s("url"),
            });
        }
    }
    let p = cache.join("directories/ern_epicare.json");
    if p.exists() {
        let i = orgs.add_input(Input::from_file(
            "ERN EpiCARE directory",
            &p,
            "https://epi-care.eu/",
            "2026-10-03",
            "see directories header",
        )?);
        for (k, r) in recs(&read(&p)?).iter().enumerate() {
            let s = |f: &str| r[f].as_str().unwrap_or("").to_string();
            let name = s("name");
            // Centre names are often "Department X, Hospital Y": try the whole name and each part.
            let mut names = vec![name.clone()];
            names.extend(name.split(", ").map(String::from));
            mentions.push(OrgMention {
                id: s("id"),
                label: name,
                names,
                country: iso2(&s("country")),
                website: s("website"),
                input: i,
                locator: format!("records[{k}]"),
                url: s("profile_url"),
            });
        }
    }
    let p = cache.join("research_intl/cordis.json");
    if p.exists() {
        let i = orgs.add_input(Input::from_file(
            "CORDIS projects (organisations)",
            &p,
            "https://cordis.europa.eu/",
            "2026-10-03",
            "CC-BY-4.0 (EU Open Data)",
        )?);
        let mut seen = HashSet::new();
        for (k, r) in recs(&read(&p)?).iter().enumerate() {
            for (j, o) in r["organisations"].as_array().into_iter().flatten().enumerate() {
                let pic = o["pic"].as_str().unwrap_or("");
                if pic.is_empty() || !seen.insert(pic.to_string()) {
                    continue;
                }
                let s = |f: &str| o[f].as_str().unwrap_or("").to_string();
                mentions.push(OrgMention {
                    id: format!("cordis.org:{pic}"),
                    label: s("name"),
                    names: vec![s("name")],
                    country: iso2(&s("country")),
                    website: s("website"),
                    input: i,
                    locator: format!("records[{k}].organisations[{j}]"),
                    url: r["url"].as_str().unwrap_or("").into(),
                });
            }
        }
    }
    let p = cache.join("research_intl/gtr.json");
    if p.exists() {
        let i = orgs.add_input(Input::from_file(
            "UKRI GtR projects (organisations)",
            &p,
            "https://gtr.ukri.org/",
            "2026-10-03",
            "OGL-UK-3.0",
        )?);
        let mut seen = HashSet::new();
        for (k, r) in recs(&read(&p)?).iter().enumerate() {
            for (j, o) in r["organisations"].as_array().into_iter().flatten().enumerate() {
                let id = o["gtr_org_id"].as_str().unwrap_or("");
                if id.is_empty() || !seen.insert(id.to_string()) {
                    continue;
                }
                let name = o["name"].as_str().unwrap_or("").to_string();
                mentions.push(OrgMention {
                    id: format!("gtr.org:{id}"),
                    label: name.clone(),
                    names: vec![name],
                    country: iso2(o["country"].as_str().unwrap_or("")),
                    website: String::new(),
                    input: i,
                    locator: format!("records[{k}].organisations[{j}]"),
                    url: o["api_url"].as_str().unwrap_or("").into(),
                });
            }
        }
    }
    for m in &mentions {
        match_org(&mut orgs, &ror, m);
    }
    check_cardinality(&mut orgs);
    orgs.parameters = json!({"exact": "none: this producer has no explicit source equivalence between a mention and ROR", "composite_candidates": "normalised name equals a ROR display/label/alias, country among all ROR locations, exact website host, exactly one ROR record (semapv:CompositeMatching; confidence is an uncalibrated rule tier)",
        "candidates": "name+country (0.8), name only (0.6), website host only (0.6)", "normalisation": "lower case, simple accent folding, punctuation removed, leading 'the' dropped; names under 4 characters ignored",
        "ern_centres": "full name and each comma-separated part tried (department vs hospital)"});
    orgs.extra.insert("mentions".into(), json!(mentions.len()));

    // ---- funder-ror
    let mut funders = MappingSet {
        id: "funder-ror".into(),
        description: "Funders: ROR = Crossref Funder Registry id (ROR-asserted), plus funder names in our grant caches as candidates".into(),
        license: "https://creativecommons.org/publicdomain/zero/1.0/".into(),
        curie_map: curies(),
        ..Default::default()
    };
    let i_ror = funders.add_input(ror_input);
    for (k, r) in ror.recs.iter().enumerate() {
        for f in &r.fundref {
            let mut row = Row::xref(
                &r.id,
                &format!("crossref.funder:{f}"),
                i_ror,
                format!("ROR row {} ({})", k + 2, r.id),
            );
            row.subject_label = r.display.clone();
            row.subject_source = "infores:ror".into();
            row.object_source = "infores:crossref-funder-registry".into();
            row.evidence_url = format!("https://ror.org/{}", r.id.trim_start_matches("ROR:"));
            row.comment = "ROR external_ids FundRef".into();
            // Precision sample: non-preferred FundRef ids of a ROR record can be faculties or funds the
            // organisation administers (e.g. a foundation under a university), so only the preferred id
            // is an identity; an unspecified preferred id is unknown, including singleton lists.
            let preferred = !r.fundref_preferred.is_empty() && *f == r.fundref_preferred;
            if !preferred {
                row.predicate = CLOSE.into();
                row.asserted = CLOSE.into();
                row.confidence = 0.8;
                row.comment = "ROR external_ids FundRef (not the preferred id: may be a sub-unit or administered fund): candidate".into();
            }
            if !r.active {
                row.demote("retired");
            }
            funders.rows.push(row);
        }
    }
    // Our funder ids: which resolve to ROR through these rows (coverage, not new mappings).
    let mut used_fundrefs: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let p = cache.join("research_intl/europepmc_grants.json");
    if p.exists() {
        for r in recs(&read(&p)?) {
            if let Some(f) = r["funder"]["fundref"].as_str() {
                let id = f.trim_start_matches("https://doi.org/10.13039/").to_string();
                let hit = ror.by_fundref.contains_key(&id);
                used_fundrefs.insert(id, (r["funder"]["name"].as_str().unwrap_or("").into(), hit));
            }
        }
    }
    // NIH institutes/centres (RePORTER agency) → ROR by full name in the US: candidates only.
    let mut ics: BTreeMap<String, (String, String, usize)> = BTreeMap::new();
    let dir = cache.join("reporter");
    if dir.exists() {
        let mut files: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for p in files {
            let i = funders.add_input(Input::from_file(
                "NIH RePORTER grants",
                &p,
                "https://api.reporter.nih.gov/",
                "2026-10-03",
                "public-domain",
            )?);
            for (k, r) in recs(&read(&p)?).iter().enumerate() {
                if let (Some(code), Some(name)) = (r["agency"]["code"].as_str(), r["agency"]["name"].as_str())
                {
                    ics.entry(code.into()).or_insert((
                        name.into(),
                        format!("{} records[{k}].agency", p.file_name().unwrap().to_string_lossy()),
                        i,
                    ));
                }
            }
        }
    }
    for (code, (name, loc, source_input)) in &ics {
        let m = OrgMention {
            id: format!("nih.ic:{code}"),
            label: name.clone(),
            names: vec![name.clone()],
            country: "US".into(),
            website: String::new(),
            input: *source_input,
            locator: loc.clone(),
            url: "https://reporter.nih.gov/".into(),
        };
        match_org(&mut funders, &ror, &m);
    }
    check_cardinality(&mut funders);
    let cluster = check_clusters(&mut [&mut funders], &[]);
    funders
        .extra
        .insert("cluster_conflict_rows".into(), json!(cluster));
    funders.parameters = json!({"exact": "ROR external_ids explicitly preferred FundRef id, 1:1 per prefix pair; missing preferred status and all other ids remain candidates", "nih_ic": "RePORTER agency name + US against ROR names: candidates only"});
    funders
        .extra
        .insert("our_fundref_ids_resolved_by_ror".into(), json!(used_fundrefs));
    Ok(vec![orgs, funders])
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    fn ror(id: &str, domains: &[&str]) -> Ror {
        Ror {
            id: id.into(),
            display: "Synthetic Research Institute".into(),
            country: "US".into(),
            countries: vec!["US".into(), "DE".into()],
            hosts: domains.iter().map(|d| (*d).into()).collect(),
            fundref: vec![],
            fundref_preferred: String::new(),
            active: true,
        }
    }
    fn mention(web: &str) -> OrgMention {
        OrgMention {
            id: "atlasorg:synthetic".into(),
            label: "Synthetic Research Institute".into(),
            names: vec!["Synthetic Research Institute".into()],
            country: "DE".into(),
            website: web.into(),
            input: 0,
            locator: "synthetic-record".into(),
            url: "https://example.invalid/synthetic".into(),
        }
    }
    #[test]
    fn second_country_counts_but_shared_parent_host_does_not_establish_identity() {
        let idx = RorIndex {
            recs: vec![ror("ROR:synthetic", &["example.invalid"])],
            by_name: HashMap::from([(norm_name("Synthetic Research Institute"), vec![0])]),
            by_fundref: HashMap::new(),
        };
        let mut composite = MappingSet::default();
        match_org(&mut composite, &idx, &mention("https://example.invalid"));
        assert_eq!(composite.rows[0].predicate, CLOSE);
        assert_eq!(composite.rows[0].justification, "semapv:CompositeMatching");
        let mut child = MappingSet::default();
        match_org(&mut child, &idx, &mention("https://department.example.invalid"));
        assert_eq!(child.rows[0].predicate, CLOSE);
    }
    #[test]
    fn ambiguous_name_preserves_every_candidate() {
        let idx = RorIndex {
            recs: vec![ror("ROR:synthetic1", &[]), ror("ROR:synthetic2", &[])],
            by_name: HashMap::from([(norm_name("Synthetic Research Institute"), vec![0, 1])]),
            by_fundref: HashMap::new(),
        };
        let mut set = MappingSet::default();
        match_org(&mut set, &idx, &mention(""));
        assert_eq!(set.rows.len(), 2);
        assert!(set.rows.iter().all(|r| r.predicate == CLOSE));
        assert!(set.exclusion_ledger.is_empty());
    }
}
