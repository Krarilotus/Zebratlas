use super::*;
use atlas_core::DiseaseIdentity;
use atlas_core::disease::Disease;
use atlas_core::evidence::GeneLink;
use atlas_core::graph::{
    Grant, GraphData, Organisation, Paper, Person, PersonSource, Quarantine, RecordHash, SourceRecord, Study, StudyKind,
};
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, Provenance, RecordRef, SourceEntity};

const CONDITION: &str = "MONDO:fixture-a";
const OTHER: &str = "MONDO:fixture-b";
const GENE: &str = "HGNC:fixture-a";
const RESOLVED: &str = "person:resolved";
const PI: &str = "person:reporter-pi";
const GRANT: &str = "REPORTER:fixture";
const PAPER: &str = "PMID:fixture";

fn atlas(gene_count: usize) -> Atlas {
    let mut a = Disease::new(CONDITION, ActivityIdx(0));
    a.name = "Synthetic condition A".into();
    for n in 0..gene_count {
        a.genes.push(GeneLink {
            symbol: format!("FIXTURE{n}"),
            association: "MENDELIAN".into(),
            source: "synthetic fixture".into(),
            source_disease: CONDITION.into(),
            pmids: vec![],
            assessed: Some(true),
            hgnc: Some(if n == 0 {
                GENE.into()
            } else {
                format!("HGNC:fixture-{n}")
            }),
            ncbi_gene: None,
            record: RecordRef::line(EntityIdx(0), n as u32 + 1),
        });
    }
    let mut b = Disease::new(OTHER, ActivityIdx(0));
    b.name = "Synthetic condition B".into();
    Atlas::new(vec![], DiseaseIdentity::default(), Provenance::default(), vec![a, b])
}

fn data() -> GraphData {
    let mut data = GraphData::default();
    let entity = data.provenance.add_entity(SourceEntity {
        file: "fixtures/connections.jsonl".into(),
        url: "https://example.invalid/synthetic-fixture".into(),
        ..Default::default()
    });
    // Every node and edge gets a separately addressable synthetic record. Record zero
    // really exists: Study uses a scalar record index, unlike the other node types.
    data.records = (0..32)
        .map(|n| SourceRecord {
            entity,
            locator: Locator::Line(n + 1),
            id: format!("synthetic-record-{n}"),
            url: None,
            fetched_at: None,
            hash: RecordHash::JsonLine,
            sha256: [0; 32],
        })
        .collect();
    data
}

fn edge(from: &str, relation: Relation, to: &str, level: LinkLevel, record: u32) -> GraphEdge {
    GraphEdge {
        from: from.into(),
        relation,
        to: to.into(),
        kind: EdgeKind::Observed,
        level,
        reason: "synthetic connection".into(),
        activity: ActivityIdx(0),
        records: vec![record],
    }
}

fn study(id: &str) -> Study {
    Study {
        id: id.into(),
        title: format!("Synthetic study {id}"),
        status: "RECRUITING".into(),
        kind: StudyKind::Trial,
        phases: vec![],
        sponsor: String::new(),
        sponsor_class: String::new(),
        start: "2026-01-01".into(),
        completion: String::new(),
        enrollment: None,
        countries: vec![],
        interventions: vec![],
        record: 0,
    }
}

fn org(id: &str) -> Organisation {
    Organisation {
        id: id.into(),
        name: format!("Synthetic group {id}"),
        kind: OrgKind::PatientGroup,
        url: None,
        contact_url: None,
        country: None,
        country_basis: None,
        description: None,
        languages: vec![],
        verified_on: None,
        channels: vec![],
        records: vec![1],
    }
}

fn person(id: &str, record: u32) -> Person {
    Person {
        id: id.into(),
        name: format!("Synthetic person {id}"),
        name_variants: vec![],
        orcids: vec![],
        affiliations: vec![],
        source: PersonSource::Resolved,
        genes: vec![],
        communities: vec![],
        cross_community: false,
        matched_by: vec![],
        merge_basis: vec![],
        records: vec![record],
    }
}

