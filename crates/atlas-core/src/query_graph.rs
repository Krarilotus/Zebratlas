//! Index-backed query exploration. No model calls, IO, or upstream query-builder code.
//! Counts are distinct visible neighbours, not estimates of a SPARQL result set.

use crate::{
    Atlas, Graph,
    graph::RecordWithhold,
    node::{EdgeKind, NodeKey, NodeKind, NodeRef},
    provenance::RecordRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Outgoing,
    Incoming,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestRequest {
    pub node: Option<String>,
    pub class: Option<String>,
    pub relation: Option<String>,
    pub direction: Option<Direction>,
    pub target_class: Option<String>,
    pub q: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Citation {
    pub source_url: String,
    pub retrieved_at: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub record_sha256: Option<String>,
    pub record_locator: String,
    #[serde(rename = "prov:wasDerivedFrom")]
    pub entity: String,
}

#[derive(Debug, Serialize)]
pub struct Suggestion {
    pub relation: String,
    pub direction: Direction,
    pub target_class: String,
    pub target_kind: NodeKind,
    /// Distinct targets after current visibility and prefix filters.
    pub count: usize,
    pub node: Option<NodeRef>,
    /// One witness, not an exhaustive list of the aggregate's supporting records.
    pub witness_edge: String,
    /// Connected assertion status; direct atlas records have no assertion status.
    pub witness_status: Option<EdgeKind>,
    pub evidence_sample: Vec<Citation>,
}

#[derive(Debug, Serialize)]
pub struct SuggestPage {
    pub version: u32,
    pub phase: &'static str,
    pub total: usize,
    pub offset: usize,
    pub items: Vec<Suggestion>,
    pub index_sha256: String,
    pub provenance_url: &'static str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub relation: String,
    pub direction: Direction,
}
/// Small index preview contract. The authoritative D57a compiler remains in atlas-ask.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub focus: Vec<String>,
    pub steps: Vec<Step>,
    pub output: Option<NodeKind>,
    pub country: Option<String>,
    pub recruiting: Option<bool>,
    pub kind: Option<String>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub step: usize,
    pub ids: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Preview {
    pub count: usize,
    pub results: Vec<NodeRef>,
    pub index_sha256: String,
    pub provenance_url: &'static str,
}

#[derive(Debug)]
struct IndexedNode {
    node: NodeRef,
    class: String,
    prefixes: Vec<String>,
    countries: Vec<String>,
    recruiting: Option<bool>,
}
#[derive(Debug)]
enum Lineage {
    Connected(u32),
    Atlas(Vec<RecordRef>),
}
#[derive(Debug)]
struct Link {
    from: usize,
    to: usize,
    id: String,
    lineage: Lineage,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct GroupKey {
    relation: String,
    direction: Direction,
    class: String,
    kind: NodeKind,
}
#[derive(Debug)]
struct Group {
    key: Arc<GroupKey>,
    targets: Vec<Target>,
}

#[derive(Clone, Copy, Debug)]
struct Target {
    node: u32,
    start: u32,
    end: u32,
}

/// Flat postings avoid minimum-sized tree allocations for millions of tiny groups.
/// Group metadata is interned once; only integer postings repeat per scope.
#[derive(Debug, Default)]
struct Groups {
    keys: BTreeMap<GroupKey, u32>,
    metadata: Vec<Arc<GroupKey>>,
    scopes: HashMap<String, Vec<Posting>>,
}

#[derive(Clone, Copy, Debug)]
struct Posting {
    group: u32,
    target: u32,
    link: u32,
}

impl Groups {
    fn add(&mut self, scope: String, key: &GroupKey, target: usize, link: usize) {
        let group = if let Some(&group) = self.keys.get(key) {
            group
        } else {
            let group = u32::try_from(self.metadata.len()).expect("too many suggestion group types");
            self.metadata.push(Arc::new(key.clone()));
            self.keys.insert(key.clone(), group);
            group
        };
        let target = u32::try_from(target).expect("too many suggestion nodes");
        let link = u32::try_from(link).expect("too many suggestion links");
        self.scopes
            .entry(scope)
            .or_default()
            .push(Posting { group, target, link });
    }

    fn finish(self, nodes: &[IndexedNode]) -> (HashMap<String, Vec<Group>>, Vec<u32>) {
        let metadata = self.metadata;
        let mut links = Vec::new();
        let scopes = self
            .scopes
            .into_iter()
            .map(|(scope, mut postings)| {
                postings.sort_unstable_by(|a, b| {
                    metadata[a.group as usize]
                        .cmp(&metadata[b.group as usize])
                        .then(a.target.cmp(&b.target))
                        .then(a.link.cmp(&b.link))
                });
                let mut postings = postings.into_iter().peekable();
                let mut groups = Vec::new();
                while let Some(first) = postings.peek().copied() {
                    let key = metadata[first.group as usize].clone();
                    let mut targets = Vec::new();
                    while let Some(target) = postings.peek().copied().filter(|p| p.group == first.group) {
                        let start = u32::try_from(links.len()).expect("too many suggestion postings");
                        while postings
                            .peek()
                            .is_some_and(|p| p.group == first.group && p.target == target.target)
                        {
                            links.push(postings.next().unwrap().link);
                        }
                        let end = u32::try_from(links.len()).expect("too many suggestion postings");
                        targets.push(Target {
                            node: target.target,
                            start,
                            end,
                        });
                    }
                    targets.sort_by(|a, b| {
                        nodes[a.node as usize]
                            .node
                            .label
                            .cmp(&nodes[b.node as usize].node.label)
                            .then(nodes[a.node as usize].node.id.cmp(&nodes[b.node as usize].node.id))
                    });
                    targets.shrink_to_fit();
                    groups.push(Group { key, targets });
                }
                groups.shrink_to_fit();
                (scope, groups)
            })
            .collect();
        links.shrink_to_fit();
        (scopes, links)
    }
}

#[derive(Debug, Default)]
pub struct SuggestionIndex {
    nodes: Vec<IndexedNode>,
    ids: HashMap<String, usize>,
    ambiguous_aliases: BTreeSet<String>,
    links: Vec<Link>,
    scopes: HashMap<String, Vec<Group>>,
    postings: Vec<u32>,
    relations: BTreeSet<String>,
    pub sha256: String,
    pub excluded_unresolved: usize,
}

impl SuggestionIndex {
    pub fn new(atlas: &Atlas, graph: &Graph) -> Self {
        let mut index = Self::default();
        let mut aliases = Vec::new();
        for (i, d) in atlas.diseases().iter().enumerate().filter(|(_, d)| d.is_active()) {
            let idx = index.add_node(
                atlas.disease_ref(i as u32),
                "disease",
                d.synonyms.iter().map(|s| s.text.clone()).collect(),
                vec![],
                None,
            );
            for alias in &d.source_ids {
                aliases.push((alias.clone(), idx));
            }
        }
        for (i, gene) in atlas.genes().iter().enumerate() {
            let idx = index.add_node(
                atlas.node_ref(NodeKey {
                    kind: NodeKind::Gene,
                    idx: i as u32,
                }),
                "gene",
                vec![gene.symbol.clone()],
                vec![],
                None,
            );
            for alias in [Some(&gene.symbol), gene.hgnc.as_ref(), gene.ncbi_gene.as_ref()]
                .into_iter()
                .flatten()
            {
                aliases.push((alias.clone(), idx));
            }
        }
        for (i, term) in atlas.hpo.terms().iter().enumerate().filter(|(_, t)| !t.obsolete) {
            index.add_node(
                atlas.term_ref(i as u32),
                "phenotype",
                term.synonyms.iter().map(|s| s.text.clone()).collect(),
                vec![],
                None,
            );
        }
        for kind in [
            NodeKind::Study,
            NodeKind::Grant,
            NodeKind::Paper,
            NodeKind::Person,
            NodeKind::Organisation,
            NodeKind::Asset,
        ] {
            for idx in 0..graph.node_count(kind) as u32 {
                let key = NodeKey { kind, idx };
                if graph.node_withheld(key).is_some() {
                    continue;
                }
                let (class, countries, recruiting) = match kind {
                    NodeKind::Study => (
                        graph.study(idx).kind.as_str(),
                        graph.study(idx).countries.clone(),
                        Some(graph.study(idx).is_recruiting()),
                    ),
                    NodeKind::Grant => (kind.as_str(), vec![graph.grant(idx).country.clone()], None),
                    NodeKind::Organisation => (
                        graph.org(idx).kind.as_str(),
                        graph.org(idx).country.clone().into_iter().collect(),
                        None,
                    ),
                    NodeKind::Asset => (graph.asset(idx).kind.as_str(), vec![], None),
                    _ => (kind.as_str(), vec![], None),
                };
                index.add_node(graph.node_ref(key), class, vec![], countries, recruiting);
            }
        }
        // Reserve every canonical identity before resolving aliases. Ambiguous
        // aliases never replace identities or silently select one entity.
        index.add_aliases(aliases);
        let aliases = graph
            .data()
            .aliases
            .iter()
            .filter_map(|(alias, canonical)| index.ids.get(canonical).map(|&idx| (alias.clone(), idx)))
            .collect();
        index.add_aliases(aliases);
        for gene in &graph.data().gene_aliases {
            if let Some(&idx) = index.ids.get(&gene.symbol) {
                index.nodes[idx].prefixes.extend(
                    gene.aliases
                        .iter()
                        .chain(&gene.previous)
                        .chain(std::iter::once(&gene.name))
                        .map(|s| s.to_lowercase()),
                );
            }
        }
        let mut groups = Groups::default();
        for (i, edge) in graph.edges().iter().enumerate() {
            if graph.records_withheld(&edge.records).is_some() {
                continue;
            }
            index.add_link(
                &edge.from,
                edge.relation.as_str(),
                &edge.to,
                Lineage::Connected(i as u32),
                &mut groups,
            );
        }
        for d in atlas.diseases().iter().filter(|d| d.is_active()) {
            for gene in &d.genes {
                let Some(g) = atlas.gene(&gene.symbol) else {
                    continue;
                };
                index.add_link(
                    &d.id,
                    "has_associated_gene",
                    atlas.gene_at(g).id(),
                    Lineage::Atlas(vec![gene.record.clone()]),
                    &mut groups,
                );
            }
            for (relation, phenotypes) in [("has_phenotype", &d.phenotypes), ("lacks_phenotype", &d.excluded)] {
                for p in phenotypes {
                    index.add_link(
                        &d.id,
                        relation,
                        &atlas.term_ref(p.term).id,
                        Lineage::Atlas(p.annotations.iter().map(|a| a.record.clone()).collect()),
                        &mut groups,
                    );
                }
            }
        }
        (index.scopes, index.postings) = groups.finish(&index.nodes);
        let mut digest = Sha256::new();
        // Deterministic even though the lookup maps use randomised hashers.
        digest.update(b"query-suggestions-index-v1");
        for node in &index.nodes {
            digest.update(
                serde_json::to_vec(&(
                    &node.node,
                    &node.class,
                    &node.prefixes,
                    &node.countries,
                    node.recruiting,
                ))
                .unwrap(),
            );
        }
        let aliases: BTreeMap<_, _> = index.ids.iter().collect();
        digest.update(serde_json::to_vec(&aliases).unwrap());
        for link in &index.links {
            digest.update(link.id.as_bytes());
        }
        for provenance in [&atlas.provenance, graph.provenance()] {
            digest.update(serde_json::to_vec(provenance).unwrap());
        }
        for record in &graph.data().records {
            digest.update(serde_json::to_vec(record).unwrap());
        }
        if std::env::var_os("ATLAS_STARTUP_MEMORY_TRACE").is_some() {
            eprintln!(
                "suggestion-index-shape: nodes={} ids={} links={} edge_id_bytes={} scopes={} groups={} targets={} postings={}",
                index.nodes.len(),
                index.ids.len(),
                index.links.len(),
                index.links.iter().map(|l| l.id.len()).sum::<usize>(),
                index.scopes.len(),
                index.scopes.values().map(Vec::len).sum::<usize>(),
                index.scopes.values().flatten().map(|g| g.targets.len()).sum::<usize>(),
                index
                    .scopes
                    .values()
                    .flatten()
                    .flat_map(|g| &g.targets)
                    .map(|t| (t.end - t.start) as usize)
                    .sum::<usize>()
            );
        }
        index.sha256 = format!("{:x}", digest.finalize());
        index
    }

    fn add_aliases(&mut self, aliases: Vec<(String, usize)>) {
        let canonical: BTreeSet<_> = self.nodes.iter().map(|n| n.node.id.clone()).collect();
        let mut candidates: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for (alias, idx) in aliases {
            if canonical.contains(&alias) || self.ambiguous_aliases.contains(&alias) {
                continue;
            }
            if let Some(&existing) = self.ids.get(&alias) {
                candidates.entry(alias.clone()).or_default().insert(existing);
            }
            candidates.entry(alias).or_default().insert(idx);
        }
        for (alias, targets) in candidates {
            if targets.len() == 1 {
                self.ids.insert(alias, *targets.first().unwrap());
            } else {
                self.ids.remove(&alias);
                self.ambiguous_aliases.insert(alias);
            }
        }
    }

    fn add_node(
        &mut self,
        node: NodeRef,
        class: &str,
        mut aliases: Vec<String>,
        countries: Vec<String>,
        recruiting: Option<bool>,
    ) -> usize {
        if let Some(&idx) = self.ids.get(&node.id) {
            return idx;
        }
        let idx = self.nodes.len();
        aliases.extend([node.id.clone(), node.label.clone()]);
        self.ids.insert(node.id.clone(), idx);
        self.nodes.push(IndexedNode {
            node,
            class: class.into(),
            prefixes: aliases.into_iter().map(|s| s.to_lowercase()).collect(),
            countries,
            recruiting,
        });
        idx
    }

    fn add_link(&mut self, from: &str, relation: &str, to: &str, lineage: Lineage, groups: &mut Groups) {
        let (Some(&a), Some(&b)) = (self.ids.get(from), self.ids.get(to)) else {
            self.excluded_unresolved += 1;
            return;
        };
        let link = self.links.len();
        self.relations.insert(relation.into());
        self.links.push(Link {
            from: a,
            to: b,
            id: crate::node::edge_id(from, relation, to),
            lineage,
        });
        for (source, target, direction) in [(a, b, Direction::Outgoing), (b, a, Direction::Incoming)] {
            let node = &self.nodes[source];
            let dest = &self.nodes[target];
            let key = GroupKey {
                relation: relation.into(),
                direction,
                class: dest.class.clone(),
                kind: dest.node.kind,
            };
            let scopes = BTreeSet::from([
                format!("node:{source}"),
                format!("class:{}", node.class),
                format!("class:{}", node.node.kind.as_str()),
            ]);
            for scope in scopes {
                groups.add(scope, &key, target, link);
            }
        }
    }

    fn visible(&self, link: usize, allowed: &impl Fn(&str) -> bool) -> bool {
        let link = &self.links[link];
        allowed(&link.id) && allowed(&self.nodes[link.from].node.id) && allowed(&self.nodes[link.to].node.id)
    }

    fn scope(&self, request: &SuggestRequest) -> Result<String, String> {
        match (&request.node, &request.class) {
            (Some(id), None) => self
                .ids
                .get(id)
                .map(|idx| format!("node:{idx}"))
                .ok_or_else(|| "unknown node".into()),
            (None, Some(class)) => Ok(format!("class:{class}")),
            _ => Err("provide exactly one node or class".into()),
        }
    }

    pub fn suggest(
        &self,
        request: &SuggestRequest,
        atlas: &Atlas,
        graph: &Graph,
        allowed: &impl Fn(&str) -> bool,
    ) -> Result<SuggestPage, String> {
        if request.q.as_ref().is_some_and(|q| q.len() > 160) {
            return Err("filter too long".into());
        }
        let scope = self.scope(request)?;
        let q = request.q.as_deref().unwrap_or("").trim().to_lowercase();
        let mut matches = Vec::new();
        let mut work = 0usize;
        for group in self.scopes.get(&scope).into_iter().flatten() {
            if request.relation.as_ref().is_some_and(|r| r != &group.key.relation)
                || request.direction.is_some_and(|d| d != group.key.direction)
                || request
                    .target_class
                    .as_ref()
                    .is_some_and(|c| c != &group.key.class && c != group.key.kind.as_str())
            {
                continue;
            }
            let property_match = q.is_empty()
                || group.key.relation.replace('_', " ").contains(&q)
                || group.key.relation.contains(&q)
                || group.key.class.replace('_', " ").contains(&q)
                || group.key.class.contains(&q)
                || group.key.kind.as_str().contains(&q);
            let mut visible = Vec::new();
            for target in &group.targets {
                let idx = target.node as usize;
                let links = &self.postings[target.start as usize..target.end as usize];
                work += 1;
                if work > 100_000 {
                    return Err("suggestion work limit exceeded; narrow the node or class scope".into());
                }
                let target_match = q.is_empty() || self.nodes[idx].prefixes.iter().any(|p| p.starts_with(&q));
                if !target_match && (request.relation.is_some() || !property_match) {
                    continue;
                }
                for &link in links {
                    work += 1;
                    if work > 100_000 {
                        return Err("suggestion work limit exceeded; narrow the node or class scope".into());
                    }
                    if self.visible(link as usize, allowed) {
                        visible.push((idx, link as usize));
                        break;
                    }
                }
            }
            if visible.is_empty() {
                continue;
            }
            if request.relation.is_some() {
                for (idx, link) in visible {
                    matches.push((&group.key, Some(idx), 1, link));
                }
            } else {
                matches.push((&group.key, None, visible.len(), visible[0].1));
            }
        }
        if request.relation.is_none() {
            matches.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)));
        } else {
            matches.sort_by(|a, b| {
                self.nodes[a.1.unwrap()]
                    .node
                    .label
                    .cmp(&self.nodes[b.1.unwrap()].node.label)
                    .then(a.0.cmp(b.0))
                    .then(a.1.cmp(&b.1))
            });
        }
        let total = matches.len();
        let offset = request.offset.unwrap_or(0).min(total);
        let items = matches
            .into_iter()
            .skip(offset)
            .take(request.limit.unwrap_or(5).clamp(1, 5))
            .map(|(key, idx, count, link)| Suggestion {
                relation: key.relation.clone(),
                direction: key.direction,
                target_class: key.class.clone(),
                target_kind: key.kind,
                count,
                node: idx.map(|i| self.nodes[i].node.clone()),
                witness_edge: self.links[link].id.clone(),
                witness_status: match &self.links[link].lineage {
                    Lineage::Connected(idx) => Some(graph.edge(*idx).kind),
                    Lineage::Atlas(_) => None,
                },
                evidence_sample: self.citations(link, atlas, graph),
            })
            .collect();
        Ok(SuggestPage {
            version: 1,
            phase: if request.relation.is_some() {
                "targets"
            } else {
                "properties"
            },
            total,
            offset,
            items,
            index_sha256: self.sha256.clone(),
            provenance_url: "/api/query-graph/index",
        })
    }

    fn citations(&self, link: usize, atlas: &Atlas, graph: &Graph) -> Vec<Citation> {
        match &self.links[link].lineage {
            Lineage::Connected(idx) => graph
                .edge(*idx)
                .records
                .iter()
                .take(3)
                .map(|&r| {
                    let r = graph.record(r);
                    let e = graph.provenance().entity(r.entity);
                    Citation {
                        source_url: r.url.clone().unwrap_or_else(|| e.url.clone()),
                        retrieved_at: r.fetched_at.clone().or_else(|| e.retrieved_at.clone()),
                        version: e.version.clone(),
                        sha256: e.sha256.clone(),
                        record_sha256: Some(crate::graph::hex(&r.sha256)),
                        record_locator: r.locator.to_string(),
                        entity: e.id.clone(),
                    }
                })
                .collect(),
            Lineage::Atlas(records) => records
                .iter()
                .take(3)
                .map(|r| {
                    let e = atlas.provenance.entity(r.entity);
                    Citation {
                        source_url: e.url.clone(),
                        retrieved_at: e.retrieved_at.clone(),
                        version: e.version.clone(),
                        sha256: e.sha256.clone(),
                        record_sha256: None,
                        record_locator: r.locator.to_string(),
                        entity: e.id.clone(),
                    }
                })
                .collect(),
        }
    }

    pub fn preview(&self, request: &PreviewRequest, allowed: &impl Fn(&str) -> bool) -> Result<Preview, String> {
        if request.focus.is_empty()
            || request.focus.len() > 8
            || request.steps.len() > 4
            || request.bindings.len() > 4
            || request
                .bindings
                .iter()
                .any(|b| b.step == 0 || b.step > request.steps.len() || b.ids.is_empty() || b.ids.len() > 8)
            || request.country.as_ref().is_some_and(|s| s.len() > 100)
            || request.kind.as_ref().is_some_and(|s| s.len() > 100)
        {
            return Err("invalid preview bounds".into());
        }
        let mut current = BTreeSet::new();
        for id in &request.focus {
            let idx = *self.ids.get(id).ok_or("unknown focus node")?;
            if allowed(&self.nodes[idx].node.id) {
                current.insert(idx);
            }
        }
        let mut work = 0;
        for (step_index, step) in request.steps.iter().enumerate() {
            if !self.relations.contains(&step.relation) {
                return Err("unknown relation type".into());
            }
            let mut next = BTreeSet::new();
            for source in current {
                for group in self
                    .scopes
                    .get(&format!("node:{source}"))
                    .into_iter()
                    .flatten()
                    .filter(|g| g.key.relation == step.relation && g.key.direction == step.direction)
                {
                    for target in &group.targets {
                        let links = &self.postings[target.start as usize..target.end as usize];
                        work += links.len();
                        if work > 100_000 {
                            return Err("preview work limit exceeded".into());
                        }
                        if links.iter().any(|&l| self.visible(l as usize, allowed)) {
                            next.insert(target.node as usize);
                        }
                    }
                }
            }
            for binding in request.bindings.iter().filter(|b| b.step == step_index + 1) {
                let bound: BTreeSet<_> = binding
                    .ids
                    .iter()
                    .map(|id| self.ids.get(id).copied().ok_or("unknown bound node"))
                    .collect::<Result<_, _>>()?;
                next.retain(|idx| bound.contains(idx));
            }
            current = next;
        }
        let nodes: Vec<_> = current
            .into_iter()
            .filter(|&idx| {
                let node = &self.nodes[idx];
                request.output.is_none_or(|k| k == node.node.kind)
                    && request.country.as_ref().is_none_or(|c| node.countries.contains(c))
                    && request.recruiting.is_none_or(|r| node.recruiting == Some(r))
                    && request.kind.as_ref().is_none_or(|k| k == &node.class)
            })
            .collect();
        Ok(Preview {
            count: nodes.len(),
            results: nodes
                .into_iter()
                .take(100)
                .map(|idx| self.nodes[idx].node.clone())
                .collect(),
            index_sha256: self.sha256.clone(),
            provenance_url: "/api/query-graph/index",
        })
    }
}

#[cfg(test)]
mod tests;
