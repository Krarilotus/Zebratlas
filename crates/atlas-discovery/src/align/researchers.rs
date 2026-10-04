//! Source-asserted ORCID identity. Author occurrences are explicit alias spaces, not person profiles.
//! Derived name-based overlap caches are never evidence for exact identity. D36: never bulk release.
use super::*;
use anyhow::Result;
use atlas_core::provenance::Locator;
use atlas_core::withhold::{KeyKind, QUARANTINE_FILE, SALT_ENV, SUPPRESSION_FILE, Salt, Withhold};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

/// ISO 7064 MOD 11-2 checksum; malformed ORCIDs must never become exact keys.
pub fn orcid(value: &str) -> Option<String> {
    let s = atlas_core::withhold::normalise_orcid(value)?;
    let digits: Vec<u8> = s.bytes().filter(|b| *b != b'-').collect();
    let total = digits[..15].iter().fold(0u32, |a, b| (a + u32::from(b - b'0')) * 2);
    let check = (12 - total % 11) % 11;
    let want = if check == 10 { b'X' } else { b'0' + check as u8 };
    (digits[15] == want).then_some(s)
}

/// Discovery has only a core dependency: IO here, all parsing/matching in the shared core filter.
pub fn withholding(data: &Path) -> Result<Withhold> {
    fn read(p: &Path) -> Result<Option<Vec<u8>>> {
        match std::fs::read(p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    let s = read(&data.join(SUPPRESSION_FILE))?;
    let q = read(&data.join(QUARANTINE_FILE))?;
    let salt = Salt::from_config(std::env::var(SALT_ENV).ok().as_deref());
    Ok(Withhold::from_lists(salt, s.as_deref(), q.as_deref())?)
}

#[derive(Clone)]
struct Mention {
    id: String,
    name: String,
    affiliations: Vec<String>,
    rors: BTreeSet<String>,
    topics: BTreeSet<String>,
    orcid: Option<String>,
    input: usize,
    locator: String,
    url: String,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

fn withheld(w: &Withhold, m: &Mention) -> bool {
    let mut keys = Vec::new();
    keys.extend(w.salt().key(KeyKind::Node, &m.id));
    if let Some(o) = &m.orcid {
        keys.extend(w.salt().key(KeyKind::Orcid, o));
        keys.extend(w.salt().key(KeyKind::Node, &format!("ORCID:{o}")));
    }
    w.keys(&keys).is_some()
        || m.affiliations
            .iter()
            .chain(std::iter::once(&String::new()))
            .any(|a| w.contact(&m.name, a, None).is_some())
}

pub fn build(data: &Path) -> Result<Vec<MappingSet>> {
    // Validate before writing anything; closed lists are a build error, never an empty list.
    let w = withholding(data)?;
    let mut set = MappingSet {
        id: "researcher-orcid".into(),
        description: "Private researcher identity assertions; ORCID only for exact identity".into(),
        license: "https://creativecommons.org/licenses/by/4.0/".into(),
        curie_map: base_curies(),
        ..Default::default()
    };
    set.extra.insert("release".into(), json!(false));
    for (p, u) in [
        ("ORCID", "https://orcid.org/"),
        ("pubmed.author", "https://w3id.org/rare-atlas/pubmed-author/"),
        ("epmc.author", "https://w3id.org/rare-atlas/epmc-author/"),
        ("REPORTER.PI", "https://reporter.nih.gov/search/"),
        ("openalex", "https://openalex.org/"),
        ("cordis.person", "https://w3id.org/rare-atlas/cordis-person/"),
    ] {
        set.curie_map.insert(p.into(), u.into());
    }
    for file in [SUPPRESSION_FILE, QUARANTINE_FILE] {
        let p = data.join(file);
        if p.exists() {
            set.add_input(Input::from_file(
                "withholding policy",
                &p,
                "local:withholding-policy",
                "1",
                "private",
            )?);
        }
    }
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();
    let mut suppressed_orcids = HashSet::new();
    let mut suppressed_subjects = HashSet::new();
    let ror_syntax = regex::Regex::new(r"^0[a-z0-9]{6}[0-9]{2}$")?;
    for (folder, source, url) in [
        (
            "pubmed",
            "pubmed.author",
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/",
        ),
        (
            "research_intl",
            "epmc.author",
            "https://www.ebi.ac.uk/europepmc/webservices/rest/",
        ),
        ("reporter", "reporter.pi", "https://api.reporter.nih.gov/"),
        ("align/researchers", "openalex", "https://api.openalex.org/"),
    ] {
        let dir = data.join("cache").join(folder);
        if !dir.exists() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for p in files {
            if folder == "research_intl" && !p.file_name().unwrap().to_string_lossy().starts_with("europepmc_") {
                continue;
            }
            let v: Value = serde_json::from_slice(&std::fs::read(&p)?)?;
            let mut input = Input::from_file(
                "source researcher assertions",
                &p,
                url,
                "cache-v1",
                "public identifiers",
            )?;
            input.retrieved_at = v["header"]["retrieved_at"].as_str().map(String::from);
            input.version = format!("{} v{}", v["schema"].as_str().unwrap_or("source-cache"), v["version"]);
            let i = set.add_input(input);
            for (k, r) in v["records"].as_array().into_iter().flatten().enumerate() {
                if w.record(
                    &p.strip_prefix(data)?.to_string_lossy(),
                    &Locator::Record(format!("records[{k}]")),
                )
                .is_some()
                {
                    set.exclude("quarantined source record", "withheld");
                    continue;
                }
                let people = if source == "reporter.pi" {
                    &r["pis"]
                } else if source == "openalex" {
                    &r["authorships"]
                } else {
                    &r["authors"]
                };
                for (j, a) in people.as_array().into_iter().flatten().enumerate() {
                    let authorship = a;
                    let a = if source == "openalex" { &a["author"] } else { a };
                    let raw_orcid = a["orcid"].as_str().unwrap_or("");
                    let o = orcid(raw_orcid);
                    if !raw_orcid.is_empty() && o.is_none() {
                        set.exclude("invalid ORCID checksum/syntax", "invalid identifier");
                    }
                    let record = r["id"].as_str().unwrap_or("");
                    let id = if source == "reporter.pi" {
                        let profile = a["profile_id"]
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| a["profile_id"].to_string());
                        if profile.is_empty() || !profile.bytes().all(|b| b.is_ascii_digit()) {
                            set.exclude("PI lacks stable profile id", "missing identifier");
                            continue;
                        }
                        format!("REPORTER.PI:{profile}")
                    } else if source == "openalex" {
                        let local = a["id"]
                            .as_str()
                            .unwrap_or("")
                            .trim_start_matches("https://openalex.org/");
                        if !local.starts_with('A') || local.len() < 2 || !local[1..].bytes().all(|b| b.is_ascii_digit())
                        {
                            set.exclude("invalid OpenAlex author id", "invalid identifier");
                            continue;
                        }
                        format!(
                            "openalex:{}",
                            a["id"]
                                .as_str()
                                .unwrap_or("")
                                .trim_start_matches("https://openalex.org/")
                        )
                    } else {
                        format!("{source}:{}-{j}", record.replace(':', "_"))
                    };
                    if record.is_empty() {
                        set.exclude("author lacks work identifier", "missing identifier");
                        continue;
                    }
                    let name = a["full_name"]
                        .as_str()
                        .or_else(|| a["display_name"].as_str())
                        .map(String::from)
                        .unwrap_or_else(|| {
                            format!(
                                "{} {}",
                                a["fore_name"]
                                    .as_str()
                                    .or_else(|| a["first_name"].as_str())
                                    .unwrap_or(""),
                                a["last_name"].as_str().unwrap_or("")
                            )
                        });
                    let mut topics: BTreeSet<String> = strings(&r["genes"]).into_iter().collect();
                    if let Some(g) = r["gene"].as_str() {
                        topics.insert(g.into());
                    }
                    let mut affiliations = strings(&a["affiliations"]);
                    let mut rors = strings(&a["ror_ids"]);
                    if source == "reporter.pi"
                        && let Some(aff) = r["organization"]["name"].as_str()
                    {
                        affiliations.push(aff.into());
                    }
                    if source == "openalex" {
                        affiliations.extend(strings(&authorship["raw_affiliation_strings"]));
                        for institution in authorship["institutions"].as_array().into_iter().flatten() {
                            if let Some(n) = institution["display_name"].as_str() {
                                affiliations.push(n.into());
                            }
                            if let Some(id) = institution["ror"].as_str() {
                                rors.push(id.into());
                            }
                        }
                    }
                    let rors = rors
                        .iter()
                        .filter_map(|r| {
                            let local = r.trim_start_matches("https://ror.org/").trim_start_matches("ROR:");
                            ror_syntax.is_match(local).then(|| format!("ROR:{local}"))
                        })
                        .collect();
                    let m = Mention {
                        id,
                        name,
                        affiliations,
                        rors,
                        topics,
                        orcid: o,
                        input: i,
                        locator: format!(
                            "records[{k}].{}[{j}]{}.orcid",
                            if source == "reporter.pi" {
                                "pis"
                            } else if source == "openalex" {
                                "authorships"
                            } else {
                                "authors"
                            },
                            if source == "openalex" { ".author" } else { "" }
                        ),
                        url: r["url"].as_str().unwrap_or(url).into(),
                    };
                    if withheld(&w, &m) {
                        if let Some(o) = &m.orcid {
                            suppressed_orcids.insert(o.clone());
                        }
                        suppressed_subjects.insert(m.id.clone());
                        set.exclude("suppressed researcher mention", "withheld");
                        continue;
                    }
                    // Repeated work in gene caches is the same mention; contradictory ORCIDs remain rows.
                    if let Some(o) = &m.orcid {
                        if seen.insert((m.id.clone(), o.clone())) {
                            let mut row = Row::xref(&m.id, &format!("ORCID:{o}"), i, m.locator.clone());
                            row.evidence_url = m.url.clone();
                            row.subject_source = format!("infores:{}", source.split('.').next().unwrap());
                            row.object_source = "infores:orcid".into();
                            row.comment =
                                "Source asserts ORCID on this author/PI; labels intentionally omitted (D36)".into();
                            set.rows.push(row);
                        }
                    } else {
                        set.exclude("no source-asserted ORCID (never exact)", "unidentified mention");
                    }
                    mentions.push(m);
                }
            }
        }
    }
    // A name/affiliation suppression on a source-asserted identity applies to its other occurrences.
    // Never restore the same ORCID through another publication or a spelling/affiliation variant.
    set.rows.retain(|r| {
        !suppressed_subjects.contains(&r.subject_id)
            && !suppressed_orcids.contains(r.object_id.strip_prefix("ORCID:").unwrap_or(""))
    });
    let mut propagated = 0;
    mentions.retain(|m| {
        let keep =
            !suppressed_subjects.contains(&m.id) && !m.orcid.as_ref().is_some_and(|o| suppressed_orcids.contains(o));
        if !keep {
            propagated += 1;
        }
        keep
    });
    for _ in 0..propagated {
        set.exclude("suppression propagated across asserted identity", "withheld");
    }
    // Candidates need all three independent guards, never just names or a shared paper.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in mentions.iter().enumerate() {
        if !m.rors.is_empty() && !m.topics.is_empty() && !org::norm_name(&m.name).is_empty() {
            by_name.entry(org::norm_name(&m.name)).or_default().push(i);
        }
    }
    let mut candidate_seen = HashSet::new();
    for indices in by_name.values() {
        for (pos, &a) in indices.iter().enumerate() {
            for &b in &indices[pos + 1..] {
                let (a, b) = (&mentions[a], &mentions[b]);
                if a.id == b.id
                    || a.orcid.is_some() && b.orcid.is_some()
                    || a.rors.is_disjoint(&b.rors)
                    || a.topics.is_disjoint(&b.topics)
                {
                    continue;
                }
                if !candidate_seen.insert((a.id.clone(), b.id.clone())) {
                    continue;
                }
                let mut row = Row::link(&a.id, CLOSE, &b.id, a.input, a.locator.clone());
                row.justification = "semapv:CompositeMatching".into();
                row.confidence = 0.6;
                row.evidence_url = a.url.clone();
                row.comment = format!(
                    "Name + source-asserted ROR affiliation + topic overlap; candidate only; other evidence {} ({})",
                    b.url, b.locator
                );
                set.rows.push(row);
            }
        }
    }
    check_cardinality(&mut set);
    for p in ["pubmed.author", "epmc.author"] {
        accept_cardinality(
            &mut set,
            p,
            "many_to_one",
            "author occurrences are an explicit alias space, not distinct profiles",
        );
    }
    check_clusters(&mut [&mut set], &["pubmed.author", "epmc.author"]);
    set.parameters = json!({"exact": "source-asserted, checksum-valid ORCID", "candidate": "same normalised name + source-asserted ROR + topic overlap", "alias_prefixes": ["pubmed.author", "epmc.author"], "suppression": "atlas_core::withhold; strict, fail-closed", "salt_id": w.salt().id()});
    set.notes.push("D36: release false includes ORCID and author occurrences; reports contain counts only. Source caches are not modified. Derived people.overlap identities are not accepted as assertions.".into());
    set.notes.push("OpenAlex acquisition remains blocked (recorded HTTP 429); no retries or alternate routes. CORDIS/RePORTER without ORCID stay unidentified.".into());
    Ok(vec![set])
}