fn research_data() -> GraphData {
    let mut data = data();
    data.grants.push(Grant {
        id: GRANT.into(),
        title: "Synthetic grant".into(),
        activity_code: String::new(),
        agency: String::new(),
        organisation: "Synthetic institution".into(),
        country: String::new(),
        fiscal_years: vec![2026],
        award_total: None,
        start: String::new(),
        end: String::new(),
        url: "https://example.invalid/grant".into(),
        records: vec![3],
    });
    data.papers.push(Paper {
        id: PAPER.into(),
        title: "Synthetic paper".into(),
        journal: "Synthetic journal".into(),
        year: Some(2025),
        doi: None,
        review: false,
        records: vec![2],
    });
    data.people = vec![person(RESOLVED, 4), person(PI, 5)];
    data.edges = vec![
        edge(GRANT, Relation::AboutCondition, CONDITION, LinkLevel::Text, 6),
        edge(PAPER, Relation::AboutCondition, CONDITION, LinkLevel::Text, 7),
        edge(
            RESOLVED,
            Relation::PrincipalInvestigatorOf,
            GRANT,
            LinkLevel::Curated,
            8,
        ),
        edge(PI, Relation::PrincipalInvestigatorOf, GRANT, LinkLevel::Curated, 9),
        edge(RESOLVED, Relation::AuthorOf, PAPER, LinkLevel::Curated, 10),
        edge(RESOLVED, Relation::SameAs, PI, LinkLevel::Curated, 11),
    ];
    data
}

fn found<'a, 'g>(connections: &'a Connections<'g>, graph: &Graph, id: &str) -> &'a Found<'g> {
    connections
        .found
        .iter()
        .find(|f| graph.node_ref(f.who).id == id)
        .unwrap_or_else(|| panic!("missing connection {id}"))
}

fn researcher<'a, 'g>(f: &'a Found<'g>) -> &'a Researcher<'g> {
    match &f.origin {
        Origin::Researcher(t) => t,
        other => panic!("expected researcher, got {other:?}"),
    }
}

fn evidence_ids(f: &Found<'_>) -> Vec<String> {
    f.evidence
        .iter()
        .map(|e| match e {
            Evidence::Edge(e) => e.id(),
            Evidence::Gene(g) => format!("gene:{g}"),
            Evidence::Hierarchy { ancestor, name } => format!("hierarchy:{ancestor}:{name}"),
        })
        .collect()
}

fn quarantine(data: &mut GraphData, record: u32) {
    data.quarantine.push(Quarantine {
        record,
        reason: "synthetic withheld record".into(),
    });
    data.quarantine.sort_by_key(|q| q.record);
}

#[test]
fn related_groups_and_foreign_condition_gene_links_stay_related() {
    let atlas = atlas(1);
    let mut data = data();
    data.orgs = vec![org("org:related"), org("org:foreign"), org("org:gene-related")];
    data.edges = vec![
        edge(
            "org:related",
            Relation::ServesCondition,
            CONDITION,
            LinkLevel::Related,
            12,
        ),
        edge("org:foreign", Relation::ServesGene, GENE, LinkLevel::Curated, 13),
        edge("org:foreign", Relation::ServesCondition, OTHER, LinkLevel::Curated, 14),
        edge("org:gene-related", Relation::ServesGene, GENE, LinkLevel::Related, 15),
    ];
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, true);
    for id in ["org:related", "org:foreign", "org:gene-related"] {
        assert!(!found(&connections, &graph, id).exact, "{id} was promoted to exact");
    }
    assert!(found(&collect(&atlas, &graph, 1, true), &graph, "org:foreign").exact);
}

