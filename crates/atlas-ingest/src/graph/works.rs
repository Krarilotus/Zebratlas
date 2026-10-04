//! NIH RePORTER grants (`reporter.grants` v1) and PubMed articles (`pubmed.articles` v1), per gene:
//! grant/paper → gene, grant/paper → condition (a name of one of the gene's rare conditions occurs in
//! the title or abstract), grant → institution, PI → grant. ORCID-identified authors are collected for
//! the people step.

use std::collections::HashMap;
use std::path::Path;

use atlas_core::graph::{
    Coverage, Grant, LinkLevel, OrgKind, Organisation, Paper, Person, PersonSource, RecIdx, RecordHash, Relation,
    SourceRecord, activity,
};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{EntityIdx, Locator, SourceEntity};
use serde_json::Value;

use super::builder::{Builder, NewEdge, normalize_name, strip_contacts};
use super::cache::{self, Envelope};
use super::trials::Dicts;
use crate::error::IngestError;

/// A PubMed author with an ORCID, for the people step.
pub struct OrcidAuthor {
    pub orcid: String,
    pub occurrence_id: String,
    pub name: String,
    pub affiliations: Vec<String>,
    pub paper: String,
    pub gene: String,
    pub record: RecIdx,
}

/// Rare conditions of a gene with their linkable names (normalised).
pub struct ConditionNames {
    by_gene: HashMap<String, Vec<(String, Vec<String>)>>,
}

impl ConditionNames {
    pub fn new(b: &Builder<'_>, dicts: &Dicts) -> Self {
        let mut names: HashMap<&str, Vec<String>> = HashMap::new();
        for (name, ids) in &dicts.by_name {
            for id in ids {
                names.entry(id.as_str()).or_default().push(name.clone());
            }
        }
        let mut by_gene: HashMap<String, Vec<(String, Vec<String>)>> = HashMap::new();
        for g in b.atlas.genes() {
            let list = g
                .diseases
                .iter()
                .map(|&d| b.atlas.disease_at(d))
                .filter(|d| d.rare)
                .filter_map(|d| names.get(d.id.as_str()).map(|n| (d.id.clone(), n.clone())))
                .collect();
            by_gene.insert(g.symbol.clone(), list);
        }
        Self { by_gene }
    }

    /// Conditions of `gene` named in `text`: `(condition id, matched name)`.
    fn named_in<'a>(&'a self, gene: &str, text: &str) -> Vec<(&'a str, &'a str)> {
        let hay = format!(" {} ", normalize_name(text));
        let mut out = Vec::new();
        for (id, names) in self.by_gene.get(gene).into_iter().flatten() {
            if let Some(n) = names.iter().find(|n| hay.contains(&format!(" {n} "))) {
                out.push((id.as_str(), n.as_str()));
            }
        }
        out
    }
}

fn s<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
}

