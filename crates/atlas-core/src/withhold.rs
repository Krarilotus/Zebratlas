//! Withholding (D43, D36 §3): the one filter for data that must not be shown, exported or released.
//!
//! Two lists, one owner (design/ARCHITECTURE.md, "Withholding"):
//! - **suppression** (`data/suppression.json`): salted SHA-256 keys of person identifiers, written
//!   when a reviewer approves a removal/objection request. Never holds a name, ORCID or e-mail.
//! - **quarantine** (`data/cache/quarantine.json`): source records (cache file + locator).
//!
//! The filter is **fail-closed**: [`Withhold::closed`] (an unreadable list, an unknown version, a
//! foreign salt) withholds every person, every study contact and every edge touching a person.
//! This module has no IO; `atlas_ingest::withhold` reads the files.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Graph;
use crate::graph::{GraphData, Person, RecIdx};
use crate::node::{NodeKey, NodeKind};
use crate::provenance::Locator;

mod keys;
pub use keys::*;

fn url_key(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?;
    if authority.is_empty() || authority.contains('@') || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    url.split(['?', '#']).next()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Graph build, API, exports and release (the default for removal and objection).
    #[default]
    All,
    /// Website/API only (e.g. while a correction is pending); builds and releases also drop it,
    /// because the release never holds more than the site.
    Display,
}

/// One approved suppression (`data/suppression.json` `entries[]`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuppressionEntry {
    /// `sup_<hex>`.
    pub id: String,
    /// Hex keys from [`Salt::key`].
    pub keys: Vec<String>,
    #[serde(default)]
    pub scope: Scope,
    /// `gdpr_art17_erasure`, `gdpr_art21_objection`, ...
    pub reason: String,
    /// Decision date (RFC 3339).
    pub date: String,
    /// Deciding reviewer (agent id, e.g. `agent:reviewer/alice`).
    pub reviewer: String,
    /// Privacy request reference (`pr_…`), when it came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    pub salt_id: String,
}

/// One quarantined record or file (`data/cache/quarantine.json` `entries[]`/`records[]`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(alias = "cache_file", alias = "path")]
    pub file: String,
    /// `records[i]`, `/records/i`, `L<n>` or a line number; missing = the whole file.
    #[serde(default, alias = "record_locator")]
    pub locator: Option<serde_json::Value>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
}

// Legacy acquisition manifests omit schema/version; suppression never does.
#[derive(Deserialize)]
struct QuarantineFile {
    #[serde(default)]
    schema: String,
    #[serde(default = "quarantine_version")]
    version: u32,
    #[serde(alias = "records")]
    entries: Vec<QuarantineEntry>,
}
fn quarantine_version() -> u32 {
    VERSION
}

/// The shared list file shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListFile<E> {
    pub schema: String,
    pub version: u32,
    #[serde(alias = "records")]
    pub entries: Vec<E>,
}

pub type SuppressionFile = ListFile<SuppressionEntry>;