#[test]
fn gene_support_scope_never_promotes_gene_only_studies_to_diagnosis_exact() {
    for (count, expected) in [(1, true), (3, true), (4, false)] {
        let atlas = atlas(count);
        let mut data = data();
        data.studies.push(study("NCT:gene"));
        data.orgs.push(org("org:gene"));
        data.edges
            .push(edge("org:gene", Relation::ServesGene, GENE, LinkLevel::Gene, 12));
        data.edges
            .push(edge("NCT:gene", Relation::NamesGene, GENE, LinkLevel::Gene, 12));
        let graph = Graph::new(data);
        assert!(!found(&collect(&atlas, &graph, 0, false), &graph, "NCT:gene").exact);
        assert_eq!(
            found(&collect(&atlas, &graph, 0, false), &graph, "org:gene").exact,
            expected
        );
    }
    let mut d = atlas(1).disease_at(0).clone();
    d.genes.push(d.genes[0].clone());
    let mut modifier = d.genes[0].clone();
    modifier.symbol = "MODIFIER".into();
    modifier.hgnc = Some("HGNC:modifier".into());
    modifier.association = "modifier".into();
    d.genes.push(modifier);
    let atlas = Atlas::new(vec![], DiseaseIdentity::default(), Provenance::default(), vec![d]);
    let mut data = data();
    data.studies = vec![study("NCT:gene"), study("NCT:modifier")];
    data.edges = vec![
        edge("NCT:gene", Relation::NamesGene, GENE, LinkLevel::Gene, 12),
        edge(
            "NCT:modifier",
            Relation::NamesGene,
            "HGNC:modifier",
            LinkLevel::Gene,
            13,
        ),
    ];
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, false);
    assert_eq!(connections.genes.len(), 1);
    assert_eq!(connections.genes[0].links.len(), 2);
    assert_eq!(connections.found.len(), 1);
    assert!(!connections.found[0].exact);
}

#[test]
fn gene_only_grants_and_researchers_stay_related_without_diagnosis_evidence() {
    let atlas = atlas(1);
    let mut data = research_data();
    for edge in &mut data.edges {
        if edge.relation == Relation::AboutCondition {
            edge.relation = Relation::AboutGene;
            edge.to = GENE.into();
        }
    }
    let graph = Graph::new(data.clone());
    let connections = collect(&atlas, &graph, 0, true);
    assert!(!found(&connections, &graph, GRANT).exact);
    assert!(!found(&connections, &graph, RESOLVED).exact);
    assert_eq!(researcher(found(&connections, &graph, RESOLVED)).condition_works, 0);
    data.edges
        .push(edge(GRANT, Relation::AboutCondition, CONDITION, LinkLevel::Text, 6));
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, true);
    assert!(found(&connections, &graph, GRANT).exact);
    assert!(found(&connections, &graph, RESOLVED).exact);
}

#[test]
fn strongest_connection_retains_direct_gene_and_umbrella_evidence() {
    let atlas = atlas(1);
    let mut data = data();
    data.studies.push(study("NCT:all-paths"));
    data.edges = vec![
        edge(
            "NCT:all-paths",
            Relation::StudiesCondition,
            CONDITION,
            LinkLevel::Exact,
            12,
        ),
        edge("NCT:all-paths", Relation::NamesGene, GENE, LinkLevel::Gene, 13),
        edge(
            "NCT:all-paths",
            Relation::StudiesCondition,
            "MONDO:umbrella",
            LinkLevel::Exact,
            14,
        ),
    ];
    data.umbrella.push((
        CONDITION.into(),
        vec![("MONDO:umbrella".into(), "Synthetic umbrella".into())],
    ));
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, false);
    assert_eq!(connections.found.len(), 1);
    let f = &connections.found[0];
    assert!(f.exact);
    assert_eq!(f.level, LinkLevel::Exact);
    assert!(matches!(f.origin, Origin::Condition(_)));
    assert_eq!(f.evidence.iter().filter(|e| matches!(e, Evidence::Edge(_))).count(), 3);
    assert!(f.evidence.contains(&Evidence::Gene(0)));
    assert!(f.evidence.iter().any(|e| matches!(
        e,
        Evidence::Hierarchy {
            ancestor: "MONDO:umbrella",
            ..
        }
    )));
}