/// Register one envelope file as a `prov:Entity`.
fn entity(b: &mut Builder<'_>, data: &Path, path: &Path, env: &Envelope, licence: &str) -> EntityIdx {
    let file = path
        .strip_prefix(data)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    b.entity(SourceEntity {
        id: format!("source:{file}"),
        url: env
            .header
            .get("query")
            .and_then(|q| q.get("api"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        file,
        version: Some(format!("{} v{}", env.schema, env.version)),
        retrieved_at: env.header_str("retrieved_at").map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        licence: Some(licence.into()),
    })
}

fn record(b: &mut Builder<'_>, entity: EntityIdx, i: usize, r: &Value) -> RecIdx {
    b.record(SourceRecord {
        entity,
        locator: Locator::Record(format!("records[{i}]")),
        id: s(r, "id").to_owned(),
        url: r.get("url").and_then(Value::as_str).map(str::to_owned),
        fetched_at: r.get("fetched_at").and_then(Value::as_str).map(str::to_owned),
        hash: RecordHash::CanonicalJson,
        sha256: cache::canonical_sha256(r),
    })
}

/// Shared per-source bookkeeping for gene-scoped caches.
struct Scope {
    coverage: Coverage,
}

impl Scope {
    fn new(source: &str, label: &str) -> Self {
        Self {
            coverage: Coverage {
                source: source.into(),
                label: label.into(),
                status: "absent".into(),
                ..Coverage::default()
            },
        }
    }

    fn add(&mut self, data: &Path, path: &Path, env: &Envelope, gene: &str) {
        let c = &mut self.coverage;
        c.status = "loaded".into();
        c.files.push(
            path.strip_prefix(data)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
        );
        c.genes.push(gene.to_owned());
        c.records += env.records.len() as u64;
        if env.header_verified {
            c.header_checksums_verified += 1;
        } else {
            c.header_checksums_failed += 1;
        }
        let at = env.header_str("retrieved_at").map(str::to_owned);
        if at > c.retrieved_at {
            c.retrieved_at = at;
        }
    }
}

fn text_edges(
    b: &mut Builder<'_>,
    names: &ConditionNames,
    from: &str,
    gene: &str,
    fields: [(&str, &str); 2],
    act: atlas_core::provenance::ActivityIdx,
    rec: RecIdx,
) {
    for (place, text) in fields {
        for (id, name) in names.named_in(gene, text) {
            let e = NewEdge {
                from,
                relation: Relation::AboutCondition,
                to: id,
                kind: EdgeKind::Inferred,
                level: LinkLevel::Exact,
                reason: format!("{place} names \"{name}\""),
                activity: act,
            };
            b.edge(e, &[rec]);
        }
    }
}

pub fn reporter(b: &mut Builder<'_>, data: &Path, names: &ConditionNames) -> Result<(), IngestError> {
    let dir = data.join("cache/reporter");
    let mut scope = Scope::new("reporter", "NIH RePORTER");
    let act = b.start(
        activity::INGEST_REPORTER,
        "NIH RePORTER grants per gene: grant, PI, institution",
        &[],
    );
    let text = b.start(
        activity::LINK_TEXT,
        "Condition names in titles and abstracts (grants, papers)",
        &[],
    );
    let (mut read, mut no_gene) = (0usize, 0usize);
    let mut files = json_files(&dir);
    files.extend(json_files(&data.join("cache/bench-fixes/reporter")));
    for path in files {
        let env = cache::read_envelope(&path, "reporter.grants", &[1])?;
        let gene = s(&env.header["query"], "search_text").to_owned();
        let ent = entity(b, data, &path, &env, "NIH RePORTER (US government work, public domain)");
        b.data.provenance.activity_mut(act).used.push(ent);
        b.data.provenance.activity_mut(text).used.push(ent);
        scope.add(data, &path, &env, &gene);
        for (i, r) in env.records.iter().enumerate() {
            read += 1;
            let rec = record(b, ent, i, r);
            if !env.header_verified {
                super::quarantine::mark(b, rec, "RePORTER envelope checksum failed");
            }
            let id = format!("REPORTER:{}", s(r, "id"));
            grant_node(b, &id, r, rec);
            let symbol = s(r, "gene");
            match b.gene_id(symbol) {
                Some(g) => {
                    let matched: Vec<&str> = r["matched_in"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    let e = NewEdge {
                        from: &id,
                        relation: Relation::AboutGene,
                        to: &g,
                        kind: EdgeKind::Observed,
                        level: LinkLevel::Text,
                        reason: format!("{symbol} in {} (RePORTER text search)", matched.join(", ")),
                        activity: act,
                    };
                    b.edge(e, &[rec]);
                }
                None => no_gene += 1,
            }
            text_edges(
                b,
                names,
                &id,
                symbol,
                [("title", s(r, "title")), ("abstract", s(r, "abstract"))],
                text,
                rec,
            );
            let org_name = s(&r["organization"], "name");
            if !org_name.is_empty() {
                let org = Builder::org_id(org_name);
                institution(b, &org, &r["organization"], rec);
                let e = NewEdge {
                    from: &id,
                    relation: Relation::AwardedTo,
                    to: &org,
                    kind: EdgeKind::Observed,
                    level: LinkLevel::Curated,
                    reason: "grantee organisation".into(),
                    activity: act,
                };
                b.edge(e, &[rec]);
            }
            for pi in r["pis"].as_array().into_iter().flatten() {
                let Some(profile) = pi.get("profile_id").and_then(Value::as_u64) else {
                    continue;
                };
                let pid = format!("REPORTER.PI:{profile}");
                if b.node(&pid).is_none() {
                    b.register(&pid, NodeKind::Person, b.data.people.len());
                    b.data.people.push(Person {
                        id: pid.clone(),
                        name: s(pi, "full_name").to_owned(),
                        name_variants: Vec::new(),
                        orcids: Vec::new(),
                        affiliations: vec![format!("{}, {}", org_name, s(&r["organization"], "country"))],
                        source: PersonSource::ReporterPi,
                        genes: Vec::new(),
                        communities: Vec::new(),
                        cross_community: false,
                        matched_by: vec!["reporter profile_id".into()],
                        merge_basis: Vec::new(),
                        records: Vec::new(),
                    });
                }
                let (_, pi_idx) = b.node(&pid).expect("registered");
                let p = &mut b.data.people[pi_idx as usize];
                if !p.records.contains(&rec) {
                    p.records.push(rec);
                }
                if !symbol.is_empty() && !p.genes.iter().any(|g| g == symbol) {
                    p.genes.push(symbol.to_owned());
                }
                let contact = if pi.get("is_contact_pi").and_then(Value::as_bool) == Some(true) {
                    " (contact PI)"
                } else {
                    ""
                };
                let e = NewEdge {
                    from: &pid,
                    relation: Relation::PrincipalInvestigatorOf,
                    to: &id,
                    kind: EdgeKind::Observed,
                    level: LinkLevel::Curated,
                    reason: format!("principal investigator{contact}"),
                    activity: act,
                };
                b.edge(e, &[rec]);
            }
        }
    }
    b.finish(
        act,
        &[
            ("records", read),
            ("grants", b.data.grants.len()),
            ("skipped:gene-not-in-atlas", no_gene),
        ],
    );
    b.data.coverage.push(Coverage {
        scope: "grants whose title, abstract or terms name the gene (NIH RePORTER API v2)".into(),
        ..scope.coverage
    });
    Ok(())
}

fn grant_node(b: &mut Builder<'_>, id: &str, r: &Value, rec: RecIdx) {
    if let Some((_, i)) = b.node(id) {
        b.data.grants[i as usize].records.push(rec);
        return;
    }
    let years: Vec<u16> = r["fiscal_years"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|y| y.as_u64())
        .map(|y| y as u16)
        .collect();
    b.register(id, NodeKind::Grant, b.data.grants.len());
    b.data.grants.push(Grant {
        id: id.to_owned(),
        title: s(r, "title").to_owned(),
        activity_code: s(r, "activity_code").to_owned(),
        agency: s(&r["agency"], "code").to_owned(),
        organisation: s(&r["organization"], "name").to_owned(),
        country: s(&r["organization"], "country").to_owned(),
        fiscal_years: years,
        award_total: r.get("award_total").and_then(Value::as_u64),
        start: s(r, "start").to_owned(),
        end: s(r, "end").to_owned(),
        url: s(r, "url").to_owned(),
        records: vec![rec],
    });
}

fn institution(b: &mut Builder<'_>, id: &str, org: &Value, rec: RecIdx) {
    if let Some((_, i)) = b.node(id) {
        let o = &mut b.data.orgs[i as usize];
        if o.records.len() < 3 {
            o.records.push(rec);
        }
        return;
    }
    let place = [s(org, "city"), s(org, "state"), s(org, "country")]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    b.register(id, NodeKind::Organisation, b.data.orgs.len());
    b.data.orgs.push(Organisation {
        id: id.to_owned(),
        name: s(org, "name").to_owned(),
        kind: OrgKind::Institution,
        url: None,
        contact_url: None,
        country: Some(s(org, "country").to_owned()).filter(|c| !c.is_empty()),
        country_basis: Some("RePORTER organisation record".into()),
        description: Some(format!("Research institution ({place})")),
        languages: Vec::new(),
        verified_on: None,
        channels: Vec::new(),
        records: vec![rec],
    });
}

pub fn pubmed(b: &mut Builder<'_>, data: &Path, names: &ConditionNames) -> Result<Vec<OrcidAuthor>, IngestError> {
    let dir = data.join("cache/pubmed");
    let mut scope = Scope::new("pubmed", "PubMed");
    let act = b.start(
        activity::INGEST_PUBMED,
        "PubMed articles per gene ([tiab] search): paper → gene, authors",
        &[],
    );
    let text = b
        .data
        .provenance
        .activities
        .iter()
        .position(|a| a.id == activity::LINK_TEXT)
        .map(|i| atlas_core::provenance::ActivityIdx(i as u16))
        .unwrap_or_else(|| b.start(activity::LINK_TEXT, "Condition names in titles and abstracts", &[]));
    let mut authors = Vec::new();
    let mut read = 0usize;
    for path in json_files(&dir) {
        let env = cache::read_envelope(&path, "pubmed.articles", &[1])?;
        let gene = s(&env.header["query"], "term").trim_end_matches("[tiab]").to_owned();
        let ent = entity(
            b,
            data,
            &path,
            &env,
            "PubMed/MEDLINE (NLM terms; abstracts may be copyrighted)",
        );
        b.data.provenance.activity_mut(act).used.push(ent);
        b.data.provenance.activity_mut(text).used.push(ent);
        scope.add(data, &path, &env, &gene);
        for (i, r) in env.records.iter().enumerate() {
            read += 1;
            let rec = record(b, ent, i, r);
            let id = s(r, "id").to_owned();
            match b.node(&id) {
                Some((_, p)) => b.data.papers[p as usize].records.push(rec),
                None => {
                    let review = r["publication_types"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|t| t.as_str() == Some("Review"));
                    b.register(&id, NodeKind::Paper, b.data.papers.len());
                    b.data.papers.push(Paper {
                        id: id.clone(),
                        title: s(r, "title").to_owned(),
                        journal: s(r, "journal").to_owned(),
                        year: r.get("year").and_then(Value::as_u64).map(|y| y as u16),
                        doi: r.get("doi").and_then(Value::as_str).map(str::to_owned),
                        review,
                        records: vec![rec],
                    });
                    // a PubMed Central copy is a legal free-to-read version (D35); never circumvention
                    if let Some(pmc) = r.get("pmcid").and_then(Value::as_str).filter(|p| p.starts_with("PMC")) {
                        b.data.open_access.push(atlas_core::graph::OpenAccess {
                            paper: id.clone(),
                            url: format!("https://pmc.ncbi.nlm.nih.gov/articles/{pmc}/"),
                            status: "pmc".into(),
                            licence: None,
                            record: rec,
                        });
                    }
                }
            }
            let symbol = s(r, "gene");
            if let Some(g) = b.gene_id(symbol) {
                let e = NewEdge {
                    from: &id,
                    relation: Relation::AboutGene,
                    to: &g,
                    kind: EdgeKind::Observed,
                    level: LinkLevel::Text,
                    reason: format!("{symbol} in title/abstract (PubMed [tiab] search)"),
                    activity: act,
                };
                b.edge(e, &[rec]);
            }
            text_edges(
                b,
                names,
                &id,
                symbol,
                [("title", s(r, "title")), ("abstract", s(r, "abstract"))],
                text,
                rec,
            );
            for (author_index, a) in r["authors"].as_array().into_iter().flatten().enumerate() {
                let Some(orcid) = a.get("orcid").and_then(Value::as_str).filter(|o| !o.is_empty()) else {
                    continue;
                };
                let name = format!("{} {}", s(a, "fore_name"), s(a, "last_name")).trim().to_owned();
                let affiliations = a["affiliations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(strip_contacts)
                    .collect();
                let Some(orcid) = atlas_core::withhold::normalise_orcid(orcid)
                    .filter(|o| atlas_core::identity_policy::valid_orcid(o))
                else {
                    continue;
                };
                let occurrence_id = format!("pubmed.author:{}-{author_index}", id.replace(':', "_"));
                let occurrence = b.record(SourceRecord {
                    entity: ent,
                    locator: Locator::Record(format!("records[{i}].authors[{author_index}]")),
                    id: occurrence_id.clone(),
                    url: Some(format!(
                        "https://pubmed.ncbi.nlm.nih.gov/{}/",
                        id.trim_start_matches("PMID:")
                    )),
                    fetched_at: None,
                    hash: RecordHash::CanonicalJson,
                    sha256: cache::canonical_sha256(a),
                });
                authors.push(OrcidAuthor {
                    orcid,
                    occurrence_id,
                    name,
                    affiliations,
                    paper: id.clone(),
                    gene: symbol.to_owned(),
                    record: occurrence,
                });
            }
        }
    }
    b.finish(
        act,
        &[
            ("records", read),
            ("papers", b.data.papers.len()),
            ("orcid_author_mentions", authors.len()),
        ],
    );
    b.data.coverage.push(Coverage {
        scope: "articles naming the gene in title or abstract (PubMed [tiab], newest 5,000)".into(),
        ..scope.coverage
    });
    Ok(authors)
}
