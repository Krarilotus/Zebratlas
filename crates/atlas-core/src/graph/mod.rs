//! The connected layer (D15): studies, grants, papers, people and organisations, linked to the
//! atlas's conditions and genes by provenance-carrying edges.
//!
//! [`GraphData`] is what the graph snapshot stores; [`Graph`] adds the lookup indexes. Edge
//! endpoints are ids (CURIEs): graph nodes resolve here, conditions and genes in the [`crate::Atlas`].

pub mod assets;
pub mod identity;
pub mod initiatives;
pub mod model;
pub mod query;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use assets::*;
pub use identity::*;
pub use initiatives::*;
pub use model::*;

use crate::node::{NodeKey, NodeKind, NodeRef, parse_edge_id};
use crate::provenance::Provenance;
use crate::text::normalize_label;

/// Stable activity ids of the graph build.
pub mod activity {
    pub const INGEST_CTGOV: &str = "activity:ingest-ctgov-studies";
    pub const LINK_CTGOV: &str = "activity:link-studies";
    pub const INGEST_REPORTER: &str = "activity:ingest-reporter-grants";
    pub const INGEST_PUBMED: &str = "activity:ingest-pubmed-articles";
    pub const LINK_TEXT: &str = "activity:link-condition-names-in-text";
    pub const INGEST_PEOPLE: &str = "activity:ingest-people-overlap";
    pub const IDENTITY_PEOPLE: &str = "activity:identity-people";
    pub const INGEST_ORGS: &str = "activity:ingest-organisations";
    pub const INGEST_HGNC: &str = "activity:ingest-hgnc-aliases";
    pub const INGEST_CONTACTS: &str = "activity:ingest-study-contacts";
    pub const INGEST_LABELS: &str = "activity:ingest-wikidata-labels";
    pub const UMBRELLA: &str = "activity:umbrella-mondo-ancestors";
    pub const APPLY_SUPPRESSION: &str = "activity:apply-suppression";
    pub const INGEST_PROGRAMMES: &str = "activity:ingest-therapy-programmes";
    pub const INGEST_SAMPLES: &str = "activity:ingest-models-and-samples";
    pub const INGEST_OUTCOMES: &str = "activity:ingest-outcome-measures";
    pub const INGEST_FUNDING: &str = "activity:ingest-funding";
    pub const INGEST_CLAIMS: &str = "activity:ingest-extracted-claims";
    pub const INGEST_OPENACCESS: &str = "activity:ingest-open-access-links";
    pub const INGEST_KGX: &str = "activity:ingest-kgx";
    pub const IDENTITY_SSSOM: &str = "activity:identity-sssom";
}

/// Stored parts of the connected layer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphData {
    #[serde(default)]
    pub initiatives: Vec<Initiative>,
    pub provenance: Provenance,
    pub records: Vec<SourceRecord>,
    pub studies: Vec<Study>,
    pub grants: Vec<Grant>,
    pub papers: Vec<Paper>,
    pub people: Vec<Person>,
    pub orgs: Vec<Organisation>,
    pub edges: Vec<GraphEdge>,
    /// Condition id → MONDO ancestors (depth ≤ 2) that have direct study links, nearest first,
    /// with the ancestor's name.
    pub umbrella: Vec<(String, Vec<(String, String)>)>,
    pub gene_aliases: Vec<GeneAlias>,
    pub coverage: Vec<Coverage>,
    /// Official contacts per study (where the contacts cache covers it).
    pub contacts: Vec<StudyContacts>,
    pub wiki_items: Vec<WikiItem>,
    pub wiki_names: Vec<WikiName>,
    /// Models, samples, programmes, outcome measures, funding calls (D39 jobs).
    #[serde(default)]
    pub assets: Vec<Asset>,
    /// Licence + class per source entity of `provenance` (D37 §3).
    #[serde(default)]
    pub licences: Vec<EntityLicence>,
    #[serde(default)]
    pub open_access: Vec<OpenAccess>,
    /// SSSOM exact-match identity: `(alias id, canonical id)`, sorted by alias.
    #[serde(default)]
    pub aliases: Vec<(String, String)>,
    /// One explicit PROV activity for each retained SSSOM identity cluster.
    #[serde(default)]
    pub identity_merges: Vec<IdentityMerge>,
    /// Quarantined source records (`data/cache/quarantine.json`), sorted by record.
    #[serde(default)]
    pub quarantine: Vec<Quarantine>,
}

