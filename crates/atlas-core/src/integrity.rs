//! Graph contracts (shape-style) over the atlas plus the connected layer:
//! every edge endpoint resolves to a node of the kind its relation allows (no dangling edges),
//! every node and edge carries provenance (records with a sha256, a generating activity),
//! identity conflicts are listed, and nodes/edges are counted per kind.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::atlas::Atlas;
use crate::graph::{AssetKind, Coverage, Graph, LicenceClass, RecIdx, Relation};
use crate::identity::Conflict;
use crate::node::NodeKind;

const ANY: &[NodeKind] = &[
    NodeKind::Disease,
    NodeKind::Gene,
    NodeKind::Phenotype,
    NodeKind::Paper,
    NodeKind::Study,
    NodeKind::Grant,
    NodeKind::Person,
    NodeKind::Organisation,
    NodeKind::Asset,
];

/// Allowed `(from, to)` kinds per relation.
pub fn shape(relation: Relation) -> (&'static [NodeKind], &'static [NodeKind]) {
    use NodeKind::*;
    match relation {
        Relation::StudiesCondition => (&[Study], &[Disease]),
        Relation::NamesGene => (&[Study], &[Gene]),
        Relation::AboutGene => (&[Paper, Grant], &[Gene]),
        Relation::AboutCondition => (&[Paper, Grant], &[Disease]),
        Relation::AuthorOf => (&[Person], &[Paper]),
        Relation::PrincipalInvestigatorOf => (&[Person], &[Grant]),
        Relation::SponsoredBy => (&[Study], &[Organisation]),
        Relation::AwardedTo => (&[Grant], &[Organisation]),
        Relation::ServesCondition => (&[Organisation], &[Disease]),
        Relation::ServesGene => (&[Organisation], &[Gene]),
        Relation::SameAs => (&[Person], &[Person]),
        Relation::HeldBy => (&[Asset, Study, Grant], &[Organisation]),
        Relation::ModelOf => (&[Asset], &[Disease, Gene]),
        Relation::StudiedFor => (&[Asset, Study], &[Disease]),
        Relation::Targets => (&[Asset], &[Gene, Asset]),
        Relation::ResourceFor | Relation::Funds => (&[Asset, Organisation], &[Disease, Gene]),
        Relation::ClaimsAbout => (&[Paper], &[Gene, Disease, Phenotype]),
        Relation::HasPhenotype => (&[Disease, Gene, Asset], &[Phenotype]),
        Relation::OrthologousTo => (&[Gene, Asset], &[Gene, Asset]),
        Relation::GeneAssociatedWithCondition => (&[Gene, Asset], &[Disease]),
        Relation::CandidateSameAs | Relation::RelatedTo => (ANY, ANY),
    }
}