impl SuppressionFile {
    pub fn new() -> Self {
        Self {
            schema: "suppression".into(),
            version: VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WithholdError {
    #[error("{list:?} list is not valid JSON of the expected shape: {detail}")]
    Invalid { list: ListKind, detail: String },
    #[error("{list:?} list has schema {schema:?} version {version}; expected version {VERSION}")]
    Version {
        list: ListKind,
        schema: String,
        version: u32,
    },
    #[error("suppression entry {entry} was hashed with another salt ({found}); this server's salt is {expected}")]
    ForeignSalt {
        entry: String,
        found: String,
        expected: String,
    },
}

/// `cache/<...>` form of a data-relative file path.
pub fn norm_file(f: &str) -> String {
    let f = f.trim().replace('\\', "/");
    let f = f.trim_start_matches("./").trim_start_matches("data/");
    if f.starts_with("cache/") || f.starts_with("raw/") {
        f.to_owned()
    } else {
        format!("cache/{f}")
    }
}

/// Canonical locator text (`records[i]` or `L<n>`); `None` = whole file.
pub fn norm_locator(v: &serde_json::Value) -> Option<String> {
    if let Some(n) = v.as_u64() {
        return Some(format!("L{n}"));
    }
    let l = v.as_str()?.trim().trim_start_matches('#');
    if l.is_empty() || l == "*" {
        return None;
    }
    if let Some(i) = l.strip_prefix("/records/").and_then(|x| x.split('/').next()) {
        return Some(format!("records[{i}]"));
    }
    if let Some(n) = l.strip_prefix("line:").and_then(|n| n.parse::<u64>().ok()) {
        return Some(format!("L{n}"));
    }
    if let Some((root, _)) = l.split_once("]/") {
        return Some(format!("{root}]"));
    }
    if let Ok(n) = l.parse::<u64>() {
        return Some(format!("L{n}"));
    }
    Some(l.to_owned())
}

#[derive(Debug, Default)]
struct Lists {
    suppression: Vec<SuppressionEntry>,
    by_key: HashMap<String, u32>,
    quarantine: Vec<(String, String)>,
    source_urls: HashMap<String, u32>,
    /// (file, locator or "") → quarantine index.
    by_record: HashMap<(String, String), u32>,
}

/// The withholding filter. Cheap to query; build once per load.
#[derive(Debug)]
pub struct Withhold {
    salt: Salt,
    lists: Result<Lists, String>,
}

const CLOSED_REASON: &str = "withholding list unavailable (fail-closed)";

impl Withhold {
    /// Nothing withheld.
    pub fn empty(salt: Salt) -> Self {
        Self {
            salt,
            lists: Ok(Lists::default()),
        }
    }

    /// Withholds every person-derived item (an unreadable list must not open the data).
    pub fn closed(salt: Salt, why: impl Into<String>) -> Self {
        Self {
            salt,
            lists: Err(why.into()),
        }
    }

    /// Parse both lists (`None` = file missing = empty list). Strict: any error is returned, for
    /// builds and releases that must stop. Servers use [`Withhold::from_lists_or_closed`].
    pub fn from_lists(
        salt: Salt,
        suppression: Option<&[u8]>,
        quarantine: Option<&[u8]>,
    ) -> Result<Self, WithholdError> {
        let mut lists = Lists::default();
        if let Some(bytes) = suppression {
            let f: SuppressionFile = serde_json::from_slice(bytes).map_err(|e| WithholdError::Invalid {
                list: ListKind::Suppression,
                detail: e.to_string(),
            })?;
            if f.version != VERSION || f.schema != "suppression" {
                return Err(WithholdError::Version {
                    list: ListKind::Suppression,
                    schema: f.schema,
                    version: f.version,
                });
            }
            for (i, e) in f.entries.into_iter().enumerate() {
                if e.salt_id != salt.id() {
                    return Err(WithholdError::ForeignSalt {
                        entry: e.id,
                        found: e.salt_id,
                        expected: salt.id().to_owned(),
                    });
                }
                for k in &e.keys {
                    lists.by_key.entry(k.clone()).or_insert(i as u32);
                }
                lists.suppression.push(e);
            }
        }
        if let Some(bytes) = quarantine {
            let f: QuarantineFile = serde_json::from_slice(bytes).map_err(|e| WithholdError::Invalid {
                list: ListKind::Quarantine,
                detail: e.to_string(),
            })?;
            if f.version != VERSION || !["", "quarantine", "acquisition.quarantine"].contains(&f.schema.as_str()) {
                return Err(WithholdError::Version {
                    list: ListKind::Quarantine,
                    schema: f.schema,
                    version: f.version,
                });
            }
            for (i, e) in f.entries.into_iter().enumerate() {
                let file = norm_file(&e.file);
                let invalid = |detail: &str| WithholdError::Invalid {
                    list: ListKind::Quarantine,
                    detail: detail.into(),
                };
                if e.file.trim().is_empty()
                    || file.contains(':')
                    || file.split('/').any(|p| p.is_empty() || p == "." || p == "..")
                {
                    return Err(invalid("invalid quarantine cache path"));
                }
                if let Some(v) = &e.locator
                    && (!(v.is_string() || v.is_u64())
                        || v.as_str()
                            .is_some_and(|l| l.trim().is_empty() || l.chars().any(char::is_control)))
                {
                    return Err(invalid("invalid quarantine record locator"));
                }
                let loc = e.locator.as_ref().and_then(norm_locator).unwrap_or_default();
                let id = e.id.unwrap_or_else(|| format!("quarantine[{i}]"));
                let reason = e
                    .reason
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "quarantined".into());
                if let Some(url) = e.source_url.as_deref().and_then(url_key) {
                    lists.source_urls.insert(url.to_owned(), lists.quarantine.len() as u32);
                }
                lists.by_record.insert((file, loc), lists.quarantine.len() as u32);
                lists.quarantine.push((id, reason));
            }
        }
        Ok(Self { salt, lists: Ok(lists) })
    }

    /// Like [`Withhold::from_lists`], but an error gives a closed filter (servers keep running).
    pub fn from_lists_or_closed(salt: Salt, suppression: Option<&[u8]>, quarantine: Option<&[u8]>) -> Self {
        match Self::from_lists(salt.clone(), suppression, quarantine) {
            Ok(w) => w,
            Err(e) => Self::closed(salt, e.to_string()),
        }
    }

    pub fn salt(&self) -> &Salt {
        &self.salt
    }

    /// `Some(why)` when the filter is closed.
    pub fn closed_reason(&self) -> Option<&str> {
        self.lists.as_ref().err().map(String::as_str)
    }

    pub fn suppression_entries(&self) -> &[SuppressionEntry] {
        self.lists.as_ref().map_or(&[], |l| l.suppression.as_slice())
    }

    pub fn quarantine_len(&self) -> usize {
        self.lists.as_ref().map_or(0, |l| l.quarantine.len())
    }

    /// Canonical quarantine pairs, for count-only release reports.
    pub fn quarantine_records(&self) -> impl Iterator<Item = (&str, &str)> {
        self.lists
            .as_ref()
            .ok()
            .into_iter()
            .flat_map(|l| l.by_record.keys())
            .map(|(file, locator)| (file.as_str(), locator.as_str()))
    }

    /// Conservative fallback for older snapshots lacking curated-page derivation links.
    pub fn source_url(&self, url: &str) -> Option<Withheld<'_>> {
        let lists = self.lists.as_ref().ok()?;
        let i = *lists.source_urls.get(url_key(url)?)?;
        let (id, reason) = &lists.quarantine[i as usize];
        Some(Withheld {
            list: ListKind::Quarantine,
            reason,
            entry: id,
        })
    }

    fn closed_hit(&self) -> Withheld<'_> {
        Withheld {
            list: ListKind::Suppression,
            reason: CLOSED_REASON,
            entry: "closed",
        }
    }

