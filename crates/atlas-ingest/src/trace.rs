//! Provenance trace (D43): everything derived from one person.
//!
//! Given an ORCID, a name (+ affiliation), an e-mail or a node id, [`trace`] lists
//! - graph **nodes** (people matched directly, plus everyone linked by `same_as` identity edges),
//! - **edges** touching them,
//! - **graph records** (`prov:wasDerivedFrom` of those nodes and edges: cache file + locator) and
//!   the **derivation chain** of their source entities (entity → the activities that used it →
//!   the entity's own upstream description),
//! - **study contacts** with that name / e-mail,
//! - **cache records** that mention an identifier although no graph item uses them (raw scan of the
//!   people-related caches, record granularity),
//! - **release files** (file + line) that mention a node id or identifier,
//! - the salted **suppression keys** a reviewer's approval writes.
//!
//! The report holds personal data (names in locators, ids); callers print [`TraceReport::counts`]
//! unless an authorised reviewer asks for the detail.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use atlas_core::Graph;
use atlas_core::graph::{Person, RecIdx, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::text::normalize_label;
use atlas_core::withhold::{KeyKind, Salt, normalise_orcid};
use serde::Serialize;
use serde_json::Value;

/// What to trace. Any combination; all given identifiers are followed.
#[derive(Clone, Debug, Default)]
pub struct TraceQuery {
    pub orcid: Option<String>,
    pub name: Option<String>,
    pub affiliation: Option<String>,
    pub email: Option<String>,
    pub node: Option<String>,
}

impl TraceQuery {
    pub fn is_empty(&self) -> bool {
        [&self.orcid, &self.name, &self.email, &self.node]
            .iter()
            .all(|v| v.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
}

#[derive(Clone, Debug)]
pub struct TraceOptions {
    /// Data-relative cache dirs scanned for mentions (default: the people-related caches).
    pub cache_dirs: Vec<String>,
    /// Release directories scanned for mentions (default: `<repo>/release`).
    pub release_dirs: Vec<PathBuf>,
    /// Skip cache files larger than this (bytes).
    pub max_file_bytes: u64,
}

impl TraceOptions {
    pub fn for_data(data: &Path) -> Self {
        let repo = data.parent().unwrap_or(data);
        Self {
            cache_dirs: ["cache/people", "cache/pubmed", "cache/reporter", "cache/contacts"]
                .map(String::from)
                .to_vec(),
            release_dirs: vec![repo.join("release")],
            max_file_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TracedNode {
    pub id: String,
    pub kind: &'static str,
    /// `orcid`, `node`, `name`, `name+affiliation`, `same_as`.
    pub matched_by: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct TracedRecord {
    /// Data-relative file.
    pub file: String,
    pub locator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TracedContact {
    pub study: String,
    /// `central` / `official`.
    pub role: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ReleaseHit {
    pub file: String,
    pub line: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TraceReport {
    pub nodes: Vec<TracedNode>,
    pub edges: Vec<String>,
    pub graph_records: Vec<TracedRecord>,
    /// `entity file ← activity id` and `entity file ← upstream` lines (`prov:wasDerivedFrom`).
    pub derivation: Vec<String>,
    pub contacts: Vec<TracedContact>,
    /// Cache records mentioning an identifier (includes ones the graph does not use).
    pub cache_records: Vec<TracedRecord>,
    pub release: Vec<ReleaseHit>,
    /// Salted suppression keys (hex) for the matched people, contacts and query identifiers.
    pub keys: Vec<String>,
}

/// Counts only: safe to log or show in reports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TraceCounts {
    pub nodes: usize,
    pub edges: usize,
    pub graph_records: usize,
    pub source_entities: usize,
    pub contacts: usize,
    pub cache_records: usize,
    pub cache_records_not_in_graph: usize,
    pub release_files: usize,
    pub release_lines: usize,
    pub keys: usize,
}

impl TraceReport {
    pub fn counts(&self) -> TraceCounts {
        let in_graph: HashSet<&TracedRecord> = self.graph_records.iter().collect();
        let files: BTreeSet<&str> = self.release.iter().map(|h| h.file.as_str()).collect();
        let entities: BTreeSet<&str> = self.graph_records.iter().map(|r| r.file.as_str()).collect();
        TraceCounts {
            nodes: self.nodes.len(),
            edges: self.edges.len(),
            graph_records: self.graph_records.len(),
            source_entities: entities.len(),
            contacts: self.contacts.len(),
            cache_records: self.cache_records.len(),
            cache_records_not_in_graph: self.cache_records.iter().filter(|r| !in_graph.contains(r)).count(),
            release_files: files.len(),
            release_lines: self.release.len(),
            keys: self.keys.len(),
        }
    }
}

fn person_matches(p: &Person, name: &str, aff: Option<&str>) -> bool {
    let names = std::iter::once(&p.name).chain(&p.name_variants);
    let name_ok = names.into_iter().any(|n| normalize_label(n) == name);
    name_ok && aff.is_none_or(|a| p.affiliations.iter().any(|x| normalize_label(x).contains(a)))
}

/// Trace one person through the graph, the caches under `data` and the release directories.
pub fn trace(data: &Path, graph: &Graph, q: &TraceQuery, salt: &Salt, opts: &TraceOptions) -> TraceReport {
    let mut r = TraceReport::default();
    let orcid = q.orcid.as_deref().and_then(normalise_orcid);
    let name = q.name.as_deref().map(normalize_label).filter(|n| !n.is_empty());
    let aff = q.affiliation.as_deref().map(normalize_label).filter(|a| !a.is_empty());
    let email = q
        .email
        .as_deref()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| e.contains('@'));

    // 1. Direct matches.
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    if let Some(id) = q.node.as_deref().map(str::trim).filter(|s| !s.is_empty())
        && graph.node(id).is_some()
    {
        found.insert(id.to_owned(), "node".into());
    }
    let people = graph.data().people.iter();
    for p in people {
        let by_orcid = orcid.as_deref().is_some_and(|o| {
            p.id.strip_prefix("ORCID:") == Some(o) || p.orcids.iter().any(|x| normalise_orcid(x).as_deref() == Some(o))
        });
        if by_orcid {
            found.entry(p.id.clone()).or_insert_with(|| "orcid".into());
        } else if let Some(n) = &name
            && person_matches(p, n, aff.as_deref())
        {
            let how = if aff.is_some() { "name+affiliation" } else { "name" };
            found.entry(p.id.clone()).or_insert_with(|| how.into());
        }
    }
    // 2. Identity closure over same_as edges.
    let mut queue: VecDeque<String> = found.keys().cloned().collect();
    while let Some(id) = queue.pop_front() {
        for inc in graph.incident(&id) {
            if inc.edge.relation == Relation::SameAs && !found.contains_key(inc.other) {
                found.insert(inc.other.to_owned(), "same_as".into());
                queue.push_back(inc.other.to_owned());
            }
        }
    }
    // 3. Nodes, edges, records.
    let mut records: BTreeSet<RecIdx> = BTreeSet::new();
    let mut edges: BTreeSet<u32> = BTreeSet::new();
    for (id, how) in &found {
        let Some(key) = graph.node(id) else { continue };
        r.nodes.push(TracedNode {
            id: id.clone(),
            kind: key.kind.as_str(),
            matched_by: how.clone(),
        });
        records.extend(graph.node_records(key));
        if key.kind == NodeKind::Person {
            r.keys.extend(salt.person_keys(graph.person(key.idx)));
        }
        for inc in graph.incident(id) {
            edges.insert(inc.idx);
        }
    }
    for &e in &edges {
        let edge = graph.edge(e);
        r.edges.push(edge.id());
        records.extend(&edge.records);
    }
    let prov = graph.provenance();
    let mut entities = BTreeSet::new();
    for &i in &records {
        let rec = graph.record(i);
        entities.insert(rec.entity.0);
        r.graph_records.push(TracedRecord {
            file: prov.entity(rec.entity).file.clone(),
            locator: rec.locator.to_string(),
        });
    }
    // 4. Derivation chain of the source entities (PROV-O used / wasDerivedFrom).
    for e in entities {
        let ent = &prov.entities[usize::from(e)];
        for a in prov.activities.iter().filter(|a| a.used.iter().any(|u| u.0 == e)) {
            r.derivation.push(format!("{} <- used by {}", ent.file, a.id));
        }
        r.derivation.push(format!("{} <- derived from {}", ent.file, ent.url));
    }
    // 5. Study contacts.
    for c in &graph.data().contacts {
        for x in &c.central {
            let by_email = email
                .as_deref()
                .is_some_and(|e| x.email.as_deref().map(str::to_lowercase).as_deref() == Some(e));
            let by_name = name.as_deref().is_some_and(|n| normalize_label(&x.name) == n);
            if by_email || by_name {
                r.contacts.push(TracedContact {
                    study: c.study.clone(),
                    role: "central",
                });
                r.keys.extend(salt.key(KeyKind::NameAff, &format!("{}|", x.name)));
                r.keys
                    .extend(x.email.as_deref().and_then(|e| salt.key(KeyKind::Email, e)));
            }
        }
        for x in &c.officials {
            let by_name = name.as_deref().is_some_and(|n| normalize_label(&x.name) == n)
                && aff
                    .as_deref()
                    .is_none_or(|a| normalize_label(&x.affiliation).contains(a));
            if by_name {
                r.contacts.push(TracedContact {
                    study: c.study.clone(),
                    role: "official",
                });
                r.keys
                    .extend(salt.key(KeyKind::NameAff, &format!("{}|{}", x.name, x.affiliation)));
            }
        }
    }
    // Query identifiers themselves (so a re-ingested source with a new node id still matches).
    r.keys
        .extend(orcid.as_deref().and_then(|o| salt.key(KeyKind::Orcid, o)));
    r.keys
        .extend(email.as_deref().and_then(|e| salt.key(KeyKind::Email, e)));
    if let (Some(n), Some(a)) = (&name, &aff) {
        r.keys.extend(salt.key(KeyKind::NameAff, &format!("{n}|{a}")));
    }
    // 6. Raw mentions in the caches and the release.
    let mut needles: BTreeSet<String> = BTreeSet::new();
    needles.extend(orcid.iter().cloned());
    needles.extend(email.iter().cloned());
    for n in &r.nodes {
        needles.insert(n.id.to_lowercase());
        if let Some(o) = n.id.strip_prefix("ORCID:") {
            needles.insert(o.to_lowercase());
        }
    }
    for n in &r.nodes {
        if let Some(NodeKey {
            kind: NodeKind::Person,
            idx,
        }) = graph.node(&n.id)
        {
            let p = graph.person(idx);
            needles.extend(
                p.orcids
                    .iter()
                    .filter_map(|o| normalise_orcid(o))
                    .map(|o| o.to_lowercase()),
            );
            for x in std::iter::once(&p.name).chain(&p.name_variants) {
                needles.insert(x.trim().to_lowercase());
            }
        }
    }
    if let Some(n) = q.name.as_deref() {
        needles.insert(n.trim().to_lowercase());
    }
    needles.retain(|n| n.chars().count() >= 6);
    let needles: Vec<String> = needles.into_iter().collect();
    if !needles.is_empty() {
        for dir in &opts.cache_dirs {
            scan_cache_dir(
                data,
                &data.join(dir),
                &needles,
                opts.max_file_bytes,
                &mut r.cache_records,
            );
        }
        for dir in &opts.release_dirs {
            scan_release(dir, dir, &needles, &mut r.release);
        }
    }
    r.edges.sort();
    r.graph_records.sort();
    r.graph_records.dedup();
    r.contacts.sort();
    r.contacts.dedup();
    r.cache_records.sort();
    r.cache_records.dedup();
    r.release.sort();
    r.keys.sort();
    r.keys.dedup();
    r
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn hit(text: &str, needles: &[String]) -> bool {
    needles.iter().any(|n| text.contains(n.as_str()))
}

fn scan_cache_dir(data: &Path, dir: &Path, needles: &[String], max: u64, out: &mut Vec<TracedRecord>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files {
        if p.is_dir() {
            scan_cache_dir(data, &p, needles, max, out);
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "json" | "jsonl" | "tsv" | "csv" | "ndjson") {
            continue;
        }
        if std::fs::metadata(&p).map_or(true, |m| m.len() > max) {
            continue;
        }
        let file = rel(data, &p);
        if ext == "json" {
            let Ok(bytes) = std::fs::read(&p) else { continue };
            // Cheap pre-check before parsing.
            let lower = String::from_utf8_lossy(&bytes).to_lowercase();
            if !hit(&lower, needles) {
                continue;
            }
            drop(lower);
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
                out.push(TracedRecord {
                    file,
                    locator: "*".into(),
                });
                continue;
            };
            match v.get("records").and_then(Value::as_array) {
                Some(recs) => {
                    for (i, rec) in recs.iter().enumerate() {
                        if hit(&rec.to_string().to_lowercase(), needles) {
                            out.push(TracedRecord {
                                file: file.clone(),
                                locator: format!("records[{i}]"),
                            });
                        }
                    }
                }
                None => out.push(TracedRecord {
                    file,
                    locator: "*".into(),
                }),
            }
        } else {
            let Ok(f) = std::fs::File::open(&p) else { continue };
            for (i, line) in BufReader::new(f).lines().enumerate() {
                let Ok(line) = line else { break };
                if hit(&line.to_lowercase(), needles) {
                    out.push(TracedRecord {
                        file: file.clone(),
                        locator: format!("L{}", i + 1),
                    });
                }
            }
        }
    }
}

fn scan_release(root: &Path, dir: &Path, needles: &[String], out: &mut Vec<ReleaseHit>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_release(root, &p, needles, out);
            continue;
        }
        let Ok(f) = std::fs::File::open(&p) else { continue };
        for (i, line) in BufReader::new(f).lines().enumerate() {
            let Ok(line) = line else { break };
            if hit(&line.to_lowercase(), needles) {
                out.push(ReleaseHit {
                    file: rel(root.parent().unwrap_or(root), &p),
                    line: i as u64 + 1,
                });
            }
        }
    }
}
