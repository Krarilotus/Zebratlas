use super::*;
use crate::{
    DiseaseIdentity, Provenance,
    graph::{GraphData, GraphEdge, LinkLevel, Paper, RecordHash, Relation, SourceRecord},
    node::EdgeKind,
    provenance::{ActivityIdx, Locator, SourceEntity},
};
use std::time::Instant;

fn fixture(n: usize) -> (Atlas, Graph) {
    let atlas = Atlas::new(vec![], DiseaseIdentity::default(), Provenance::default(), vec![]);
    let mut data = GraphData::default();
    let entity = data.provenance.add_entity(SourceEntity {
        id: "source:synthetic-fixture".into(),
        url: "https://example.invalid/fixture".into(),
        version: Some("synthetic-v1".into()),
        retrieved_at: Some("2026-10-04T00:00:00Z".into()),
        sha256: Some("0".repeat(64)),
        ..Default::default()
    });
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Line(1),
        id: "fixture".into(),
        url: None,
        fetched_at: None,
        hash: RecordHash::JsonLine,
        sha256: [0; 32],
    });
    for i in 0..=n {
        data.papers.push(Paper {
            id: format!("fixture:paper:{i}"),
            title: format!("Synthetic paper {i:06}"),
            journal: String::new(),
            year: None,
            doi: None,
            review: false,
            records: vec![0],
        });
    }
    for i in 1..=n {
        data.edges.push(GraphEdge {
            from: "fixture:paper:0".into(),
            relation: Relation::RelatedTo,
            to: format!("fixture:paper:{i}"),
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: "synthetic fixture".into(),
            activity: ActivityIdx(0),
            records: vec![0],
        });
    }
    (atlas, Graph::new(data))
}

#[test]
fn property_first_pagination_prefix_direction_and_lineage() {
    let (atlas, graph) = fixture(12);
    let index = SuggestionIndex::new(&atlas, &graph);
    let mut request = SuggestRequest {
        node: Some("fixture:paper:0".into()),
        ..Default::default()
    };
    let groups = index.suggest(&request, &atlas, &graph, &|_| true).unwrap();
    assert_eq!(groups.phase, "properties");
    assert_eq!(groups.items.len(), 1);
    assert_eq!(groups.items[0].count, 12);
    assert_eq!(groups.items[0].target_class, "paper");
    assert!(groups.items[0].node.is_none());
    assert_eq!(groups.items[0].witness_status, Some(EdgeKind::Observed));
    let evidence = &groups.items[0].evidence_sample[0];
    assert_eq!(evidence.version.as_deref(), Some("synthetic-v1"));
    assert_eq!(evidence.record_locator, "L1");
    assert_eq!(evidence.record_sha256.as_ref().unwrap().len(), 64);
    request.relation = Some("related_to".into());
    request.limit = Some(99);
    let targets = index.suggest(&request, &atlas, &graph, &|_| true).unwrap();
    assert_eq!(targets.total, 12);
    assert_eq!(targets.items.len(), 5);
    request.offset = Some(10);
    assert_eq!(
        index.suggest(&request, &atlas, &graph, &|_| true).unwrap().items.len(),
        2
    );
    request.offset = None;
    request.q = Some("Synthetic paper 00001".into());
    assert_eq!(index.suggest(&request, &atlas, &graph, &|_| true).unwrap().total, 3);
    request.node = Some("fixture:paper:12".into());
    request.q = None;
    request.direction = Some(Direction::Incoming);
    assert_eq!(
        index.suggest(&request, &atlas, &graph, &|_| true).unwrap().items[0]
            .node
            .as_ref()
            .unwrap()
            .id,
        "fixture:paper:0"
    );
}

#[test]
fn duplicate_edges_do_not_inflate_counts_and_dynamic_withholding_precedes_counts() {
    let (atlas, graph) = fixture(3);
    let mut data = graph.into_data();
    data.edges.push(data.edges[0].clone());
    let graph = Graph::new(data);
    let index = SuggestionIndex::new(&atlas, &graph);
    let request = SuggestRequest {
        node: Some("fixture:paper:0".into()),
        ..Default::default()
    };
    assert_eq!(
        index.suggest(&request, &atlas, &graph, &|_| true).unwrap().items[0].count,
        3
    );
    let page = index
        .suggest(&request, &atlas, &graph, &|id| id != "fixture:paper:1")
        .unwrap();
    assert_eq!(page.items[0].count, 2);
    assert!(!page.items[0].witness_edge.ends_with("fixture:paper:1"));
    let page = index
        .suggest(&request, &atlas, &graph, &|id| id != "fixture:paper:0")
        .unwrap();
    assert_eq!(page.total, 0);
}

#[test]
fn class_counts_are_unique_targets_and_filter_properties() {
    let (atlas, graph) = fixture(8);
    let index = SuggestionIndex::new(&atlas, &graph);
    let request = SuggestRequest {
        class: Some("paper".into()),
        q: Some("related".into()),
        ..Default::default()
    };
    let page = index.suggest(&request, &atlas, &graph, &|_| true).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].count, 8);
    assert_eq!(page.items[1].count, 1);
    let bad = SuggestRequest {
        node: Some("unknown".into()),
        ..Default::default()
    };
    assert!(index.suggest(&bad, &atlas, &graph, &|_| true).is_err());
}