    /// The first suppression entry matching any of `keys`.
    pub fn keys<'a>(&'a self, keys: &[String]) -> Option<Withheld<'a>> {
        let l = match &self.lists {
            Ok(l) => l,
            Err(_) => return Some(self.closed_hit()),
        };
        keys.iter().find_map(|k| l.by_key.get(k)).map(|&i| {
            let e = &l.suppression[i as usize];
            Withheld {
                list: ListKind::Suppression,
                reason: &e.reason,
                entry: &e.id,
            }
        })
    }

    /// A graph person (closed filter: every person).
    pub fn person(&self, p: &Person) -> Option<Withheld<'_>> {
        if self.closed_reason().is_some() {
            return Some(self.closed_hit());
        }
        self.keys(&self.salt.person_keys(p))
    }

    /// A named contact (study official or central contact). Closed filter: every contact.
    pub fn contact(&self, name: &str, affiliation: &str, email: Option<&str>) -> Option<Withheld<'_>> {
        let mut keys = Vec::new();
        keys.extend(self.salt.key(KeyKind::NameAff, &format!("{name}|{affiliation}")));
        keys.extend(self.salt.key(KeyKind::NameAff, &format!("{name}|")));
        keys.extend(email.and_then(|e| self.salt.key(KeyKind::Email, e)));
        if keys.is_empty() && self.closed_reason().is_none() {
            return None;
        }
        self.keys(&keys)
    }

    /// A source record by data-relative file + locator (quarantine; whole-file entries included).
    /// A closed filter does not withhold non-person records: it only closes person data.
    pub fn record(&self, file: &str, locator: &Locator) -> Option<Withheld<'_>> {
        let l = self.lists.as_ref().ok()?;
        if l.by_record.is_empty() {
            return None;
        }
        let file = norm_file(file);
        let loc = norm_locator(&serde_json::Value::String(locator.to_string())).unwrap_or_default();
        let i = l
            .by_record
            .get(&(file.clone(), loc))
            .or_else(|| l.by_record.get(&(file, String::new())))?;
        let (id, reason) = &l.quarantine[*i as usize];
        Some(Withheld {
            list: ListKind::Quarantine,
            reason,
            entry: id,
        })
    }

    pub fn records<'a>(&'a self, graph: &GraphData, records: &[RecIdx]) -> Option<Withheld<'a>> {
        records.iter().find_map(|&r| {
            let rec = &graph.records[r as usize];
            let entity = graph.provenance.entity(rec.entity);
            self.record(&entity.file, &rec.locator)
                .or_else(|| rec.url.as_deref().and_then(|url| self.source_url(url)))
                .or_else(|| self.source_url(&entity.url))
        })
    }

    /// A graph node: a suppressed person, or any node with a quarantined record.
    pub fn node<'a>(&'a self, graph: &'a Graph, key: NodeKey) -> Option<Withheld<'a>> {
        if let Some(hit) = self
            .salt
            .key(KeyKind::Node, &graph.node_ref(key).id)
            .and_then(|key| self.keys(&[key]))
            .filter(|_| self.closed_reason().is_none())
        {
            return Some(hit);
        }
        if key.kind == NodeKind::Person
            && let Some(w) = self.person(graph.person(key.idx))
        {
            return Some(w);
        }
        self.records(graph.data(), graph.node_records(key)).or_else(|| {
            graph.node_quarantined(key).map(|reason| Withheld {
                list: ListKind::Quarantine,
                reason,
                entry: "snapshot",
            })
        })
    }

    /// A node by id (non-graph ids, e.g. atlas conditions, are never withheld here).
    pub fn node_id<'a>(&'a self, graph: &'a Graph, id: &str) -> Option<Withheld<'a>> {
        graph.node(id).and_then(|k| self.node(graph, k))
    }

    /// A graph edge: withheld when either end or one of its records is.
    pub fn edge<'a>(&'a self, graph: &'a Graph, idx: u32) -> Option<Withheld<'a>> {
        let e = graph.edge(idx);
        self.node_id(graph, &e.from)
            .or_else(|| self.node_id(graph, &e.to))
            .or_else(|| self.records(graph.data(), &e.records))
    }
}