#[test]
fn researcher_counts_unique_alias_works_and_retains_identity_evidence() {
    let atlas = atlas(1);
    let graph = Graph::new(research_data());
    let connections = collect(&atlas, &graph, 0, true);
    assert_eq!((connections.papers, connections.grants), (1, 1));
    assert_eq!(connections.found.iter().filter(|f| f.kind == "researcher").count(), 1);
    let f = found(&connections, &graph, RESOLVED);
    let tally = researcher(f);
    assert_eq!((tally.condition_works, tally.gene_works, tally.grants), (2, 0, 1));
    assert!((f.score - 1013.6).abs() < 1e-9);
    let same_as = graph
        .edges()
        .iter()
        .find(|e| e.relation == Relation::SameAs)
        .unwrap()
        .id();
    assert!(evidence_ids(f).contains(&same_as));
    assert_eq!(
        collect(&atlas, &graph, 0, false)
            .found
            .iter()
            .filter(|f| f.kind == "researcher")
            .count(),
        0
    );
}

#[test]
fn quarantined_work_and_about_edges_do_not_count_or_feed_researchers() {
    let atlas = atlas(1);
    for withheld in [3, 6] {
        let mut data = research_data();
        quarantine(&mut data, withheld);
        let graph = Graph::new(data);
        let connections = collect(&atlas, &graph, 0, true);
        assert_eq!((connections.papers, connections.grants), (1, 0));
        assert!(connections.found.iter().all(|f| graph.node_ref(f.who).id != GRANT));
        let tally = researcher(found(&connections, &graph, RESOLVED));
        assert_eq!((tally.condition_works, tally.grants), (1, 0));
        assert!(
            connections
                .found
                .iter()
                .flat_map(|f| &f.evidence)
                .all(|e| { !matches!(e, Evidence::Edge(edge) if edge.from == GRANT || edge.to == GRANT) })
        );
    }
}

#[test]
fn quarantined_roles_and_alias_nodes_cannot_feed_canonical_researcher() {
    let atlas = atlas(1);
    for withheld in [5, 9, 11] {
        let mut data = research_data();
        // The grant reaches R only through P. R retains a separate valid paper path.
        data.edges
            .retain(|e| !(e.from == RESOLVED && e.relation == Relation::PrincipalInvestigatorOf));
        quarantine(&mut data, withheld);
        let graph = Graph::new(data);
        let connections = collect(&atlas, &graph, 0, true);
        let f = found(&connections, &graph, RESOLVED);
        assert_eq!((researcher(f).condition_works, researcher(f).grants), (1, 0));
        assert!(
            f.evidence
                .iter()
                .all(|e| !matches!(e, Evidence::Edge(edge) if edge.records.contains(&withheld)))
        );
        // A withheld identity edge may leave a visible, independent PI profile. It
        // cannot attribute that profile's grant to the resolved person.
        if withheld == 11 {
            assert_eq!(researcher(found(&connections, &graph, PI)).grants, 1);
        } else {
            assert!(connections.found.iter().all(|f| graph.node_ref(f.who).id != PI));
        }
    }
}

#[test]
fn quarantined_direct_edges_fall_back_to_visible_gene_paths() {
    let atlas = atlas(1);
    let mut data = data();
    data.studies.push(study("NCT:visible-gene"));
    data.edges = vec![
        edge(
            "NCT:visible-gene",
            Relation::StudiesCondition,
            CONDITION,
            LinkLevel::Exact,
            12,
        ),
        edge("NCT:visible-gene", Relation::NamesGene, GENE, LinkLevel::Gene, 13),
    ];
    quarantine(&mut data, 12);
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, false);
    let f = found(&connections, &graph, "NCT:visible-gene");
    assert_eq!(f.level, LinkLevel::Gene);
    assert!(matches!(f.origin, Origin::Gene { .. }));
    assert!(
        f.evidence
            .iter()
            .all(|e| !matches!(e, Evidence::Edge(edge) if edge.records.contains(&12)))
    );
}

