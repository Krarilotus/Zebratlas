use super::*;
use crate::evidence::GeneLink;
use crate::graph::{GraphData, LinkLevel, Organisation, RecordHash, Relation, SourceRecord};
use crate::provenance::{ActivityIdx, EntityIdx, Locator, Provenance};
use crate::{Disease, DiseaseIdentity};

fn fixture() -> (Atlas, Graph, PathwayData) {
    let source = SourceEntity {
        id: "source:fixture.tsv".into(),
        file: "fixture.tsv".into(),
        url: "https://example.org/fixture.tsv".into(),
        version: Some("fixture-v1".into()),
        sha256: Some("a".repeat(64)),
        retrieved_at: Some("2026-01-01T00:00:00Z".into()),
        ..SourceEntity::default()
    };
    let provenance = Provenance {
        entities: vec![source.clone()],
        activities: vec![Activity {
            id: "activity:ingest-fixture".into(),
            used: vec![EntityIdx(0)],
            ..Activity::default()
        }],
    };
    let mut diseases = Vec::new();
    for (n, symbol) in [(1, "PLACEHOLDER_A"), (2, "PLACEHOLDER_B")] {
        let mut d = Disease::new(format!("MONDO:PLACEHOLDER_{n}"), ActivityIdx(0));
        d.name = format!("Placeholder condition {n}");
        d.derived_from.push(RecordRef::line(EntityIdx(0), n));
        d.genes.push(GeneLink {
            symbol: symbol.into(),
            association: "MENDELIAN".into(),
            source: "fixture".into(),
            source_disease: d.id.clone(),
            pmids: Vec::new(),
            assessed: None,
            hgnc: Some(format!("HGNC:PLACEHOLDER_{n}")),
            ncbi_gene: Some(format!("NCBIGene:{n}")),
            record: RecordRef::line(EntityIdx(0), n),
        });
        diseases.push(d);
    }
    let atlas = Atlas::new(Vec::new(), DiseaseIdentity::default(), provenance.clone(), diseases);
    let org: Organisation = serde_json::from_value(serde_json::json!({
        "id":"org:placeholder", "name":"Placeholder group", "kind":"patient_group", "url":"https://example.org",
        "contact_url":"https://example.org/contact", "country":null, "country_basis":null, "description":null,
        "languages":["en"], "verified_on":null, "channels":[], "records":[0]
    }))
    .unwrap();
    let graph = Graph::new(GraphData {
        provenance,
        records: vec![SourceRecord {
            entity: EntityIdx(0),
            locator: Locator::Line(3),
            id: "fixture:org".into(),
            url: Some("https://example.org".into()),
            fetched_at: source.retrieved_at.clone(),
            hash: RecordHash::TsvLine,
            sha256: [1; 32],
        }],
        orgs: vec![org],
        edges: vec![GraphEdge {
            from: "org:placeholder".into(),
            to: "MONDO:PLACEHOLDER_1".into(),
            relation: Relation::ServesCondition,
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: "placeholder fixture assertion".into(),
            activity: ActivityIdx(0),
            records: vec![0],
        }],
        ..GraphData::default()
    });
    let annotations = (1..=2)
        .map(|n| PathwayAnnotation {
            gene: format!("HGNC:PLACEHOLDER_{n}"),
            pathway: NodeRef {
                id: "R-HSA-PLACEHOLDER".into(),
                label: "Placeholder process".into(),
                kind: NodeKind::Pathway,
            },
            evidence: UnitEvidence {
                source: source.clone(),
                locator: format!("L{}", n + 3),
                record_url: Some("https://example.org/process".into()),
                retrieved_at: source.retrieved_at.clone(),
                record_sha256: Some(format!("{n:064x}")),
                upstream_activity: Some("activity:ingest-fixture".into()),
                evidence_code: Some("TAS".into()),
                status: Some(AssertionStatus::Asserted),
            },
        })
        .collect();
    (
        atlas,
        graph,
        PathwayData {
            annotations,
            available: true,
            ..PathwayData::default()
        },
    )
}

