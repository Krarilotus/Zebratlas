//! Equivalent full-scan and selective-index execution of a fixed combined graph plan.
//! This is a query-engine benchmark, not a natural-language understanding benchmark.
use std::collections::{BTreeSet, HashSet};
use std::hint::black_box;
use std::time::Instant;

use atlas_core::Graph;
use atlas_core::graph::{OrgKind, RecordWithhold, Relation};
use atlas_core::node::{NodeKey, NodeKind};
use serde_json::json;

fn study(graph: &Graph, i: u32) -> bool {
    let s = graph.study(i);
    s.status == "RECRUITING"
        && s.countries.iter().any(|c| c == "Germany")
        && graph
            .node_withheld(NodeKey {
                kind: NodeKind::Study,
                idx: i,
            })
            .is_none()
}

fn group(graph: &Graph, from: &str, records: &[u32]) -> bool {
    graph.node(from).is_some_and(|key| {
        key.kind == NodeKind::Organisation
            && graph.org(key.idx).kind == OrgKind::PatientGroup
            && graph.node_withheld(key).is_none()
            && graph.records_withheld(records).is_none()
    })
}

fn scan(graph: &Graph, pathways: &HashSet<String>) -> BTreeSet<(String, String)> {
    let studies: HashSet<_> = (0..graph.node_count(NodeKind::Study) as u32)
        .filter(|&i| study(graph, i))
        .map(|i| graph.study(i).id.as_str())
        .collect();
    let conditions: HashSet<_> = graph
        .edges()
        .iter()
        .filter(|e| {
            e.relation == Relation::StudiesCondition
                && studies.contains(e.from.as_str())
                && pathways.contains(&e.to)
                && graph.records_withheld(&e.records).is_none()
        })
        .map(|e| e.to.as_str())
        .collect();
    graph
        .edges()
        .iter()
        .filter(|e| {
            e.relation == Relation::ServesCondition
                && conditions.contains(e.to.as_str())
                && group(graph, &e.from, &e.records)
        })
        .map(|e| (e.from.clone(), e.id()))
        .collect()
}

fn indexed(graph: &Graph, pathways: &HashSet<String>) -> BTreeSet<(String, String)> {
    let conditions: HashSet<_> = graph
        .query()
        .studies(Some("Germany"), Some("RECRUITING"))
        .filter(|&i| study(graph, i))
        .flat_map(|i| graph.incident(&graph.study(i).id))
        .filter(|inc| {
            inc.outgoing
                && inc.edge.relation == Relation::StudiesCondition
                && pathways.contains(inc.other)
                && graph.records_withheld(&inc.edge.records).is_none()
        })
        .map(|inc| inc.other.to_owned())
        .collect();
    conditions
        .iter()
        .flat_map(|id| graph.incident(id))
        .filter(|inc| {
            !inc.outgoing
                && inc.edge.relation == Relation::ServesCondition
                && group(graph, inc.other, &inc.edge.records)
        })
        .map(|inc| (inc.other.to_owned(), inc.edge.id()))
        .collect()
}

fn timings(mut run: impl FnMut() -> BTreeSet<(String, String)>) -> serde_json::Value {
    let mut samples = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        black_box(run());
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    json!({ "median_ms": (samples[49] + samples[50]) / 2.0, "p95_ms": samples[94], "samples_ms": samples })
}