/// Shared withholding interface for quarantine and D43 suppression. Loaded graphs attach the
/// current `Withhold`; snapshots retain quarantine marks for offline callers.
pub trait RecordWithhold {
    /// Reason when any of `records` is withheld.
    fn records_withheld(&self, records: &[RecIdx]) -> Option<&str>;

    /// Reason when a node's records are withheld.
    fn node_withheld(&self, key: NodeKey) -> Option<&str>;
}

impl RecordWithhold for Graph {
    fn records_withheld(&self, records: &[RecIdx]) -> Option<&str> {
        records
            .iter()
            .find_map(|r| self.withheld_records.get(*r as usize).and_then(|x| x.as_deref()))
            .or_else(|| self.quarantined(records))
    }

    fn node_withheld(&self, key: NodeKey) -> Option<&str> {
        self.withheld_nodes
            .get(&key)
            .map(|reason| reason.as_ref())
            .or_else(|| self.node_quarantined(key))
    }
}

/// The connected layer with its indexes (rebuilt on load).
#[derive(Debug, Default)]
pub struct Graph {
    data: GraphData,
    query: query::QueryIndex,
    /// Derived visibility decisions, rebuilt when lists are attached; never serialized.
    withheld_records: Vec<Option<Box<str>>>,
    withheld_nodes: HashMap<NodeKey, Box<str>>,
    nodes: HashMap<Box<str>, NodeKey>,
    /// Node id → incident edges (out and in), ascending.
    adjacency: HashMap<Box<str>, Vec<u32>>,
    umbrella: HashMap<Box<str>, u32>,
    /// Normalised alias/previous symbol/name → gene alias entries.
    aliases: HashMap<Box<str>, Vec<u32>>,
    /// Approved symbol / HGNC id → alias entry (no request-time HGNC scan).
    gene_alias_ids: HashMap<Box<str>, u32>,
    contacts: HashMap<Box<str>, u32>,
    /// Normalised Wikidata label/alias → name entries.
    wiki: HashMap<Box<str>, Vec<u32>>,
    /// Entity index → licence entry.
    licences: HashMap<u16, u32>,
    /// Paper id → open-access entries.
    open_access: HashMap<Box<str>, Vec<u32>>,
    /// Canonical id → indexes into `data.aliases` (SSSOM exact merges).
    aliases_of: HashMap<Box<str>, Vec<u32>>,
}

/// One incident edge as seen from a node.
#[derive(Clone, Copy, Debug)]
pub struct Incident<'a> {
    pub idx: u32,
    pub edge: &'a GraphEdge,
    /// The other end.
    pub other: &'a str,
    /// The node is the edge's `from`.
    pub outgoing: bool,
}

