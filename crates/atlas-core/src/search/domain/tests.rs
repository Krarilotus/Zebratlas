//! Synthetic fixtures exercise retrieval rules; none of these associations are biological claims.
use super::*;
use crate::disease::{Disease, PhenotypeEdge};
use crate::evidence::GeneLink;
use crate::graph::{
    GeneAlias, GraphData, GraphEdge, LinkLevel, OrgKind, Organisation, RecordHash, SourceRecord, WikiItem, WikiName,
};
use crate::provenance::{ActivityIdx, EntityIdx, Locator, Provenance, RecordRef, SourceEntity};
use crate::{DiseaseIdentity, Term};

#[test]
fn lexical_top_k_preserves_full_ranking_and_evidence_with_many_word_candidates() {
    let (atlas, graph) = fixture();
    let mut data = graph.data().clone();
    let template = data.orgs[0].clone();
    for i in 0..240 {
        let mut org = template.clone();
        org.id = format!("TEST:word-org-{i:03}");
        org.name = format!("Shared candidate title {i:03}");
        data.orgs.push(org);
    }
    let graph = Graph::new(data);
    let index = Index::build(&atlas, &graph);
    for limit in [1, 20, 100, 500] {
        for query in [
            "shared candidates",
            "shared title",
            "please find shared candidate",
            "Fixture gene shared title",
        ] {
            let opts = SearchOptions {
                limit,
                include_retired: false,
            };
            let full = index.retrieve(&atlas, &graph, query, opts, true);
            let selected = index.lexical_context(&atlas, &graph, query, opts);
            assert!(full.present.is_empty());
            assert_eq!(
                serde_json::to_value(&full.hits).unwrap(),
                serde_json::to_value(&selected.hits).unwrap(),
                "{query}, K={limit}"
            );
        }
    }
}

fn fixture() -> (Atlas, Graph) {
    let mut root = Term::new("HP:0000118");
    root.name = "Phenotypic abnormality".into();
    let mut seizure = Term::new("HP:0001250");
    seizure.name = "Seizure".into();
    seizure.parents.push(root.id.clone());
    let mut focal = Term::new("HP:0007359");
    focal.name = "Focal seizure".into();
    focal.parents.push(seizure.id.clone());
    let mut ataxia = Term::new("HP:0001251");
    ataxia.name = "Ataxia".into();
    ataxia.parents.push(root.id.clone());
    let mut a = Disease::new("TEST:condition-a", ActivityIdx(0));
    a.name = "Fixture condition A".into();
    a.add_name("Fixture ambiguous", None);
    a.phenotypes = vec![PhenotypeEdge {
        term: 2,
        annotations: vec![],
    }];
    a.genes.push(GeneLink {
        symbol: "TESTGENE".into(),
        association: "MENDELIAN".into(),
        source: "fixture".into(),
        source_disease: a.id.clone(),
        pmids: vec![],
        assessed: None,
        hgnc: Some("TEST:gene".into()),
        ncbi_gene: None,
        record: RecordRef::line(EntityIdx(0), 1),
    });
    a.derived_from.push(RecordRef::line(EntityIdx(0), 1));
    let mut b = Disease::new("TEST:condition-b", ActivityIdx(0));
    b.name = "Fixture condition B".into();
    b.add_name("Fixture ambiguous", None);
    b.phenotypes = vec![PhenotypeEdge {
        term: 3,
        annotations: vec![],
    }];
    b.genes.push(GeneLink {
        association: "UNKNOWN".into(),
        source_disease: b.id.clone(),
        ..a.genes[0].clone()
    });
    let mut c = Disease::new("TEST:parent", ActivityIdx(0));
    c.name = "Fixture parent".into();
    a.parents.push(c.id.clone());
    let mut prov = Provenance::default();
    prov.add_entity(SourceEntity {
        id: "source:fixture".into(),
        file: "fixture.tsv".into(),
        url: "https://example.org/fixture".into(),
        version: Some("test-v1".into()),
        retrieved_at: Some("2026-10-04T00:00:00Z".into()),
        sha256: Some("00".repeat(32)),
        ..SourceEntity::default()
    });
    let atlas = Atlas::new(
        vec![root, seizure, focal, ataxia],
        DiseaseIdentity::default(),
        prov.clone(),
        vec![a, b, c],
    );
    let data = GraphData {
        provenance: prov,
        records: vec![SourceRecord {
            entity: EntityIdx(0),
            locator: Locator::Line(1),
            id: "fixture:record".into(),
            url: None,
            fetched_at: Some("2026-10-04T00:00:00Z".into()),
            hash: RecordHash::TsvLine,
            sha256: [0; 32],
        }],
        gene_aliases: vec![GeneAlias {
            hgnc: "TEST:gene".into(),
            symbol: "TESTGENE".into(),
            name: "Fixture gene".into(),
            aliases: vec!["FIXTURE_ALIAS".into()],
            previous: vec!["FIXTURE_OLD".into()],
            record: 0,
        }],
        wiki_items: vec![WikiItem {
            qid: "TEST:wiki".into(),
            targets: vec!["TEST:condition-a".into(), "TEST:condition-b".into()],
            record: 0,
        }],
        wiki_names: vec![WikiName {
            text: "Fixture traduction".into(),
            lang: "fr".into(),
            alias: true,
            item: 0,
        }],
        orgs: vec![Organisation {
            id: "TEST:group".into(),
            name: "Fixture support group".into(),
            kind: OrgKind::PatientGroup,
            url: None,
            contact_url: None,
            country: None,
            country_basis: None,
            description: None,
            languages: vec![],
            verified_on: None,
            channels: vec![],
            records: vec![0],
        }],
        edges: vec![GraphEdge {
            from: "TEST:group".into(),
            relation: Relation::ServesGene,
            to: "TEST:gene".into(),
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: "Fixture link".into(),
            activity: ActivityIdx(0),
            records: vec![0],
        }],
        aliases: vec![("TEST:external-group".into(), "TEST:group".into())],
        ..GraphData::default()
    };
    (atlas, Graph::new(data))
}

