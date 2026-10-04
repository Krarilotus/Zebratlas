//! ClinicalTrials.gov studies (`ctgov.studies` v1) → study nodes linked to rare conditions and genes.
//!
//! Port of the linking rules of `rare_atlas/trials.py` (SOURCES.md): exact (normalised condition
//! equals a name or synonym of ≥ 5 characters naming ≤ 2 diseases), mesh (condition MeSH id equals a
//! MONDO `equivalentTo` MeSH xref), gene (HGNC symbol as a case-sensitive token in a condition, the
//! title or a keyword; ≤ 3-letter symbols without a digit ignored). Umbrella links (MONDO parent or
//! grandparent) are stored as ancestor lists and resolved per query.
//! Difference to the Python: only *rare* active conditions are link targets (the ambiguity guard
//! still counts every active disease), and only linked studies become nodes.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use atlas_core::Term;
use atlas_core::graph::{
    Coverage, LinkLevel, OrgKind, Organisation, RecordHash, Relation, SourceRecord, Study, StudyKind, activity,
};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{Locator, SourceEntity};
use serde::Deserialize;
use sha2::Digest;

use super::builder::{Builder, NewEdge, normalize_name};
use super::cache;
use crate::error::IngestError;

pub const STUDIES: &str = "cache/trials/studies.jsonl.gz";
pub const HEADER: &str = "cache/trials/studies.header.json";
const MIN_NAME: usize = 5;
const MAX_NAME_TARGETS: usize = 2;
const UMBRELLA_DEPTH: usize = 2;
/// Records kept on a sponsor node (provenance sample; every sponsorship edge has its own record).
const ORG_RECORDS: usize = 3;

#[derive(Deserialize)]
struct Mesh {
    id: String,
    #[serde(default)]
    term: String,
}

#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    fetched_at: Option<String>,
    nct_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    start: String,
    #[serde(default)]
    completion: String,
    #[serde(default)]
    phases: Vec<String>,
    #[serde(default, rename = "type")]
    study_type: String,
    #[serde(default)]
    patient_registry: bool,
    #[serde(default)]
    enrollment: Option<u64>,
    #[serde(default)]
    sponsor: String,
    #[serde(default)]
    sponsor_class: String,
    #[serde(default)]
    conditions: Vec<String>,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    mesh: Vec<Mesh>,
    #[serde(default)]
    interventions: Vec<String>,
    #[serde(default)]
    countries: Vec<String>,
}

fn kind(l: &Line) -> StudyKind {
    match l.study_type.as_str() {
        "INTERVENTIONAL" => return StudyKind::Trial,
        "EXPANDED_ACCESS" => return StudyKind::ExpandedAccess,
        _ => {}
    }
    let text = std::iter::once(&l.title)
        .chain(&l.keywords)
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if l.patient_registry || text.contains("registry") {
        StudyKind::Registry
    } else if text.contains("natural history") {
        StudyKind::NaturalHistory
    } else {
        StudyKind::Observational
    }
}

/// Lookup tables from the atlas: names, MeSH xrefs and gene symbols → link targets.
pub struct Dicts {
    /// Normalised name → rare active conditions (names naming > 2 active diseases are dropped).
    pub by_name: HashMap<String, Vec<String>>,
    by_mesh: HashMap<String, Vec<String>>,
    /// Gene symbol → gene id.
    symbols: HashMap<String, String>,
}

impl Dicts {
    pub fn new(b: &Builder<'_>, mondo: &HashMap<&str, &Term>) -> Self {
        let mut names: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        let mut by_mesh: HashMap<String, Vec<String>> = HashMap::new();
        for (_, d) in b.atlas.active() {
            let all = std::iter::once(d.name.as_str()).chain(d.synonyms.iter().map(|n| n.text.as_str()));
            for name in all {
                let key = normalize_name(name);
                if key.len() >= MIN_NAME {
                    let list = names.entry(key).or_default();
                    if !list.iter().any(|(id, _)| *id == d.id) {
                        list.push((d.id.clone(), d.rare));
                    }
                }
            }
            if !d.rare {
                continue;
            }
            if let Some(term) = mondo.get(d.id.as_str()) {
                for x in &term.xrefs {
                    if x.id.starts_with("MESH:") && x.sources.iter().any(|s| s == "MONDO:equivalentTo") {
                        by_mesh.entry(x.id.clone()).or_default().push(d.id.clone());
                    }
                }
            }
        }
        let by_name = names
            .into_iter()
            .filter(|(_, ids)| ids.len() <= MAX_NAME_TARGETS)
            .map(|(k, ids)| {
                (
                    k,
                    ids.into_iter()
                        .filter(|(_, rare)| *rare)
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>(),
                )
            })
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        let symbols = b
            .atlas
            .genes()
            .iter()
            .map(|g| (g.symbol.clone(), g.id().to_owned()))
            .collect();
        Self {
            by_name,
            by_mesh,
            symbols,
        }
    }