#[test]
fn wrong_node_kinds_are_ignored_without_hiding_valid_identity_paths() {
    let atlas = atlas(1);
    let mut data = research_data();
    data.studies.push(study("NCT:malformed"));
    data.orgs.push(org("org:malformed"));
    data.edges.extend([
        edge(
            "org:malformed",
            Relation::StudiesCondition,
            CONDITION,
            LinkLevel::Exact,
            12,
        ),
        edge(
            "NCT:malformed",
            Relation::ServesCondition,
            CONDITION,
            LinkLevel::Exact,
            13,
        ),
        edge("org:malformed", Relation::NamesGene, GENE, LinkLevel::Gene, 14),
        edge(
            "NCT:malformed",
            Relation::AboutCondition,
            CONDITION,
            LinkLevel::Text,
            15,
        ),
        edge("NCT:malformed", Relation::AuthorOf, PAPER, LinkLevel::Curated, 16),
        // This malformed SameAs sorts before the valid person mapping.
        edge("NCT:malformed", Relation::SameAs, PI, LinkLevel::Curated, 17),
    ]);
    data.edges
        .retain(|e| !(e.from == RESOLVED && e.relation == Relation::PrincipalInvestigatorOf));
    let graph = Graph::new(data);
    let connections = collect(&atlas, &graph, 0, true);
    assert_eq!((connections.papers, connections.grants), (1, 1));
    let tally = researcher(found(&connections, &graph, RESOLVED));
    assert_eq!((tally.condition_works, tally.grants), (2, 1));
    assert!(
        connections
            .found
            .iter()
            .all(|f| !["NCT:malformed", "org:malformed"].contains(&graph.node_ref(f.who).id.as_str()))
    );
}

#[test]
fn reversing_edges_preserves_selection_counts_and_evidence_order() {
    let atlas = atlas(1);
    let mut data = research_data();
    data.studies = vec![study("NCT:b"), study("NCT:a")];
    data.edges.extend([
        edge("NCT:b", Relation::NamesGene, GENE, LinkLevel::Gene, 12),
        edge("NCT:a", Relation::NamesGene, GENE, LinkLevel::Gene, 13),
        edge(GRANT, Relation::AboutGene, GENE, LinkLevel::Gene, 14),
    ]);
    let forward = Graph::new(data.clone());
    data.edges.reverse();
    let reverse = Graph::new(data);
    let snapshot = |graph: &Graph| {
        let connections = collect(&atlas, graph, 0, true);
        (
            connections.papers,
            connections.grants,
            connections
                .found
                .iter()
                .map(|f| {
                    (
                        graph.node_ref(f.who).id,
                        f.exact,
                        f.level,
                        f.score.to_bits(),
                        evidence_ids(f),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(snapshot(&forward), snapshot(&reverse));
    let connections = collect(&atlas, &forward, 0, true);
    let study_ids: Vec<_> = connections
        .found
        .iter()
        .filter(|f| f.kind == "trial")
        .map(|f| forward.node_ref(f.who).id)
        .collect();
    assert_eq!(study_ids, ["NCT:a", "NCT:b"]);
}

#[test]
fn closed_runtime_withholding_omits_people_and_preserves_non_person_connections() {
    use atlas_core::withhold::{Salt, Withhold};

    let atlas = atlas(1);
    let mut data = research_data();
    data.studies.push(study("NCT:visible"));
    data.edges.push(edge(
        "NCT:visible",
        Relation::StudiesCondition,
        CONDITION,
        LinkLevel::Exact,
        12,
    ));
    let mut graph = Graph::new(data);
    graph.set_withhold(Withhold::closed(Salt::new("synthetic-test"), "invalid fixture list"));
    let connections = collect(&atlas, &graph, 0, true);
    assert_eq!((connections.papers, connections.grants), (1, 1));
    assert_eq!(connections.found.len(), 2);
    assert_eq!(found(&connections, &graph, GRANT).kind, "grant");
    assert_eq!(found(&connections, &graph, "NCT:visible").kind, "trial");
    assert!(connections.found.iter().all(|f| f.kind != "researcher"));
    assert!(connections.found.iter().flat_map(|f| &f.evidence).all(|e| {
        !matches!(e, Evidence::Edge(edge) if matches!(
            edge.relation,
            Relation::AuthorOf | Relation::PrincipalInvestigatorOf | Relation::SameAs
        ))
    }));
}