fn main() -> anyhow::Result<()> {
    let data = atlas_ingest::data_dir();
    let (atlas, _) = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))?;
    let path = std::env::var_os("RARE_ATLAS_GRAPH_SNAPSHOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| data.join("cache/perf/graph.snapshot"));
    let (mut graph, _) = atlas_core::snapshot::load_graph(&path)?;
    graph.set_withhold(atlas_ingest::withhold::load(
        &data,
        atlas_ingest::withhold::salt_from_env(),
    )?);
    let (mechanism, _) = atlas_ingest::mechanism::load(&atlas_ingest::mechanism::snapshot_path(&data))?;
    let seed = mechanism
        .hgnc
        .get("STXBP1")
        .and_then(|g| g.entrez.as_deref())
        .expect("snapshot seed");
    let processes = mechanism.reactome.by_gene.get(seed).expect("seed Reactome annotations");
    // Explicit semantics: share at least one directly annotated Reactome pathway with STXBP1,
    // through an active condition's causal gene. No broad-ancestor or similarity inference.
    let started = Instant::now();
    let conditions: HashSet<_> = atlas
        .diseases()
        .iter()
        .filter(|d| {
            d.is_active()
                && d.genes.iter().filter(|g| g.is_causal()).any(|g| {
                    mechanism
                        .hgnc
                        .get(&g.symbol)
                        .and_then(|g| g.entrez.as_deref())
                        .and_then(|g| mechanism.reactome.by_gene.get(g))
                        .is_some_and(|p| p.iter().any(|p| processes.contains(p)))
                })
        })
        .map(|d| d.id.clone())
        .collect();
    let pathway_index_ms = started.elapsed().as_secs_f64() * 1000.0;
    let before = scan(&graph, &conditions);
    let after = indexed(&graph, &conditions);
    assert_eq!(
        before, after,
        "selectivity must not change the answer or its evidence ids"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "@type": "prov:Activity", "schema": "atlas.perf.combined", "version": 1,
            "query": "patient groups with a recruiting study in Germany for conditions sharing a pathway with STXBP1",
            "semantics": "RECRUITING; country Germany; direct Reactome pathway; causal gene; ServesCondition edge",
            "record_locator": "whole-run", "prov:used": [path.to_string_lossy(), atlas_ingest::snapshot_path(&data).to_string_lossy(), atlas_ingest::mechanism::snapshot_path(&data).to_string_lossy()],
            "conditions_with_shared_pathways": conditions.len(), "pathway_index_ms": pathway_index_ms,
            "graph_edges": graph.edges().len(), "matching_group_edges": before.len(),
            "scan": timings(|| scan(&graph, &conditions)), "indexed": timings(|| indexed(&graph, &conditions)),
            "limitations": "Fixed typed plan, not the unfinished NL/SPARQL route; a shared pathway is a research lead, not established therapeutic compatibility."
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, Organisation, Study, StudyKind};
    use atlas_core::node::EdgeKind;
    use atlas_core::provenance::ActivityIdx;

    #[test]
    fn combined_join_has_a_nonempty_control_and_keeps_evidence_ids() {
        let mut studies = Vec::new();
        for (id, status, country) in [
            ("fixture:eligible", "RECRUITING", "Germany"),
            ("fixture:closed", "COMPLETED", "Germany"),
            ("fixture:elsewhere", "RECRUITING", "France"),
        ] {
            studies.push(Study {
                id: id.into(),
                title: "Synthetic study".into(),
                status: status.into(),
                kind: StudyKind::Trial,
                phases: vec![],
                sponsor: String::new(),
                sponsor_class: String::new(),
                start: String::new(),
                completion: String::new(),
                enrollment: None,
                countries: vec![country.into()],
                interventions: vec![],
                record: 0,
            });
        }
        let org = Organisation {
            id: "fixture:group".into(),
            name: "Synthetic patient group".into(),
            kind: OrgKind::PatientGroup,
            url: None,
            contact_url: None,
            country: None,
            country_basis: None,
            description: None,
            languages: vec![],
            verified_on: None,
            channels: vec![],
            records: vec![],
        };
        let mut edges = Vec::new();
        for (from, relation, to) in [
            ("fixture:eligible", Relation::StudiesCondition, "fixture:condition"),
            ("fixture:closed", Relation::StudiesCondition, "fixture:closed-condition"),
            (
                "fixture:elsewhere",
                Relation::StudiesCondition,
                "fixture:other-condition",
            ),
            ("fixture:group", Relation::ServesCondition, "fixture:condition"),
            ("fixture:group", Relation::ServesCondition, "fixture:closed-condition"),
            ("fixture:group", Relation::ServesCondition, "fixture:other-condition"),
        ] {
            edges.push(GraphEdge {
                from: from.into(),
                relation,
                to: to.into(),
                kind: EdgeKind::Observed,
                level: LinkLevel::Curated,
                reason: "Synthetic fixture".into(),
                activity: ActivityIdx(0),
                records: vec![],
            });
        }
        let expected = BTreeSet::from([(org.id.clone(), edges[3].id())]);
        let graph = Graph::new(GraphData {
            studies,
            orgs: vec![org],
            edges,
            ..Default::default()
        });
        let pathways = [
            "fixture:condition",
            "fixture:closed-condition",
            "fixture:other-condition",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(scan(&graph, &pathways), expected);
        assert_eq!(indexed(&graph, &pathways), expected);
        let unrelated = ["fixture:unrelated".to_owned()].into();
        assert!(scan(&graph, &unrelated).is_empty());
        assert!(indexed(&graph, &unrelated).is_empty());
    }
}