#[test]
fn spelling_symbols_aliases_and_layout_variants_are_real_choices() {
    let (atlas, graph) = fixture();
    let index = Index::build(&atlas, &graph);
    for query in ["TEST GENE", "test-gene", "TESTGNEE", "TESRGENE", "FIXTURE_OL"] {
        let hits = index.suggest(
            &atlas,
            &graph,
            query,
            SearchOptions {
                limit: 3,
                include_retired: false,
            },
        );
        assert!(hits.iter().any(|h| h.node.id == "TEST:gene"), "{query}: {hits:?}");
        if matches!(query, "TESTGNEE" | "TESRGENE") {
            assert!(!hits.iter().any(|h| h.strong));
        }
    }
    let hits = index.search(&atlas, &graph, "TEST GENE", SearchOptions::default());
    assert!(hits.iter().any(|h| h.reason == Reason::CausalGene));
}

#[test]
fn first_character_suggestions_do_not_expand_the_graph_or_dead_end() {
    let (atlas, graph) = fixture();
    let index = Index::build(&atlas, &graph);
    let hits = index.suggest(
        &atlas,
        &graph,
        "f",
        SearchOptions {
            limit: 5,
            include_retired: false,
        },
    );
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.reason == Reason::Prefix));
    let hits = index.suggest(
        &atlas,
        &graph,
        "zzzzzzz",
        SearchOptions {
            limit: 3,
            include_retired: false,
        },
    );
    assert_eq!(hits.len(), 3);
    assert!(
        hits.iter()
            .all(|h| node_key(&atlas, &graph, &h.node.id).is_some() && !h.strong)
    );
    for query in ["z", &"z".repeat(160)] {
        let hits = index.suggest(
            &atlas,
            &graph,
            query,
            SearchOptions {
                limit: 3,
                include_retired: false,
            },
        );
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|h| !h.strong));
    }
}

#[test]
fn phonetic_name_candidates_are_never_identity() {
    let (atlas, graph) = fixture();
    let (terms, identity, provenance, diseases) = atlas.parts();
    let mut diseases = diseases.to_vec();
    diseases[0].name = "Smith syndrome".into();
    let atlas = Atlas::new(terms.to_vec(), identity.clone(), provenance.clone(), diseases);
    let index = Index::build(&atlas, &graph);
    let hits = index.suggest(
        &atlas,
        &graph,
        "Smythe syndrom",
        SearchOptions {
            limit: 3,
            include_retired: false,
        },
    );
    let hit = hits.iter().find(|h| h.node.id == "TEST:condition-a").unwrap();
    assert!(!hit.strong);
    assert_eq!(hit.match_kind, "phonetic");
}