    /// Symbols named in the study, first place wins: `(symbol, where)`.
    fn genes(&self, l: &Line) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = Vec::new();
        let places = [
            ("condition", l.conditions.as_slice()),
            ("title", std::slice::from_ref(&l.title)),
            ("keyword", l.keywords.as_slice()),
        ];
        for (place, texts) in places {
            for text in texts {
                for tok in tokens(text) {
                    let ok = tok.len() >= 4 || tok.bytes().any(|c| c.is_ascii_digit());
                    if ok && self.symbols.contains_key(tok) && !found.iter().any(|(s, _)| s == tok) {
                        found.push((tok.to_owned(), format!("{place} \"{text}\"")));
                    }
                }
            }
        }
        found
    }
}

/// Python `re.findall(r"[A-Za-z0-9][A-Za-z0-9-]*[A-Za-z0-9]", text)`.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .map(|run| run.trim_matches('-'))
        .filter(|t| t.len() >= 2)
}

pub fn ingest(
    b: &mut Builder<'_>,
    data: &Path,
    dicts: &Dicts,
    mondo: &HashMap<&str, &Term>,
) -> Result<(), IngestError> {
    let path = data.join(STUDIES);
    let header_path = data.join(HEADER);
    if !path.exists() || !header_path.exists() {
        b.data.coverage.push(Coverage {
            source: "ctgov".into(),
            label: "ClinicalTrials.gov".into(),
            status: "absent".into(),
            files: vec![STUDIES.into()],
            ..Coverage::default()
        });
        return Ok(());
    }
    let header_text = std::fs::read(&header_path).map_err(IngestError::io(&header_path))?;
    let header: serde_json::Value = serde_json::from_slice(&header_text).map_err(|e| IngestError::Json {
        path: header_path.clone(),
        source: e,
    })?;
    let (schema, version) = (header["schema"].as_str().unwrap_or(""), header["version"].as_u64());
    if schema != "ctgov.studies" || version != Some(1) {
        return Err(IngestError::Schema {
            path: header_path,
            found: format!("{schema} v{version:?}"),
            expected: "ctgov.studies v1".into(),
        });
    }
    let h = &header["header"];
    let retrieved_at = h["retrieved_at"].as_str().map(str::to_owned);
    let expected = h["sha256"].as_str().unwrap_or("").to_owned();
    let entity = b.entity(SourceEntity {
        id: format!("source:{STUDIES}"),
        url: h["query"]["api"]
            .as_str()
            .unwrap_or("https://clinicaltrials.gov/api/v2/studies")
            .into(),
        file: STUDIES.into(),
        version: Some("ctgov.studies v1".into()),
        retrieved_at: retrieved_at.clone(),
        sha256: Some(expected.clone()),
        bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        licence: Some("ClinicalTrials.gov terms of use (public data, cite the source)".into()),
    });
    let act = b.start(
        activity::INGEST_CTGOV,
        "Read every ClinicalTrials.gov study; keep studies that link to a rare condition or a gene",
        &[entity],
    );
    let link = b.start(
        activity::LINK_CTGOV,
        "Link studies to conditions (exact name, MeSH) and genes (symbol token), as trials.py",
        &[entity],
    );
    b.param(link, "min_name_chars", MIN_NAME.to_string());
    b.param(link, "max_name_targets", MAX_NAME_TARGETS.to_string());
    b.param(
        link,
        "targets",
        "active rare conditions; ambiguity counted over all active diseases",
    );

    let mut hasher = sha2::Sha256::new();
    let (mut read, mut kept, mut bad) = (0usize, 0usize, 0usize);
    for line in cache::gz_lines(&path)? {
        let (n, bytes) = line.map_err(IngestError::io(&path))?;
        hasher.update(&bytes);
        hasher.update(b"\n");
        if bytes.is_empty() {
            continue;
        }
        read += 1;
        let Ok(l) = serde_json::from_slice::<Line>(&bytes) else {
            bad += 1;
            continue;
        };
        let mut exact: Vec<(&str, &str)> = Vec::new();
        for c in &l.conditions {
            for id in dicts.by_name.get(&normalize_name(c)).into_iter().flatten() {
                exact.push((id, c));
            }
        }
        let mut mesh: Vec<(&str, &Mesh)> = Vec::new();
        for m in &l.mesh {
            for id in dicts.by_mesh.get(&m.id).into_iter().flatten() {
                mesh.push((id, m));
            }
        }
        let genes = dicts.genes(&l);
        if exact.is_empty() && mesh.is_empty() && genes.is_empty() {
            continue;
        }
        kept += 1;
        let rec = b.record(SourceRecord {
            entity,
            locator: Locator::Line(n),
            id: l.nct_id.clone(),
            url: l.url.clone(),
            fetched_at: l.fetched_at.clone(),
            hash: RecordHash::JsonLine,
            sha256: cache::sha256(&bytes),
        });
        let nct = l.nct_id.clone();
        for (id, cond) in exact {
            let e = NewEdge {
                from: &nct,
                relation: Relation::StudiesCondition,
                to: id,
                kind: EdgeKind::Observed,
                level: LinkLevel::Exact,
                reason: format!("condition \"{cond}\""),
                activity: link,
            };
            b.edge(e, &[rec]);
        }
        for (id, m) in mesh {
            let e = NewEdge {
                from: &nct,
                relation: Relation::StudiesCondition,
                to: id,
                kind: EdgeKind::Inferred,
                level: LinkLevel::Mesh,
                reason: format!("MeSH {} {}", m.id, m.term),
                activity: link,
            };
            b.edge(e, &[rec]);
        }
        for (symbol, place) in genes {
            let to = dicts.symbols[&symbol].clone();
            let e = NewEdge {
                from: &nct,
                relation: Relation::NamesGene,
                to: &to,
                kind: EdgeKind::Inferred,
                level: LinkLevel::Gene,
                reason: format!("{symbol} named in {place}"),
                activity: link,
            };
            b.edge(e, &[rec]);
        }
        if !l.sponsor.trim().is_empty() {
            let org = Builder::org_id(&l.sponsor);
            sponsor(b, &org, &l, rec);
            let e = NewEdge {
                from: &nct,
                relation: Relation::SponsoredBy,
                to: &org,
                kind: EdgeKind::Observed,
                level: LinkLevel::Curated,
                reason: format!("lead sponsor ({})", l.sponsor_class),
                activity: act,
            };
            b.edge(e, &[rec]);
        }
        let kind = kind(&l);
        b.register(&nct, NodeKind::Study, b.data.studies.len());
        b.data.studies.push(Study {
            id: nct,
            title: l.title,
            status: l.status,
            kind,
            phases: l.phases,
            sponsor: l.sponsor,
            sponsor_class: l.sponsor_class,
            start: l.start,
            completion: l.completion,
            enrollment: l.enrollment,
            countries: l.countries,
            interventions: l.interventions,
            record: rec,
        });
    }
    let verified = cache::hex(&hasher.finalize().into()) == expected;
    b.finish(
        act,
        &[("read", read), ("kept:linked", kept), ("skipped:unparsable", bad)],
    );
    b.count(
        act,
        if verified {
            "verified:header-sha256"
        } else {
            "mismatch:header-sha256"
        },
        1,
    );
    let links = b.data.edges.len();
    b.finish(link, &[("edges", links)]);
    umbrella(b, mondo);
    b.data.coverage.push(Coverage {
        source: "ctgov".into(),
        label: "ClinicalTrials.gov".into(),
        status: "loaded".into(),
        files: vec![STUDIES.into(), HEADER.into()],
        retrieved_at,
        scope: format!("all {read} registered studies, linked to rare conditions by name, MeSH and gene symbol"),
        genes: Vec::new(),
        records: kept as u64,
        header_checksums_verified: u64::from(verified),
        header_checksums_failed: u64::from(!verified),
        ..Coverage::default()
    });
    Ok(())
}

