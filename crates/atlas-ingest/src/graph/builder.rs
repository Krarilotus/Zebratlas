//! Shared state of the graph build: provenance registry, record table, node dedupe and edge merge.

use std::collections::HashMap;
use std::time::SystemTime;

use atlas_core::Atlas;
use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, RecIdx, Relation, SourceRecord};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{Activity, ActivityIdx, Agent, EntityIdx, SourceEntity};

use crate::sources;

pub struct Builder<'a> {
    pub atlas: &'a Atlas,
    pub data: GraphData,
    agent: Agent,
    edges: HashMap<String, u32>,
    pub nodes: HashMap<String, (NodeKind, u32)>,
}

/// Edge under construction.
pub struct NewEdge<'s> {
    pub from: &'s str,
    pub relation: Relation,
    pub to: &'s str,
    pub kind: EdgeKind,
    pub level: LinkLevel,
    pub reason: String,
    pub activity: ActivityIdx,
}

impl<'a> Builder<'a> {
    pub fn new(atlas: &'a Atlas, agent: Agent) -> Self {
        Self {
            atlas,
            data: GraphData::default(),
            agent,
            edges: HashMap::new(),
            nodes: HashMap::new(),
        }
    }

    pub fn entity(&mut self, entity: SourceEntity) -> EntityIdx {
        self.data.provenance.add_entity(entity)
    }

    pub fn start(&mut self, id: &str, label: &str, used: &[EntityIdx]) -> ActivityIdx {
        self.data.provenance.add_activity(Activity {
            id: id.to_owned(),
            label: label.to_owned(),
            started_at: Some(sources::rfc3339(SystemTime::now())),
            used: used.to_vec(),
            agent: self.agent.clone(),
            ..Activity::default()
        })
    }

    pub fn finish(&mut self, act: ActivityIdx, counts: &[(&str, usize)]) {
        let a = self.data.provenance.activity_mut(act);
        a.ended_at = Some(sources::rfc3339(SystemTime::now()));
        for (k, n) in counts {
            a.count(k, *n as u64);
        }
    }

    pub fn count(&mut self, act: ActivityIdx, key: &str, n: usize) {
        self.data.provenance.activity_mut(act).count(key, n as u64);
    }

    pub fn param(&mut self, act: ActivityIdx, key: &str, value: impl Into<String>) {
        self.data
            .provenance
            .activity_mut(act)
            .parameters
            .insert(key.to_owned(), value.into());
    }

    pub fn record(&mut self, rec: SourceRecord) -> RecIdx {
        self.data.records.push(rec);
        (self.data.records.len() - 1) as RecIdx
    }

    /// Existing node of this id.
    pub fn node(&self, id: &str) -> Option<(NodeKind, u32)> {
        self.nodes.get(id).copied()
    }

    pub fn register(&mut self, id: &str, kind: NodeKind, idx: usize) {
        self.nodes.insert(id.to_owned(), (kind, idx as u32));
    }

    /// Add an edge, or merge records into the edge with the same id (the stronger level wins).
    pub fn edge(&mut self, e: NewEdge<'_>, records: &[RecIdx]) {
        let id = atlas_core::node::edge_id(e.from, e.relation.as_str(), e.to);
        if let Some(&i) = self.edges.get(&id) {
            let edge = &mut self.data.edges[i as usize];
            for r in records {
                if !edge.records.contains(r) {
                    edge.records.push(*r);
                }
            }
            if e.level < edge.level {
                edge.level = e.level;
                edge.kind = e.kind;
                edge.reason = e.reason;
            }
            return;
        }
        self.edges.insert(id, self.data.edges.len() as u32);
        self.data.edges.push(GraphEdge {
            from: e.from.to_owned(),
            relation: e.relation,
            to: e.to.to_owned(),
            kind: e.kind,
            level: e.level,
            reason: e.reason,
            activity: e.activity,
            records: records.to_vec(),
        });
    }

    /// Canonical gene id (HGNC, else NCBIGene, else symbol) of an atlas gene symbol.
    pub fn gene_id(&self, symbol: &str) -> Option<String> {
        self.atlas.gene(symbol).map(|g| self.atlas.gene_at(g).id().to_owned())
    }

    /// Organisation node id for a free-text name.
    pub fn org_id(name: &str) -> String {
        format!("org:{}", slug(name))
    }
}

/// Lowercase ASCII words joined by `-` (`Baylor College of Medicine` → `baylor-college-of-medicine`).
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Python `re.sub(r"[^0-9a-z]+", " ", text.lower()).strip()` (trials.py `normalize_name`).
pub fn normalize_name(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_digit() || c.is_ascii_lowercase() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

/// Affiliation without e-mail addresses (D13: never surface scraped personal contacts).
pub fn strip_contacts(affiliation: &str) -> String {
    let mut words: Vec<&str> = affiliation.split_whitespace().filter(|w| !w.contains('@')).collect();
    loop {
        let n = words.len();
        if n >= 2 && words[n - 2].eq_ignore_ascii_case("electronic") && words[n - 1].eq_ignore_ascii_case("address:") {
            words.truncate(n - 2);
        } else if n >= 1
            && (words[n - 1].eq_ignore_ascii_case("email:") || words[n - 1].eq_ignore_ascii_case("e-mail:"))
        {
            words.truncate(n - 1);
        } else {
            break;
        }
    }
    words.join(" ").trim_end_matches([',', ';', ' ']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_helpers() {
        assert_eq!(
            normalize_name("STXBP1-Related  Encephalopathy!"),
            "stxbp1 related encephalopathy"
        );
        assert_eq!(slug("Baylor College of Medicine"), "baylor-college-of-medicine");
        assert_eq!(
            strip_contacts("Dept X, Paris, France. Electronic address: a@b.fr."),
            "Dept X, Paris, France."
        );
        assert_eq!(
            strip_contacts("Hospital, Paris, France. cyril@aphp.fr"),
            "Hospital, Paris, France."
        );
    }
}