#[test]
fn previous_symbols_and_gene_links_have_plain_reasons() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    let hits = idx.search(&atlas, &graph, "FIXTURE_OLD", SearchOptions::default());
    assert_eq!(hits[0].reason, Reason::PreviousSymbol);
    assert_eq!(hits[0].why, "Old gene symbol");
    assert!(
        hits.iter()
            .any(|h| h.node.id == "TEST:condition-a" && h.reason == Reason::CausalGene)
    );
    assert!(
        !hits
            .iter()
            .any(|h| h.node.id == "TEST:condition-b" && h.reason == Reason::CausalGene)
    );
    let group = hits.iter().find(|h| h.node.id == "TEST:group").unwrap();
    assert_eq!(group.reason, Reason::Neighbour);
    assert_eq!(group.evidence.edges.len(), 1);
    assert!(
        group
            .evidence
            .sources
            .iter()
            .all(|s| s.version.as_deref() == Some("test-v1") && s.sha256.is_some())
    );
}

#[test]
fn ambiguous_names_and_translations_keep_all_targets() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    for query in ["Fixture ambiguous", "Fixture traduction"] {
        let hits = idx.search(&atlas, &graph, query, SearchOptions::default());
        assert_eq!(
            hits.iter()
                .filter(|h| h.reason == Reason::Synonym || h.reason == Reason::TranslatedAlias)
                .count(),
            2
        );
    }
    let hits = idx.search(&atlas, &graph, "Fixture traduction", SearchOptions::default());
    assert!(
        hits.iter()
            .filter(|h| h.reason == Reason::TranslatedAlias)
            .all(|h| !h.strong)
    );
}

#[test]
fn exact_mapping_and_organisation_are_globally_searchable() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    let hits = idx.search(&atlas, &graph, "TEST:external-group", SearchOptions::default());
    assert_eq!(hits[0].node.id, "TEST:group");
    assert_eq!(hits[0].reason, Reason::ExactId);
    assert!(
        hits[0]
            .evidence
            .activities
            .contains(&crate::graph::activity::IDENTITY_SSSOM.to_owned())
    );
    assert_eq!(
        idx.search(&atlas, &graph, "Fixture support group", SearchOptions::default())[0]
            .node
            .kind,
        NodeKind::Organisation
    );
}

#[test]
fn sentence_mentions_respect_boundaries() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    let hits = idx.search(
        &atlas,
        &graph,
        "Meine Tochter hat eine TESTGENE-Mutation",
        SearchOptions::default(),
    );
    assert_eq!(hits[0].node.kind, NodeKind::Gene);
    assert_eq!(hits[0].reason, Reason::Mention);
    assert!(!hits[0].strong);
    assert!(
        !idx.search(&atlas, &graph, "NOTTESTGENEX", SearchOptions::default())
            .iter()
            .any(|h| h.reason == Reason::Mention)
    );
}

#[test]
fn hierarchy_weighted_symptoms_and_negation() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    let hits = idx.phenotypes(&atlas, &graph, &[1], &[], 10);
    assert_eq!(hits[0].node.id, "TEST:condition-a");
    assert_eq!(hits[0].why, "Shares 1 of your 1 symptoms");
    assert!(hits.iter().all(|h| !h.strong));
    let hits = idx.search(&atlas, &graph, "No seizure and ataxia", SearchOptions::default());
    let b = hits.iter().find(|h| h.node.id == "TEST:condition-b").unwrap();
    assert_eq!(b.why, "Shares 1 of your 1 symptoms");
    assert!(
        !hits
            .iter()
            .any(|h| h.node.id == "TEST:condition-a" && h.reason == Reason::Phenotypes)
    );
    let hits = idx.search(&atlas, &graph, "Focal seizure", SearchOptions::default());
    assert_eq!(
        hits.iter().find(|h| h.node.id == "TEST:condition-a").unwrap().why,
        "Shares 1 of your 1 symptoms"
    );
}