/// Kind of any node id: connected-layer node, canonical active-or-retired condition id, or gene id.
pub fn kind_of(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeKind> {
    if let Some(k) = graph.node(id) {
        return Some(k.kind);
    }
    if atlas.disease(id).is_some_and(|d| d.id == id) {
        return Some(NodeKind::Disease);
    }
    if atlas.gene(id).is_some_and(|g| atlas.gene_at(g).id() == id) {
        return Some(NodeKind::Gene);
    }
    if atlas.hpo.canonical(id).is_some() {
        return Some(NodeKind::Phenotype);
    }
    None
}

#[derive(Clone, Debug, Serialize)]
pub struct Violation {
    /// Contract that failed (`edge-endpoint-resolves`, `edge-has-provenance`, ...).
    pub contract: &'static str,
    pub subject: String,
    pub detail: String,
    pub detail_msg: serde_json::Value,
}

impl Violation {
    fn new(contract: &'static str, subject: String, detail_msg: serde_json::Value) -> Self {
        Self {
            contract,
            subject,
            detail: detail_msg["fallback"].as_str().unwrap_or_default().to_owned(),
            detail_msg,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Contract {
    pub id: &'static str,
    pub description: String,
    pub description_msg: serde_json::Value,
    pub checked: u64,
    pub violations: u64,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityConflicts {
    /// Disease ids that two curated sources map to different nodes (kept unmerged).
    pub disease: Vec<Conflict>,
    pub disease_total: usize,
    /// Person-level conflicts (two ORCIDs on one person, one ORCID on two persons, `same_as` across
    /// different ORCIDs).
    pub person: Vec<Violation>,
    pub person_total: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub passed: bool,
    pub contracts: Vec<Contract>,
    /// First violations per contract (at most 20 each).
    pub violations: Vec<Violation>,
    pub nodes: BTreeMap<&'static str, usize>,
    pub edges_by_relation: BTreeMap<&'static str, usize>,
    pub edges_by_kind: BTreeMap<&'static str, usize>,
    pub edges_by_level: BTreeMap<&'static str, usize>,
    pub identity_conflicts: IdentityConflicts,
    pub coverage: Vec<Coverage>,
    /// D37 §3: per licence class, source entities, records, nodes and edges (weakest record wins).
    pub licences: BTreeMap<&'static str, LicenceCount>,
    /// SSSOM identity: exact merges (aliases) and candidate links.
    pub identity: IdentityCounts,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LicenceCount {
    pub entities: usize,
    pub records: usize,
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct IdentityCounts {
    pub exact_merges: usize,
    pub candidates: usize,
    /// Quarantined source records (kept with provenance, never shown or released).
    pub quarantined_records: usize,
}

const SAMPLE: usize = 20;

struct Checker {
    contracts: Vec<Contract>,
    violations: Vec<Violation>,
}

impl Checker {
    fn run(
        &mut self,
        id: &'static str,
        description: serde_json::Value,
        items: impl Iterator<Item = (String, Option<serde_json::Value>)>,
    ) {
        let (mut checked, mut failed) = (0u64, 0u64);
        for (subject, problem) in items {
            checked += 1;
            if let Some(detail) = problem {
                failed += 1;
                if failed as usize <= SAMPLE {
                    self.violations.push(Violation::new(id, subject, detail));
                }
            }
        }
        self.contracts.push(Contract {
            id,
            description: description["fallback"].as_str().unwrap_or_default().to_owned(),
            description_msg: description,
            checked,
            violations: failed,
            passed: failed == 0,
        });
    }
}

fn record_problem(graph: &Graph, records: &[RecIdx]) -> Option<serde_json::Value> {
    if records.is_empty() {
        return Some(crate::copy::msg("integrity.problem.no_record", serde_json::json!({})));
    }
    for &r in records {
        let Some(rec) = graph.data().records.get(r as usize) else {
            return Some(crate::copy::msg(
                "integrity.problem.record_index",
                serde_json::json!({"r": r}),
            ));
        };
        if usize::from(rec.entity.0) >= graph.provenance().entities.len() {
            return Some(crate::copy::msg(
                "integrity.problem.record_entity",
                serde_json::json!({"arg0": rec.id}),
            ));
        }
        if rec.sha256 == [0; 32] {
            return Some(crate::copy::msg(
                "integrity.problem.record_checksum",
                serde_json::json!({"arg0": rec.id}),
            ));
        }
    }
    None
}

fn kind_cached<'a>(
    cache: &mut HashMap<&'a str, Option<NodeKind>>,
    atlas: &Atlas,
    graph: &Graph,
    id: &'a str,
) -> Option<NodeKind> {
    *cache.entry(id).or_insert_with(|| kind_of(atlas, graph, id))
}

pub fn check(atlas: &Atlas, graph: &Graph) -> Report {
    let mut c = Checker {
        contracts: Vec::new(),
        violations: Vec::new(),
    };
    let edges = graph.edges();
    let activities = graph.provenance().activities.len();
    let mut kinds: HashMap<&str, Option<NodeKind>> = HashMap::new();

    c.run(
        "source-header-checksums-match",
        crate::copy::msg("integrity.contract.source-header-checksums-match", serde_json::json!({})),
        graph
            .coverage()
            .iter()
            .filter(|source| source.status == "loaded")
            .map(|source| {
                (
                    format!("{}:{}", source.source, source.files.join(",")),
                    (source.header_checksums_failed > 0)
                        .then(|| crate::copy::msg("integrity.problem.header_checksums", serde_json::json!({"n": source.header_checksums_failed}))),
                )
            }),
    );

    c.run(
        "identity-merge-has-rule-evidence",
        crate::copy::msg(
            "integrity.contract.identity-merge-has-rule-evidence",
            serde_json::json!({}),
        ),
        graph.data().aliases.iter().map(|(alias, canonical)| {
            let problem = match graph.identity_merge(canonical) {
                None => Some(crate::copy::msg(
                    "integrity.problem.identity_activity",
                    serde_json::json!({}),
                )),
                Some(m)
                    if m.mappings.is_empty()
                        || !m.members.iter().any(|x| &x.id == alias && !x.derived_from.is_empty()) =>
                {
                    Some(crate::copy::msg(
                        "integrity.problem.identity_derivation",
                        serde_json::json!({}),
                    ))
                }
                Some(m) => m.mappings.iter().find_map(|r| {
                    if crate::identity_rules::rule(&r.rule_id, &r.rule_version).is_none()
                        || r.evidence_locator.is_empty()
                        || r.evidence_sha256.len() != 64
                        || r.evidence_url.is_empty()
                    {
                        Some(crate::copy::msg(
                            "integrity.problem.identity_rule",
                            serde_json::json!({}),
                        ))
                    } else {
                        record_problem(graph, &[r.record])
                    }
                }),
            };
            (alias.clone(), problem)
        }),
    );

    c.run(
        "edge-endpoint-resolves",
        crate::copy::msg("integrity.contract.edge-endpoint-resolves", serde_json::json!({})),
        edges.iter().map(|e| {
            let (from, to) = shape(e.relation);
            let ends = (
                kind_cached(&mut kinds, atlas, graph, &e.from),
                kind_cached(&mut kinds, atlas, graph, &e.to),
            );
            let problem = match ends {
                (None, _) => Some(crate::copy::msg(
                    "integrity.problem.endpoint_from",
                    serde_json::json!({"arg0": e.from}),
                )),
                (_, None) => Some(crate::copy::msg(
                    "integrity.problem.endpoint_to",
                    serde_json::json!({"arg0": e.to}),
                )),
                (Some(f), Some(t)) if !from.contains(&f) || !to.contains(&t) => Some(crate::copy::msg(
                    "integrity.problem.endpoint_kind",
                    serde_json::json!({"arg0": e.relation.as_str(), "from": from, "to": to, "f": f, "t": t}),
                )),
                _ => None,
            };
            (e.id(), problem)
        }),
    );
    c.run(
        "edge-has-provenance",
        crate::copy::msg("integrity.contract.edge-has-provenance", serde_json::json!({})),
        edges.iter().map(|e| {
            let problem = if usize::from(e.activity.0) >= activities {
                Some(crate::copy::msg(
                    "integrity.problem.unknown_activity",
                    serde_json::json!({}),
                ))
            } else {
                record_problem(graph, &e.records)
            };
            (e.id(), problem)
        }),
    );
    let mut node_checks: Vec<(String, Option<serde_json::Value>)> = Vec::new();
    for kind in [
        NodeKind::Study,
        NodeKind::Grant,
        NodeKind::Paper,
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Asset,
    ] {
        for idx in 0..graph.node_count(kind) as u32 {
            let key = crate::node::NodeKey { kind, idx };
            let id = graph.node_ref(key).id;
            node_checks.push((id, record_problem(graph, graph.node_records(key))));
        }
    }
    c.run(
        "node-has-provenance",
        crate::copy::msg("integrity.contract.node-has-provenance", serde_json::json!({})),
        node_checks.into_iter(),
    );
    let mut seen: HashMap<String, usize> = HashMap::new();
    for e in edges {
        *seen.entry(e.id()).or_default() += 1;
    }
    c.run(
        "edge-id-unique",
        crate::copy::msg("integrity.contract.edge-id-unique", serde_json::json!({})),
        seen.into_iter().map(|(id, n)| {
            (
                id,
                (n > 1).then(|| crate::copy::msg("integrity.problem.duplicate_edge", serde_json::json!({"n": n}))),
            )
        }),
    );
    c.run(
        "record-entity-has-checksum",
        crate::copy::msg("integrity.contract.record-entity-has-checksum", serde_json::json!({})),
        graph.provenance().entities.iter().map(|e| {
            (
                e.id.clone(),
                e.sha256
                    .is_none()
                    .then(|| crate::copy::msg("integrity.problem.no_checksum", serde_json::json!({}))),
            )
        }),
    );

    c.run(
        "record-has-licence",
        crate::copy::msg("integrity.contract.record-has-licence", serde_json::json!({})),
        graph.data().records.iter().enumerate().map(|(i, r)| {
            let problem = graph.licence_of(i as RecIdx).is_none().then(|| {
                crate::copy::msg(
                    "integrity.problem.entity_licence",
                    serde_json::json!({"arg0": r.entity.0}),
                )
            });
            (r.id.clone(), problem)
        }),
    );
    c.run(
        "asset-has-access-route",
        crate::copy::msg("integrity.contract.asset-has-access-route", serde_json::json!({})),
        graph.assets().iter().map(|a| {
            let ok = !a.access.route.is_empty() && (a.access.url.is_some() || a.holder_name.is_some());
            (
                a.id.clone(),
                (!ok).then(|| crate::copy::msg("integrity.problem.no_access", serde_json::json!({}))),
            )
        }),
    );
    c.run(
        "candidate-not-merged",
        crate::copy::msg("integrity.contract.candidate-not-merged", serde_json::json!({})),
        edges
            .iter()
            .filter(|e| e.relation == Relation::CandidateSameAs)
            .map(|e| {
                (
                    e.id(),
                    (e.from == e.to).then(|| crate::copy::msg("integrity.problem.self_link", serde_json::json!({}))),
                )
            }),
    );

    let person = person_conflicts(graph);
    let disease: Vec<Conflict> = atlas.identity.conflicts().iter().take(SAMPLE).cloned().collect();
    let identity_conflicts = IdentityConflicts {
        disease,
        disease_total: atlas.identity.conflicts().len(),
        person_total: person.len(),
        person: person.into_iter().take(SAMPLE).collect(),
    };

    let stats = atlas.stats();
    let mut nodes = BTreeMap::new();
    nodes.insert("disease", stats.diseases);
    nodes.insert("disease_retired", stats.retired_diseases);
    nodes.insert("disease_newly_described", stats.newly_described);
    nodes.insert("gene", stats.genes);
    nodes.insert("phenotype", stats.hpo_terms);
    nodes.insert("study", graph.node_count(NodeKind::Study));
    nodes.insert("grant", graph.node_count(NodeKind::Grant));
    nodes.insert("paper", graph.node_count(NodeKind::Paper));
    nodes.insert("person", graph.node_count(NodeKind::Person));
    nodes.insert("organisation", graph.node_count(NodeKind::Organisation));
    nodes.insert("asset", graph.node_count(NodeKind::Asset));
    for kind in AssetKind::ALL {
        let n = graph.assets().iter().filter(|a| a.kind == kind).count();
        if n > 0 {
            nodes.insert(asset_key(kind), n);
        }
    }
    let mut edges_by_relation = BTreeMap::new();
    let mut edges_by_kind = BTreeMap::new();
    let mut edges_by_level = BTreeMap::new();
    edges_by_relation.insert("has_phenotype (atlas)", stats.phenotype_edges);
    edges_by_relation.insert("lacks_phenotype (atlas)", stats.excluded_phenotype_edges);
    for e in edges {
        *edges_by_relation.entry(e.relation.as_str()).or_default() += 1;
        *edges_by_kind.entry(e.kind.as_str()).or_default() += 1;
        *edges_by_level.entry(e.level.as_str()).or_default() += 1;
    }
    let licences = licence_counts(graph);
    let identity = IdentityCounts {
        exact_merges: graph.data().aliases.len(),
        candidates: edges.iter().filter(|e| e.relation == Relation::CandidateSameAs).count(),
        quarantined_records: graph.data().quarantine.len(),
    };
    Report {
        licences,
        identity,
        passed: c.contracts.iter().all(|x| x.passed),
        contracts: c.contracts,
        violations: c.violations,
        nodes,
        edges_by_relation,
        edges_by_kind,
        edges_by_level,
        identity_conflicts,
        coverage: graph.coverage().to_vec(),
    }
}

fn asset_key(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Model => "asset:model",
        AssetKind::CellLine => "asset:cell_line",
        AssetKind::Biobank => "asset:biobank",
        AssetKind::Registry => "asset:registry",
        AssetKind::Dataset => "asset:dataset",
        AssetKind::Programme => "asset:programme",
        AssetKind::Designation => "asset:designation",
        AssetKind::Drug => "asset:drug",
        AssetKind::OutcomeMeasure => "asset:outcome_measure",
        AssetKind::FundingCall => "asset:funding_call",
        AssetKind::OrthologGene => "asset:ortholog_gene",
        AssetKind::Other => "asset:other",
    }
}

fn licence_counts(graph: &Graph) -> BTreeMap<&'static str, LicenceCount> {
    let mut out: BTreeMap<&'static str, LicenceCount> = LicenceClass::ALL
        .iter()
        .map(|c| (c.as_str(), LicenceCount::default()))
        .collect();
    for l in &graph.data().licences {
        out.entry(l.class.as_str()).or_default().entities += 1;
    }
    for i in 0..graph.data().records.len() {
        let class = graph.licence_of(i as RecIdx).map_or(LicenceClass::Unknown, |l| l.class);
        out.entry(class.as_str()).or_default().records += 1;
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
            let class = graph.licence_class(graph.node_records(crate::node::NodeKey { kind, idx }));
            out.entry(class.as_str()).or_default().nodes += 1;
        }
    }
    for e in graph.edges() {
        out.entry(graph.licence_class(&e.records).as_str()).or_default().edges += 1;
    }
    out
}

/// People whose identity the data contradicts. They are reported, never silently merged.
fn person_conflicts(graph: &Graph) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut by_orcid: HashMap<&str, Vec<&str>> = HashMap::new();
    for p in &graph.data().people {
        if p.orcids.len() > 1 {
            out.push(Violation::new(
                "person-one-orcid",
                p.id.clone(),
                crate::copy::msg(
                    "integrity.problem.person_orcids",
                    serde_json::json!({"arg0": p.orcids.len(), "arg1": p.orcids.join(", ")}),
                ),
            ));
        }
        for o in &p.orcids {
            by_orcid.entry(o).or_default().push(&p.id);
        }
    }
    let mut shared: Vec<_> = by_orcid.into_iter().filter(|(_, v)| v.len() > 1).collect();
    shared.sort();
    for (orcid, ids) in shared {
        out.push(Violation::new(
            "orcid-one-person",
            orcid.to_owned(),
            crate::copy::msg(
                "integrity.problem.orcid_people",
                serde_json::json!({"arg0": ids.len(), "arg1": ids.join(", ")}),
            ),
        ));
    }
    for e in graph.edges().iter().filter(|e| e.relation == Relation::SameAs) {
        let orcids = |id: &str| {
            graph
                .node(id)
                .map(|k| graph.person(k.idx).orcids.clone())
                .unwrap_or_default()
        };
        let (a, b) = (orcids(&e.from), orcids(&e.to));
        if !a.is_empty() && !b.is_empty() && !a.iter().any(|o| b.contains(o)) {
            out.push(Violation::new(
                "same-as-orcid-compatible",
                e.id(),
                crate::copy::msg(
                    "integrity.problem.different_orcids",
                    serde_json::json!({"a": a, "b": b}),
                ),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphData;

    #[test]
    fn loaded_header_checksum_failure_fails_integrity_without_hiding_coverage() {
        let atlas = Atlas::new(vec![], Default::default(), Default::default(), vec![]);
        let mut data = GraphData::default();
        data.coverage.push(Coverage {
            source: "models".into(),
            status: "loaded".into(),
            files: vec!["cache/models/fixture.json".into()],
            header_checksums_failed: 1,
            ..Default::default()
        });
        let report = check(&atlas, &Graph::new(data.clone()));
        assert!(!report.passed);
        let contract = report
            .contracts
            .iter()
            .find(|c| c.id == "source-header-checksums-match")
            .unwrap();
        assert_eq!((contract.checked, contract.violations), (1, 1));
        assert_eq!(report.coverage[0].header_checksums_failed, 1);
        data.coverage[0].header_checksums_failed = 0;
        assert!(check(&atlas, &Graph::new(data)).passed);
    }
}