#[test]
fn preview_is_exact_multi_hop_and_visibility_aware() {
    let (atlas, graph) = fixture(12);
    let index = SuggestionIndex::new(&atlas, &graph);
    let mut request = PreviewRequest {
        focus: vec!["fixture:paper:0".into()],
        steps: vec![Step {
            relation: "related_to".into(),
            direction: Direction::Outgoing,
        }],
        output: Some(NodeKind::Paper),
        country: None,
        recruiting: None,
        kind: None,
        bindings: vec![],
    };
    assert_eq!(index.preview(&request, &|_| true).unwrap().count, 12);
    assert_eq!(
        index.preview(&request, &|id| id != "fixture:paper:1").unwrap().count,
        11
    );
    request.steps.push(Step {
        relation: "related_to".into(),
        direction: Direction::Incoming,
    });
    assert_eq!(index.preview(&request, &|_| true).unwrap().count, 1);
    request.recruiting = Some(true);
    assert_eq!(index.preview(&request, &|_| true).unwrap().count, 0);
}

#[test]
fn concrete_target_binding_preserves_path_and_visibility() {
    let (atlas, graph) = fixture(12);
    let index = SuggestionIndex::new(&atlas, &graph);
    let mut request = PreviewRequest {
        focus: vec!["fixture:paper:0".into()],
        steps: vec![Step {
            relation: "related_to".into(),
            direction: Direction::Outgoing,
        }],
        output: Some(NodeKind::Paper),
        country: None,
        recruiting: None,
        kind: None,
        bindings: vec![Binding {
            step: 1,
            ids: vec!["fixture:paper:7".into()],
        }],
    };
    assert_eq!(index.preview(&request, &|_| true).unwrap().count, 1);
    assert_eq!(index.preview(&request, &|id| id != "fixture:paper:7").unwrap().count, 0);
    request.steps.push(Step {
        relation: "related_to".into(),
        direction: Direction::Incoming,
    });
    assert_eq!(
        index.preview(&request, &|_| true).unwrap().results[0].id,
        "fixture:paper:0"
    );
    request.bindings[0].step = 3;
    assert!(index.preview(&request, &|_| true).is_err());
}

#[test]
fn suggestion_latency_on_20k_indexed_neighbours() {
    let (atlas, graph) = fixture(20_000);
    let index = SuggestionIndex::new(&atlas, &graph);
    let request = SuggestRequest {
        class: Some("paper".into()),
        relation: Some("related_to".into()),
        direction: Some(Direction::Outgoing),
        q: Some("Synthetic paper 019".into()),
        ..Default::default()
    };
    let mut samples = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let page = index.suggest(&request, &atlas, &graph, &|_| true).unwrap();
        assert_eq!(page.total, 1_000);
        samples.push(start.elapsed());
    }
    samples.sort();
    eprintln!(
        "query-graph: 20k synthetic edges; prefix suggestions p50={:?}, p95={:?}",
        samples[10], samples[18]
    );
    // Generous regression bound for a shared development PC; report the actual p95 against 50 ms.
    assert!(samples[18].as_millis() < 500);
}

#[test]
fn canonical_ids_survive_alias_collisions_and_ambiguous_aliases_stay_excluded() {
    let (atlas, graph) = fixture(2);
    let mut index = SuggestionIndex::new(&atlas, &graph);
    let first = index.ids["fixture:paper:0"];
    let second = index.ids["fixture:paper:1"];
    index.add_aliases(vec![
        ("fixture:paper:0".into(), second),
        ("ambiguous".into(), first),
        ("ambiguous".into(), second),
    ]);
    assert_eq!(index.ids["fixture:paper:0"], first);
    assert!(!index.ids.contains_key("ambiguous"));
    index.add_aliases(vec![("ambiguous".into(), second)]);
    assert!(!index.ids.contains_key("ambiguous"));
    assert_eq!(index.nodes.len(), 3);
}

#[test]
fn target_filter_does_not_match_property_or_class_names() {
    let (atlas, graph) = fixture(3);
    let index = SuggestionIndex::new(&atlas, &graph);
    let request = SuggestRequest {
        node: Some("fixture:paper:0".into()),
        relation: Some("related_to".into()),
        q: Some("paper".into()),
        ..Default::default()
    };
    assert_eq!(index.suggest(&request, &atlas, &graph, &|_| true).unwrap().total, 0);
    let request = SuggestRequest {
        q: Some("synthetic paper".into()),
        ..request
    };
    assert_eq!(index.suggest(&request, &atlas, &graph, &|_| true).unwrap().total, 3);
}