/// What [`apply_suppression`] removed (counts only: the build's PROV activity records these).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub people: usize,
    pub edges: usize,
    pub contacts: usize,
    pub records_redacted: usize,
}

/// Remove suppressed people, their edges and matching study contacts from a graph before it is
/// stored, and redact the upstream record ids/URLs that only they used (so no name survives in
/// a `person:<name-slug>` record id). The source caches stay untouched; the next build applies the
/// list again, so re-ingestion never brings the data back. A closed filter removes every person.
pub fn apply_suppression(data: &mut GraphData, w: &Withhold) -> Applied {
    let mut out = Applied::default();
    let mut removed: HashMap<String, String> = HashMap::new();
    let mut keep = Vec::with_capacity(data.people.len());
    let mut redact: Vec<(RecIdx, String)> = Vec::new();
    for p in std::mem::take(&mut data.people) {
        match w.person(&p) {
            Some(hit) => {
                let tag = format!("suppressed:{}", hit.entry);
                redact.extend(p.records.iter().map(|&r| (r, tag.clone())));
                removed.insert(p.id.clone(), tag);
                out.people += 1;
            }
            None => keep.push(p),
        }
    }
    data.people = keep;
    // Identity metadata must obey the same suppression rules as the person nodes it describes.
    let mut kept_merges = Vec::new();
    for merge in std::mem::take(&mut data.identity_merges) {
        let hidden = removed.contains_key(&merge.canonical)
            || merge.members.iter().any(|m| removed.contains_key(&m.id))
            || (w.closed_reason().is_some() && merge.mappings.iter().any(|m| m.rule_id.starts_with("R-PER-")));
        if hidden {
            let tag = "suppressed:identity-lineage".to_owned();
            redact.extend(
                merge
                    .members
                    .iter()
                    .flat_map(|m| m.derived_from.iter().map(|&r| (r, tag.clone()))),
            );
            data.aliases.retain(|(_, canonical)| canonical != &merge.canonical);
        } else {
            kept_merges.push(merge);
        }
    }
    data.identity_merges = kept_merges;
    if !removed.is_empty() {
        let before = data.edges.len();
        let mut kept = Vec::with_capacity(before);
        for e in std::mem::take(&mut data.edges) {
            match removed.get(&e.from).or_else(|| removed.get(&e.to)) {
                Some(tag) => redact.extend(e.records.iter().map(|&r| (r, tag.clone()))),
                None => kept.push(e),
            }
        }
        out.edges = before - kept.len();
        data.edges = kept;
    }
    for c in &mut data.contacts {
        let before = c.central.len() + c.officials.len();
        c.central
            .retain(|x| w.contact(&x.name, "", x.email.as_deref()).is_none());
        c.officials
            .retain(|x| w.contact(&x.name, &x.affiliation, None).is_none());
        out.contacts += before - c.central.len() - c.officials.len();
    }
    // Records still used by a kept node or edge keep their id (they belong to that item too).
    let mut used = vec![false; data.records.len()];
    let mut mark = |rs: &[RecIdx]| rs.iter().for_each(|&r| used[r as usize] = true);
    data.people.iter().for_each(|p| mark(&p.records));
    data.edges.iter().for_each(|e| mark(&e.records));
    data.papers.iter().for_each(|p| mark(&p.records));
    data.grants.iter().for_each(|g| mark(&g.records));
    data.orgs.iter().for_each(|o| mark(&o.records));
    data.studies.iter().for_each(|s| mark(std::slice::from_ref(&s.record)));
    redact.sort();
    redact.dedup_by_key(|(r, _)| *r);
    for (r, tag) in redact {
        let rec = &mut data.records[r as usize];
        if !used[r as usize] && rec.id != tag {
            rec.id = tag;
            rec.url = None;
            out.records_redacted += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests;