#[test]
fn semantic_connections_keep_both_proofs_and_status() {
    let (atlas, graph, pathways) = fixture();
    let before = serde_json::to_vec(graph.data()).unwrap();
    let c = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    let s = c
        .units
        .iter()
        .find(|u| u.relation.as_deref() == Some("shares_pathway_with"))
        .unwrap();
    assert_eq!(s.status, AssertionStatus::Inferred);
    assert_eq!(s.members.len(), 3);
    assert_eq!(s.support.len(), 2);
    assert_eq!(
        s.evidence.iter().map(|e| e.locator.as_str()).collect::<BTreeSet<_>>(),
        BTreeSet::from(["L4", "L5"])
    );
    assert!(
        c.units
            .iter()
            .filter(|u| u.relation.as_deref() == Some("participates_in"))
            .all(|u| u.status == AssertionStatus::Asserted)
    );
    let ids: BTreeSet<_> = c.units.iter().map(|u| &u.id).collect();
    assert!(c.units.iter().all(|u| u.children.iter().all(|id| ids.contains(id))));
    assert!(c.units.iter().all(|u| u.sha256.len() == 64));
    assert_eq!(before, serde_json::to_vec(graph.data()).unwrap());
    let community = c
        .units
        .iter()
        .find(|u| u.unit_type == UnitType::Community && u.subject.id == "MONDO:PLACEHOLDER_1")
        .unwrap();
    assert_eq!(community.contacts[0].url, "https://example.org/contact");
    assert_eq!(community.contacts[0].evidence[0].locator, "L3");
    assert_eq!(
        c.root_summaries.iter().map(|u| &u.id).collect::<Vec<_>>(),
        c.roots.iter().collect::<Vec<_>>()
    );
    assert!(c.root_summaries.iter().any(|u| u.contacts > 0 && u.resources > 0));
}

#[test]
fn asset_contacts_keep_both_source_routes_once() {
    let (atlas, graph, pathways) = fixture();
    let mut data = graph.into_data();
    data.assets.push(crate::graph::Asset {
        id: "asset:placeholder".into(),
        label: "Synthetic comparison".into(),
        kind: crate::graph::AssetKind::Dataset,
        category: "synthetic".into(),
        holder: None,
        holder_name: None,
        verify_url: None,
        release: false,
        records: vec![0],
        access: crate::graph::Access {
            route: "request_form".into(),
            url: Some("https://example.org/starr".into()),
            note: None,
        },
        facts: vec![(
            "access_context".into(),
            serde_json::json!({"access_routes":[
                {"request_url":"https://example.org/starr", "route_type":"study_coordinator"},
                {"request_url":"https://example.org/esco", "route_type":"study_coordinator"}
            ]})
            .to_string(),
        )],
    });
    data.edges.push(GraphEdge {
        from: "asset:placeholder".into(),
        to: "MONDO:PLACEHOLDER_1".into(),
        relation: Relation::RelatedTo,
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        reason: "synthetic fixture".into(),
        activity: ActivityIdx(0),
        records: vec![0],
    });
    let graph = Graph::new(data);
    let collection = build(&atlas, &graph, &pathways, "MONDO:PLACEHOLDER_1").unwrap();
    let community = collection
        .units
        .iter()
        .find(|u| u.unit_type == UnitType::Community)
        .unwrap();
    let routes: Vec<_> = community
        .contacts
        .iter()
        .filter(|c| c.node == "asset:placeholder")
        .collect();
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().any(|r| r.url == "https://example.org/starr"));
    assert!(routes.iter().any(|r| r.url == "https://example.org/esco"));
    assert!(routes.iter().all(|r| !r.evidence.is_empty()));
}

#[test]
fn object_only_focus_has_item_roots() {
    let (atlas, graph, pathways) = fixture();
    let mut data = graph.into_data();
    let edge = &mut data.edges[0];
    std::mem::swap(&mut edge.from, &mut edge.to);
    let graph = Graph::new(data);
    let c = build(&atlas, &graph, &pathways, "org:placeholder").unwrap();
    assert_eq!(c.roots, vec![unit_id("item", "MONDO:PLACEHOLDER_1")]);
    assert_eq!(c.root_summaries[0].subject.id, "MONDO:PLACEHOLDER_1");
}

#[test]
fn rdf_preserves_licences_and_does_not_assert_hypotheses() {
    let (atlas, graph, pathways) = fixture();
    let mut data = graph.into_data();
    data.edges[0].kind = EdgeKind::Hypothesis;
    data.provenance.entities[0].licence = Some("PLACEHOLDER licence".into());
    let graph = Graph::new(data);
    let c = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    let ttl = rdf::turtle(&c);
    assert!(ttl.contains("@base <https://w3id.org/rare-disease-atlas/>"));
    assert!(ttl.contains("dcterms:license \"PLACEHOLDER licence\""));
    assert!(ttl.contains("rdf:predicate <vocab#serves_condition>"));
    assert!(!ttl.contains(&format!(
        "{} <vocab#serves_condition>",
        rdf::iri("org:placeholder").replace("<https://w3id.org/rare-disease-atlas/", "<")
    )));
    for prefix in ["HGNC:", "NCBIGene:", "MONDO:", "PMID:", "ORCID:"] {
        assert!(
            !rdf::iri(&format!("{prefix}bad><\" "))
                .trim_matches(['<', '>'])
                .contains(['<', '>', '"', ' '])
        );
    }
}