#[test]
fn overbroad_suggestions_fail_without_partial_counts_or_sorting() {
    let (atlas, graph) = fixture(1);
    let mut index = SuggestionIndex::new(&atlas, &graph);
    // A large witness list tests bounded visibility work without duplicating a graph.
    let group = &mut index.scopes.get_mut("node:0").unwrap()[0];
    index.postings = vec![0; 100_001];
    group.targets[0].start = 0;
    group.targets[0].end = 100_001;
    let request = SuggestRequest {
        node: Some("fixture:paper:0".into()),
        ..Default::default()
    };
    let error = index.suggest(&request, &atlas, &graph, &|_| false).unwrap_err();
    assert!(error.contains("work limit exceeded"));
    assert!(error.contains("narrow"));
    // Visibility remains fresh on the next request; no error/filtered page is cached.
    assert_eq!(
        index.suggest(&request, &atlas, &graph, &|_| true).unwrap().items[0].count,
        1
    );
}

#[test]
fn compact_groups_match_legacy_counts_order_witnesses_and_visibility() {
    let (atlas, graph) = fixture(80);
    let mut data = graph.into_data();
    // Alternate relation order, duplicate witnesses, incoming edges and equal labels.
    for i in 1..=80 {
        let mut edge = data.edges[i - 1].clone();
        edge.relation = if i % 2 == 0 {
            Relation::CandidateSameAs
        } else {
            Relation::AuthorOf
        };
        std::mem::swap(&mut edge.from, &mut edge.to);
        data.edges.push(edge.clone());
        data.edges.push(edge);
        if i % 3 == 0 {
            data.papers[i].title = "Equal label".into();
        }
    }
    let graph = Graph::new(data);
    let compact = SuggestionIndex::new(&atlas, &graph);
    let mut legacy = SuggestionIndex::new(&atlas, &graph);
    let mut tree: HashMap<String, BTreeMap<GroupKey, BTreeMap<usize, Vec<usize>>>> = HashMap::new();
    for (link, edge) in graph.edges().iter().enumerate() {
        let (a, b) = (legacy.ids[&edge.from], legacy.ids[&edge.to]);
        for (source, target, direction) in [(a, b, Direction::Outgoing), (b, a, Direction::Incoming)] {
            let key = GroupKey {
                relation: edge.relation.as_str().into(),
                direction,
                class: "paper".into(),
                kind: NodeKind::Paper,
            };
            for scope in [format!("node:{source}"), "class:paper".into()] {
                tree.entry(scope)
                    .or_default()
                    .entry(key.clone())
                    .or_default()
                    .entry(target)
                    .or_default()
                    .push(link);
            }
        }
    }
    legacy.postings.clear();
    legacy.scopes = tree
        .into_iter()
        .map(|(scope, groups)| {
            let groups = groups
                .into_iter()
                .map(|(key, targets)| {
                    let mut targets: Vec<_> = targets
                        .into_iter()
                        .map(|(t, links)| {
                            let start = legacy.postings.len() as u32;
                            legacy.postings.extend(links.into_iter().map(|l| l as u32));
                            Target {
                                node: t as u32,
                                start,
                                end: legacy.postings.len() as u32,
                            }
                        })
                        .collect();
                    targets.sort_by(|a, b| {
                        legacy.nodes[a.node as usize]
                            .node
                            .label
                            .cmp(&legacy.nodes[b.node as usize].node.label)
                            .then(
                                legacy.nodes[a.node as usize]
                                    .node
                                    .id
                                    .cmp(&legacy.nodes[b.node as usize].node.id),
                            )
                    });
                    Group {
                        key: Arc::new(key),
                        targets,
                    }
                })
                .collect();
            (scope, groups)
        })
        .collect();
    for node in [None, Some("fixture:paper:0"), Some("fixture:paper:7")] {
        for relation in [None, Some("related_to"), Some("candidate_same_as"), Some("author_of")] {
            for direction in [None, Some(Direction::Outgoing), Some(Direction::Incoming)] {
                for q in [None, Some("Equal"), Some("Synthetic paper 00001")] {
                    for offset in [0, 5, 999] {
                        let request = SuggestRequest {
                            node: node.map(Into::into),
                            class: node.is_none().then(|| "paper".into()),
                            relation: relation.map(Into::into),
                            direction,
                            q: q.map(Into::into),
                            offset: Some(offset),
                            ..Default::default()
                        };
                        let allowed =
                            |id: &str| !id.contains("fixture:paper:3") && !id.contains("fixture:paper:7|author_of|");
                        assert_eq!(
                            serde_json::to_vec(&compact.suggest(&request, &atlas, &graph, &allowed).unwrap()).unwrap(),
                            serde_json::to_vec(&legacy.suggest(&request, &atlas, &graph, &allowed).unwrap()).unwrap(),
                            "{request:?}"
                        );
                    }
                }
            }
        }
    }
    let property = compact.scopes["class:paper"]
        .iter()
        .find(|g| g.key.direction == Direction::Outgoing && g.key.relation == "related_to")
        .unwrap();
    let same_property = compact.scopes["node:0"]
        .iter()
        .find(|g| g.key.direction == Direction::Outgoing && g.key.relation == "related_to")
        .unwrap();
    assert!(Arc::ptr_eq(&property.key, &same_property.key));
}