impl Graph {
    pub fn new(data: GraphData) -> Self {
        let query = query::QueryIndex::build(&data);
        let mut nodes = HashMap::new();
        let mut add = |kind: NodeKind, ids: &mut dyn Iterator<Item = &String>| {
            for (i, id) in ids.enumerate() {
                nodes.insert(id.as_str().into(), NodeKey { kind, idx: i as u32 });
            }
        };
        add(NodeKind::Study, &mut data.studies.iter().map(|s| &s.id));
        add(NodeKind::Grant, &mut data.grants.iter().map(|s| &s.id));
        add(NodeKind::Paper, &mut data.papers.iter().map(|s| &s.id));
        add(NodeKind::Person, &mut data.people.iter().map(|s| &s.id));
        add(NodeKind::Organisation, &mut data.orgs.iter().map(|s| &s.id));
        add(NodeKind::Asset, &mut data.assets.iter().map(|s| &s.id));
        let mut adjacency: HashMap<Box<str>, Vec<u32>> = HashMap::new();
        for (i, e) in data.edges.iter().enumerate() {
            adjacency.entry(e.from.as_str().into()).or_default().push(i as u32);
            adjacency.entry(e.to.as_str().into()).or_default().push(i as u32);
        }
        let umbrella = data
            .umbrella
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str().into(), i as u32))
            .collect();
        let mut aliases: HashMap<Box<str>, Vec<u32>> = HashMap::new();
        let mut gene_alias_ids = HashMap::new();
        for (i, g) in data.gene_aliases.iter().enumerate() {
            for id in [&g.symbol, &g.hgnc] {
                gene_alias_ids.entry(id.as_str().into()).or_insert(i as u32);
            }
            for text in g.aliases.iter().chain(&g.previous).chain(std::iter::once(&g.name)) {
                let key = normalize_label(text);
                if !key.is_empty() {
                    let list = aliases.entry(key.into()).or_default();
                    if list.last() != Some(&(i as u32)) {
                        list.push(i as u32);
                    }
                }
            }
        }
        let contacts = data
            .contacts
            .iter()
            .enumerate()
            .map(|(i, c)| (c.study.as_str().into(), i as u32))
            .collect();
        let mut wiki: HashMap<Box<str>, Vec<u32>> = HashMap::new();
        for (i, n) in data.wiki_names.iter().enumerate() {
            let key = normalize_label(&n.text);
            if !key.is_empty() {
                wiki.entry(key.into()).or_default().push(i as u32);
            }
        }
        let licences = data
            .licences
            .iter()
            .enumerate()
            .map(|(i, l)| (l.entity.0, i as u32))
            .collect();
        let mut open_access: HashMap<Box<str>, Vec<u32>> = HashMap::new();
        for (i, o) in data.open_access.iter().enumerate() {
            open_access.entry(o.paper.as_str().into()).or_default().push(i as u32);
        }
        let mut aliases_of: HashMap<Box<str>, Vec<u32>> = HashMap::new();
        for (i, (_, c)) in data.aliases.iter().enumerate() {
            aliases_of.entry(c.as_str().into()).or_default().push(i as u32);
        }
        Self {
            query,
            withheld_records: Vec::new(),
            withheld_nodes: HashMap::new(),
            gene_alias_ids,
            aliases_of,
            data,
            nodes,
            adjacency,
            umbrella,
            aliases,
            contacts,
            wiki,
            licences,
            open_access,
        }
    }

    /// Attach the current lists without storing secrets or identifiers in the snapshot.
    pub fn set_withhold(&mut self, withhold: crate::withhold::Withhold) {
        self.withheld_records = (0..self.data.records.len())
            .map(|i| withhold.records(&self.data, &[i as u32]).map(|hit| hit.reason.into()))
            .collect();
        self.withheld_nodes.clear();
        for kind in [
            NodeKind::Study,
            NodeKind::Grant,
            NodeKind::Paper,
            NodeKind::Person,
            NodeKind::Organisation,
            NodeKind::Asset,
        ] {
            for idx in 0..self.node_count(kind) as u32 {
                let key = NodeKey { kind, idx };
                if let Some(hit) = withhold.node(self, key) {
                    self.withheld_nodes.insert(key, hit.reason.into());
                }
            }
        }
    }

    pub fn data(&self) -> &GraphData {
        &self.data
    }

    /// Selective relation/country/status postings for combined graph query compilers.
    pub fn query(&self) -> &query::QueryIndex {
        &self.query
    }

    /// The stored parts, dropping the indexes (to edit and re-index, e.g. suppression tests).
    pub fn into_data(self) -> GraphData {
        self.data
    }

    pub fn provenance(&self) -> &Provenance {
        &self.data.provenance
    }

    pub fn record(&self, r: RecIdx) -> &SourceRecord {
        &self.data.records[r as usize]
    }

    pub fn edges(&self) -> &[GraphEdge] {
        &self.data.edges
    }

    pub fn edge(&self, idx: u32) -> &GraphEdge {
        &self.data.edges[idx as usize]
    }

    pub fn studies(&self) -> &[Study] {
        &self.data.studies
    }

    pub fn study(&self, idx: u32) -> &Study {
        &self.data.studies[idx as usize]
    }

    pub fn grant(&self, idx: u32) -> &Grant {
        &self.data.grants[idx as usize]
    }

    pub fn paper(&self, idx: u32) -> &Paper {
        &self.data.papers[idx as usize]
    }

    pub fn person(&self, idx: u32) -> &Person {
        &self.data.people[idx as usize]
    }

    pub fn org(&self, idx: u32) -> &Organisation {
        &self.data.orgs[idx as usize]
    }

    pub fn asset(&self, idx: u32) -> &Asset {
        &self.data.assets[idx as usize]
    }

    pub fn assets(&self) -> &[Asset] {
        &self.data.assets
    }

    /// Licence of a record's source entity (`None`: the source states none; class `unknown`).
    pub fn licence_of(&self, r: RecIdx) -> Option<&EntityLicence> {
        let e = self.record(r).entity.0;
        self.licences.get(&e).map(|&i| &self.data.licences[i as usize])
    }

    /// Weakest licence class over records (a node or edge is only as open as its least open record).
    pub fn licence_class(&self, records: &[RecIdx]) -> LicenceClass {
        records
            .iter()
            .map(|&r| self.licence_of(r).map_or(LicenceClass::Unknown, |l| l.class))
            .max()
            .unwrap_or(LicenceClass::Unknown)
    }

    /// Legal free-to-read links of a paper.
    pub fn open_access(&self, paper: &str) -> impl Iterator<Item = &OpenAccess> {
        self.open_access
            .get(paper)
            .into_iter()
            .flatten()
            .map(|&i| &self.data.open_access[i as usize])
    }

    /// Quarantine reason when any of `records` is quarantined (fail-closed: such nodes and edges
    /// stay out of UI payloads, the open release and exports).
    pub fn quarantined(&self, records: &[RecIdx]) -> Option<&str> {
        records.iter().find_map(|r| {
            self.data
                .quarantine
                .binary_search_by_key(r, |q| q.record)
                .ok()
                .map(|i| self.data.quarantine[i].reason.as_str())
        })
    }

    /// Quarantine reason of a graph node (by its records).
    pub fn node_quarantined(&self, key: NodeKey) -> Option<&str> {
        self.quarantined(self.node_records(key))
    }

    /// Ids merged into `canonical` by SSSOM exact matches (e.g. a trial's EudraCT / CTIS / ICTRP
    /// registrations, a condition's DOID / MedGen ids).
    pub fn aliases_of(&self, canonical: &str) -> impl Iterator<Item = &str> {
        self.aliases_of
            .get(canonical)
            .into_iter()
            .flatten()
            .map(|&i| self.data.aliases[i as usize].0.as_str())
    }

    /// Canonical id of an SSSOM exact-match alias (the id itself when it is no alias).
    pub fn canonical_id<'a>(&'a self, id: &'a str) -> &'a str {
        match self.data.aliases.binary_search_by(|(a, _)| a.as_str().cmp(id)) {
            Ok(i) => &self.data.aliases[i].1,
            Err(_) => id,
        }
    }

    pub fn identity_merge(&self, id: &str) -> Option<&IdentityMerge> {
        let canonical = self.canonical_id(id);
        self.data
            .identity_merges
            .binary_search_by(|m| m.canonical.as_str().cmp(canonical))
            .ok()
            .map(|i| &self.data.identity_merges[i])
    }

    pub fn coverage(&self) -> &[Coverage] {
        &self.data.coverage
    }

    pub fn coverage_of(&self, source: &str) -> Option<&Coverage> {
        self.data.coverage.iter().find(|c| c.source == source)
    }

    /// Graph node (study, grant, paper, person, organisation) by id.
    pub fn node(&self, id: &str) -> Option<NodeKey> {
        self.nodes.get(id.trim()).copied()
    }

    pub fn node_count(&self, kind: NodeKind) -> usize {
        match kind {
            NodeKind::Study => self.data.studies.len(),
            NodeKind::Grant => self.data.grants.len(),
            NodeKind::Paper => self.data.papers.len(),
            NodeKind::Person => self.data.people.len(),
            NodeKind::Organisation => self.data.orgs.len(),
            NodeKind::Asset => self.data.assets.len(),
            _ => 0,
        }
    }

    pub fn node_ref(&self, key: NodeKey) -> NodeRef {
        let i = key.idx;
        let (id, label) = match key.kind {
            NodeKind::Study => (&self.study(i).id, &self.study(i).title),
            NodeKind::Grant => (&self.grant(i).id, &self.grant(i).title),
            NodeKind::Paper => (&self.paper(i).id, &self.paper(i).title),
            NodeKind::Person => (&self.person(i).id, &self.person(i).name),
            NodeKind::Organisation => (&self.org(i).id, &self.org(i).name),
            NodeKind::Asset => (&self.asset(i).id, &self.asset(i).label),
            other => unreachable!("{other:?} nodes live in the atlas"),
        };
        NodeRef {
            id: id.clone(),
            kind: key.kind,
            label: label.clone(),
        }
    }

    /// Records a graph node derives from.
    pub fn node_records(&self, key: NodeKey) -> &[RecIdx] {
        let i = key.idx;
        match key.kind {
            NodeKind::Study => std::slice::from_ref(&self.study(i).record),
            NodeKind::Grant => &self.grant(i).records,
            NodeKind::Paper => &self.paper(i).records,
            NodeKind::Person => &self.person(i).records,
            NodeKind::Organisation => &self.org(i).records,
            NodeKind::Asset => &self.asset(i).records,
            _ => &[],
        }
    }

    /// Edges touching `id`, in edge order.
    pub fn incident<'a>(&'a self, id: &'a str) -> impl Iterator<Item = Incident<'a>> + 'a {
        let list: &'a [u32] = self.adjacency.get(id).map_or(&[], Vec::as_slice);
        list.iter().map(move |&idx| {
            let edge = self.edge(idx);
            let outgoing = edge.from == id;
            let other = if outgoing { &edge.to } else { &edge.from };
            Incident {
                idx,
                edge,
                other,
                outgoing,
            }
        })
    }

    /// Edge index by `from|relation|to`.
    pub fn edge_by_id(&self, id: &str) -> Option<u32> {
        let (from, relation, to) = parse_edge_id(id)?;
        let relation = Relation::parse(relation)?;
        self.adjacency.get(from)?.iter().copied().find(|&i| {
            let e = self.edge(i);
            e.from == from && e.relation == relation && e.to == to
        })
    }

    /// Wikidata names (any language) that normalise to `text`, with their item.
    pub fn wiki_names(&self, text: &str) -> impl Iterator<Item = (&WikiName, &WikiItem)> {
        let key = normalize_label(text);
        self.wiki.get(key.as_str()).into_iter().flatten().map(|&i| {
            let n = &self.data.wiki_names[i as usize];
            (n, &self.data.wiki_items[n.item as usize])
        })
    }

    /// Official contacts of a study, when the contacts cache covers it.
    pub fn contacts(&self, nct: &str) -> Option<&StudyContacts> {
        self.contacts.get(nct).map(|&i| &self.data.contacts[i as usize])
    }

    /// MONDO ancestors (depth ≤ 2) of a condition that have direct study links: `(id, name)`.
    pub fn umbrella(&self, condition: &str) -> &[(String, String)] {
        self.umbrella
            .get(condition)
            .map_or(&[], |&i| self.data.umbrella[i as usize].1.as_slice())
    }

    /// Gene aliases whose alias, previous symbol or name normalises to `text`.
    pub fn genes_by_alias(&self, text: &str) -> impl Iterator<Item = &GeneAlias> {
        let key = normalize_label(text);
        self.aliases
            .get(key.as_str())
            .into_iter()
            .flatten()
            .map(|&i| &self.data.gene_aliases[i as usize])
    }

    /// Alias entry of a gene symbol or HGNC id.
    pub fn gene_alias(&self, symbol_or_hgnc: &str) -> Option<&GeneAlias> {
        self.gene_alias_ids
            .get(symbol_or_hgnc)
            .map(|&i| &self.data.gene_aliases[i as usize])
    }
}