fn sponsor(b: &mut Builder<'_>, org: &str, l: &Line, rec: u32) {
    match b.node(org) {
        Some((_, i)) => {
            let o = &mut b.data.orgs[i as usize];
            if o.records.len() < ORG_RECORDS {
                o.records.push(rec);
            }
        }
        None => {
            b.register(org, NodeKind::Organisation, b.data.orgs.len());
            b.data.orgs.push(Organisation {
                id: org.to_owned(),
                name: l.sponsor.clone(),
                kind: OrgKind::Sponsor,
                url: None,
                contact_url: None,
                country: None,
                country_basis: None,
                description: Some(format!("Study sponsor ({})", l.sponsor_class)),
                languages: Vec::new(),
                verified_on: None,
                channels: Vec::new(),
                records: vec![rec],
            });
        }
    }
}

/// For every rare condition with MONDO parents: ancestors within depth 2 that have direct study links.
fn umbrella(b: &mut Builder<'_>, mondo: &HashMap<&str, &Term>) {
    let act = b.start(
        activity::UMBRELLA,
        "MONDO parents and grandparents with direct study links (umbrella level)",
        &[],
    );
    let linked: HashSet<&str> = b
        .data
        .edges
        .iter()
        .filter(|e| e.relation == Relation::StudiesCondition)
        .map(|e| e.to.as_str())
        .collect();
    let mut out = Vec::new();
    for (_, d) in b.atlas.active().filter(|(_, d)| d.rare) {
        let mut found: Vec<(String, String)> = Vec::new();
        let mut frontier = vec![d.id.as_str()];
        let mut seen: HashSet<&str> = HashSet::from([d.id.as_str()]);
        for _ in 0..UMBRELLA_DEPTH {
            let mut next = Vec::new();
            for id in frontier {
                for p in mondo.get(id).map(|t| t.parents.as_slice()).unwrap_or(&[]) {
                    if let Some(t) = mondo.get(p.as_str())
                        && seen.insert(&t.id)
                    {
                        if linked.contains(t.id.as_str()) {
                            found.push((t.id.clone(), t.name.clone()));
                        }
                        next.push(t.id.as_str());
                    }
                }
            }
            frontier = next;
        }
        if !found.is_empty() {
            out.push((d.id.clone(), found));
        }
    }
    let n = out.len();
    b.data.umbrella = out;
    b.finish(act, &[("conditions_with_umbrella", n)]);
}