#[test]
fn withholding_applies_to_aliases_and_neighbour_evidence() {
    let (atlas, mut graph) = fixture();
    // Attach after indexing to prove filtering is not just a construction-time check.
    let idx = Index::build(&atlas, &graph);
    let filter = crate::withhold::Withhold::from_lists(
        crate::withhold::Salt::new("fixture-salt"),
        None,
        Some(br#"{"schema":"quarantine","version":1,"entries":[{"file":"fixture.tsv","reason":"fixture withheld"}]}"#),
    )
    .unwrap();
    graph.set_withhold(filter);
    let hits = idx.search(&atlas, &graph, "FIXTURE_OLD", SearchOptions::default());
    assert!(!hits.iter().any(|h| h.reason == Reason::PreviousSymbol));
    let hits = idx.search(&atlas, &graph, "TESTGENE", SearchOptions::default());
    assert!(!hits.iter().any(|h| h.node.kind == NodeKind::Organisation));
}

#[test]
fn zero_limit_and_stable_ties() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    assert!(
        idx.search(
            &atlas,
            &graph,
            "Fixture",
            SearchOptions {
                limit: 0,
                ..SearchOptions::default()
            }
        )
        .is_empty()
    );
    let first = idx.search(&atlas, &graph, "Fixture ambiguous", SearchOptions::default());
    for _ in 0..5 {
        assert_eq!(
            first.iter().map(|h| &h.node.id).collect::<Vec<_>>(),
            idx.search(&atlas, &graph, "Fixture ambiguous", SearchOptions::default())
                .iter()
                .map(|h| &h.node.id)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn analytics_signal_orders_symptom_candidates_below_identity() {
    let (atlas, graph) = fixture();
    let idx = Index::build(&atlas, &graph);
    let query = idx.lexical_context(&atlas, &graph, "Seizure and ataxia", SearchOptions::default());
    let hits = idx.with_phenotypes(
        &atlas,
        &graph,
        query,
        vec![
            PhenotypeCandidate {
                disease: 1,
                midrank: 1.0,
            },
            PhenotypeCandidate {
                disease: 0,
                midrank: 2.0,
            },
        ],
        SearchOptions::default(),
    );
    let diseases: Vec<_> = hits.iter().filter(|h| h.key.kind == NodeKind::Disease).collect();
    assert_eq!(diseases[0].node.id, "TEST:condition-b");
    assert!(diseases.iter().all(|h| !h.strong));
}

#[test]
fn researchers_are_reached_by_two_inspectable_edges() {
    let (atlas, graph) = fixture();
    let mut data = graph.into_data();
    data.papers.push(crate::graph::Paper {
        id: "TEST:paper".into(),
        title: "Fixture research".into(),
        journal: "Fixture".into(),
        year: None,
        doi: None,
        review: false,
        records: vec![0],
    });
    data.people.push(crate::graph::Person {
        id: "TEST:person".into(),
        name: "Fixture researcher".into(),
        name_variants: vec![],
        orcids: vec![],
        affiliations: vec![],
        source: crate::graph::PersonSource::Resolved,
        genes: vec![],
        communities: vec![],
        cross_community: false,
        matched_by: vec![],
        merge_basis: vec![],
        records: vec![0],
    });
    let edge = data.edges[0].clone();
    data.edges.push(GraphEdge {
        from: "TEST:paper".into(),
        to: "TEST:gene".into(),
        relation: Relation::AboutGene,
        ..edge.clone()
    });
    data.edges.push(GraphEdge {
        from: "TEST:person".into(),
        to: "TEST:paper".into(),
        relation: Relation::AuthorOf,
        ..edge
    });
    let graph = Graph::new(data);
    let idx = Index::build(&atlas, &graph);
    let hits = idx.search(&atlas, &graph, "TESTGENE", SearchOptions::default());
    let researcher = hits.iter().find(|h| h.node.id == "TEST:person").unwrap();
    assert_eq!(researcher.evidence.edges.len(), 2);
    assert!(researcher.evidence.edges.iter().all(|e| graph.edge_by_id(e).is_some()));
}