#[test]
fn input_order_and_gene_alias_do_not_change_units() {
    let (atlas, graph, mut pathways) = fixture();
    let a = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    pathways.annotations.reverse();
    let b = build(&atlas, &graph, &pathways, "HGNC:PLACEHOLDER_1").unwrap();
    assert_eq!(
        serde_json::to_vec(&a.units).unwrap(),
        serde_json::to_vec(&b.units).unwrap()
    );
    assert_eq!(a.roots, b.roots);
}

#[test]
fn unknown_focus_is_distinct_from_no_evidence() {
    let (atlas, graph, _) = fixture();
    assert!(build(&atlas, &graph, &PathwayData::default(), "missing").is_none());
    let c = build(&atlas, &graph, &PathwayData::default(), "PLACEHOLDER_A").unwrap();
    assert!(!c.pathways_available);
    assert!(
        c.units
            .iter()
            .all(|u| u.relation.as_deref() != Some("shares_pathway_with"))
    );
}

#[test]
fn repeated_proposition_keeps_every_source_status() {
    let (atlas, graph, pathways) = fixture();
    let mut data = graph.into_data();
    let mut record = data.records[0].clone();
    record.locator = Locator::Line(99);
    record.sha256 = [2; 32];
    data.records.push(record);
    let mut edge = data.edges[0].clone();
    edge.kind = EdgeKind::Inferred;
    edge.records = vec![1];
    data.edges.push(edge);
    let graph = Graph::new(data);
    let c = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    let u = c
        .units
        .iter()
        .find(|u| u.relation.as_deref() == Some("serves_condition"))
        .unwrap();
    assert_eq!(u.status, AssertionStatus::Inferred);
    assert_eq!(u.evidence.len(), 2);
    assert!(u.evidence.iter().any(|e| e.status == Some(AssertionStatus::Asserted)));
    assert!(u.evidence.iter().any(|e| e.status == Some(AssertionStatus::Inferred)));
}

#[test]
fn runtime_filter_and_noncausal_links_do_not_create_supported_leads() {
    let (atlas, graph, pathways) = fixture();
    struct Block;
    impl RecordWithhold for Block {
        fn records_withheld(&self, _: &[u32]) -> Option<&str> {
            Some("runtime_fixture")
        }
        fn node_withheld(&self, _: crate::node::NodeKey) -> Option<&str> {
            Some("runtime_fixture")
        }
    }
    let c = build_with_filter(&atlas, &graph, &pathways, "PLACEHOLDER_A", &Block).unwrap();
    assert!(!serde_json::to_string(&c).unwrap().contains("org:placeholder"));
    let mut diseases = atlas.diseases().to_vec();
    diseases[0].genes[0].association = "Susceptibility".into();
    let atlas = Atlas::new(
        Vec::new(),
        DiseaseIdentity::default(),
        atlas.provenance.clone(),
        diseases,
    );
    let c = build(&atlas, &graph, &pathways, "MONDO:PLACEHOLDER_1").unwrap();
    assert!(
        c.units
            .iter()
            .all(|u| u.relation.as_deref() != Some("shares_pathway_with"))
    );
}

#[test]
fn quarantined_group_and_its_contact_stay_out() {
    let (atlas, graph, pathways) = fixture();
    let mut data = graph.into_data();
    data.quarantine = serde_json::from_value(serde_json::json!([{
        "record":0, "reason":"fixture_blocked"
    }]))
    .unwrap();
    let graph = Graph::new(data);
    let c = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    assert!(!serde_json::to_string(&c).unwrap().contains("org:placeholder"));
    assert!(!rdf::turtle(&c).contains("https://example.org/contact"));
    assert_eq!(c.excluded["withheld_or_unresolved_edge"], 1);
    assert!(build(&atlas, &graph, &pathways, "org:placeholder").is_none());
}

#[test]
fn rdf_has_addressable_units_and_provenance_and_escapes_content() {
    let (atlas, graph, pathways) = fixture();
    let c = build(&atlas, &graph, &pathways, "PLACEHOLDER_A").unwrap();
    let ttl = rdf::turtle(&c);
    for text in [
        "ra:StatementUnit",
        "ra:CommunityUnit",
        "ra:MechanismItemGroupUnit",
        "prov:wasDerivedFrom",
        "prov:wasGeneratedBy",
        "prov:SoftwareAgent",
        "ra:locator",
        "ra:viaPathway",
        "ra:status \"inferred\"",
    ] {
        assert!(ttl.contains(text), "{text}");
    }
    assert_eq!(rdf::lit("\"\n\\\t"), "\"\\\"\\n\\\\\\t\"");
    assert!(rdf::iri("bad:><\" ").contains("%3E%3C%22%20"));
}
