use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, Paper, RecordHash, Relation, SourceRecord};
use atlas_core::identity::DiseaseIdentity;
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{Activity, Agent, EntityIdx, Locator, Provenance, RecordRef, SourceEntity};
use atlas_core::withhold::{Salt, Scope, SuppressionEntry, SuppressionFile, Withhold};
use atlas_core::{Atlas, Disease, Graph, Term};
use atlas_release::{export, public_url, quarantine::Quarantine, validate_edge, validate_node};

fn entity(file: &str, url: &str) -> SourceEntity {
    SourceEntity {
        id: format!("source:{file}"),
        file: file.into(),
        url: url.into(),
        version: Some("fixture-v1".into()),
        retrieved_at: Some("2026-10-03T22:00:00Z".into()),
        sha256: Some("a".repeat(64)),
        bytes: 1,
        licence: Some("CC0-forged".into()),
    }
}

// Fixtures model ingestion's licence classification, including conservative unknown terms.
fn fixture_graph(mut data: GraphData) -> Graph {
    for (i, entity) in data.provenance.entities.iter().enumerate() {
        if data.licences.iter().any(|l| l.entity == EntityIdx(i as u16)) {
            continue;
        }
        let licence = entity.licence.clone().unwrap_or_else(|| "not stated".into());
        data.licences.push(atlas_core::graph::EntityLicence {
            entity: EntityIdx(i as u16),
            class: atlas_core::graph::LicenceClass::classify(&licence),
            licence,
        });
    }
    Graph::new(data)
}

fn accepted_native_mapping(graph: &Graph, canonical: &str, alias: &str) -> Graph {
    use atlas_core::graph::{IdentityMapping, IdentityMember, IdentityMerge};
    let mut data = graph.data().clone();
    let entity = data
        .provenance
        .add_entity(entity("cache/mappings/test.sssom.tsv", "https://example.org/mappings"));
    let record = data.records.len() as u32;
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Line(2),
        id: canonical.into(),
        url: None,
        fetched_at: None,
        hash: RecordHash::TsvLine,
        sha256: [3; 32],
    });
    data.aliases.push((alias.into(), canonical.into()));
    data.identity_merges.push(IdentityMerge {
        canonical: canonical.into(),
        activity: Activity {
            id: "activity:identity-merge:native-fixture".into(),
            ..Default::default()
        },
        members: vec![
            IdentityMember {
                id: canonical.into(),
                derived_from: vec![record],
            },
            IdentityMember {
                id: alias.into(),
                derived_from: vec![record],
            },
        ],
        mappings: vec![IdentityMapping {
            subject: canonical.into(),
            object: alias.into(),
            mapping_set_id: "https://example.org/mappings".into(),
            mapping_set_version: "fixture-v1".into(),
            record,
            rule_id: "R-DIS-01".into(),
            rule_version: "1.0.0".into(),
            evidence_url: "https://example.org/source".into(),
            evidence_sha256: "a".repeat(64),
            evidence_locator: "line 2".into(),
            mapping_tool: "fixture".into(),
            assertion_id: "urn:rare-atlas:assertion:native".into(),
            decision_id: "urn:rare-atlas:decision:native".into(),
            gate_manifest_sha256: "b".repeat(64),
        }],
    });
    fixture_graph(data)
}

fn fixture() -> (Atlas, Graph) {
    let mut prov = Provenance::default();
    let hp = prov.add_entity(entity("hp.obo", "https://example.org/hp.obo"));
    let mondo = prov.add_entity(entity("mondo.obo", "https://example.org/mondo.obo"));
    let activity = prov.add_activity(Activity {
        id: "activity:ingest-mondo".into(),
        used: vec![mondo, hp],
        agent: Agent {
            name: "fixture".into(),
            version: "1".into(),
            commit: None,
        },
        ..Default::default()
    });
    let mut disease = Disease::new("MONDO:0000001", activity);
    disease.name = "Disease \"allowed\"\nΩ".into();
    disease.derive_from(RecordRef::record(mondo, "MONDO:0000001"));
    let atlas = Atlas::new(
        vec![Term::new("HP:0000001")],
        DiseaseIdentity::default(),
        prov,
        vec![disease],
    );
    let mut data = GraphData::default();
    let entity = data.provenance.add_entity(entity(
        "cache/pubmed/FIXTURE.json",
        "https://example.org/pubmed?api_key=DO_NOT_EXPORT_KEY",
    ));
    let act = data.provenance.add_activity(Activity {
        id: "activity:ingest-pubmed".into(),
        used: vec![entity],
        parameters: [("secret".into(), "DO_NOT_EXPORT_KEY".into())].into(),
        ..Default::default()
    });
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Record("records[0]".into()),
        id: "PMID:1".into(),
        url: Some("https://example.org/paper/1?token=DO_NOT_EXPORT_KEY".into()),
        fetched_at: Some("2026-10-03T22:00:00Z".into()),
        hash: RecordHash::CanonicalJson,
        sha256: [1; 32],
    });
    data.papers.push(Paper {
        id: "PMID:1".into(),
        title: "COPYRIGHT_SENTINEL_DO_NOT_LEAK".into(),
        journal: "COPYRIGHT_SENTINEL_DO_NOT_LEAK".into(),
        year: Some(2026),
        doi: None,
        review: false,
        records: vec![0],
    });
    data.edges.push(GraphEdge {
        from: "PMID:1".into(),
        relation: Relation::AboutCondition,
        to: "MONDO:0000001".into(),
        kind: EdgeKind::Inferred,
        level: LinkLevel::Text,
        reason: "COPYRIGHT_SENTINEL_DO_NOT_LEAK".into(),
        activity: act,
        records: vec![0],
    });
    (atlas, fixture_graph(data))
}

fn open() -> Withhold {
    Withhold::empty(Salt::new("test"))
}

#[test]
fn suppressed_person_and_edges_never_reach_the_release() {
    use atlas_core::graph::{Person, PersonSource};
    let (atlas, graph) = fixture();
    let mut data = graph.into_data();
    data.people.push(Person {
        id: "ORCID:0000-0000-0000-0000".into(),
        name: "Synthetic Testperson".into(),
        name_variants: vec![],
        orcids: vec!["0000-0000-0000-0000".into()],
        affiliations: vec![],
        source: PersonSource::Orcid,
        genes: vec![],
        communities: vec![],
        cross_community: false,
        matched_by: vec![],
        merge_basis: vec![],
        records: vec![0],
    });
    let mut e = data.edges[0].clone();
    e.from = "ORCID:0000-0000-0000-0000".into();
    e.relation = Relation::AuthorOf;
    e.to = "PMID:1".into();
    data.edges.push(e);
    let graph = fixture_graph(data);
    let salt = Salt::new("test");
    let file = SuppressionFile {
        entries: vec![SuppressionEntry {
            id: "sup_x".into(),
            keys: salt.person_keys(graph.person(0)),
            scope: Scope::All,
            reason: "gdpr_art17_erasure".into(),
            date: "2026-10-04T12:00:00Z".into(),
            reviewer: "agent:reviewer/tester".into(),
            request: None,
            salt_id: salt.id().into(),
        }],
        ..SuppressionFile::new()
    };
    let w = Withhold::from_lists(salt.clone(), Some(&serde_json::to_vec(&file).unwrap()), None).unwrap();
    let path = output("suppressed");
    let report = export(&atlas, &graph, &w, &path, "fixture-v1", Quarantine::default()).unwrap();
    assert_eq!((report.withheld_nodes, report.withheld_edges), (1, 1));
    for f in ["nodes.jsonl", "edges.jsonl", "graph.ttl"] {
        let text = std::fs::read_to_string(path.join(f)).unwrap();
        assert!(!text.contains("0000-0000-0000-0000"), "{f} holds the suppressed person");
    }
    std::fs::remove_dir_all(&path).ok();
    // Fail-closed: a closed filter writes nothing.
    let closed = Withhold::closed(salt, "unreadable");
    let path = output("closed");
    assert!(export(&atlas, &graph, &closed, &path, "fixture-v1", Quarantine::default()).is_err());
    assert!(!path.exists());
}

fn output(test: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "atlas-release-{}-{}-{test}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn hierarchy_exact_mappings_and_asset_endpoints_have_licensed_provenance() {
    use atlas_core::{
        Xref,
        graph::{Access, Asset, AssetKind},
    };
    let (atlas, graph) = fixture();
    let (hp, _, prov, diseases) = atlas.parts();
    let mut diseases = diseases.to_vec();
    diseases[0].parents = vec!["MONDO:0000002".into()];
    diseases[0]
        .source_ids
        .extend(["OMIM:100001".into(), "ORPHA:999".into()]);
    let mut term = Term::new(&diseases[0].id);
    term.xrefs.push(Xref {
        id: "OMIM:100001".into(),
        sources: vec!["MONDO:equivalentTo".into()],
    });
    // A related/non-exact source ID must not become skos:exactMatch.
    let identity = DiseaseIdentity::new(&[term, Term::new("MONDO:0000002")], []);
    let mut parent = diseases[0].clone();
    parent.id = "MONDO:0000002".into();
    parent.parents.clear();
    parent.source_ids.clear();
    diseases.push(parent);
    let atlas = Atlas::new(hp.to_vec(), identity, prov.clone(), diseases);
    let mut data = graph.into_data();
    data.assets.push(Asset {
        id: "fixture:restricted-model".into(),
        label: "RESTRICTED_ASSET_SENTINEL".into(),
        kind: AssetKind::Model,
        category: "synthetic".into(),
        holder: None,
        holder_name: None,
        access: Access {
            route: "official_page".into(),
            url: Some("https://example.org/synthetic-model".into()),
            note: None,
        },
        facts: vec![],
        verify_url: None,
        release: true,
        records: vec![0],
    });
    let mut edge = data.edges[0].clone();
    edge.from = "fixture:restricted-model".into();
    edge.relation = Relation::ModelOf;
    data.edges.push(edge.clone());
    let mut disabled = data.assets[0].clone();
    disabled.id = "fixture:disabled-model".into();
    disabled.release = false;
    data.assets.push(disabled);
    edge.from = "fixture:disabled-model".into();
    data.edges.push(edge);
    let graph = fixture_graph(data);
    let graph = accepted_native_mapping(&graph, "MONDO:0000001", "OMIM:100001");
    let path = output("sparql-relationships");
    let report = export(&atlas, &graph, &open(), &path, "fixture-v1", Quarantine::default()).unwrap();
    assert_eq!(report.nodes["asset"], 1);
    assert_eq!(report.excluded_nodes["asset"], 1);
    assert_eq!(report.edges["subclass_of (supplemental)"], 1);
    assert_eq!(report.edges["model_of"], 1);
    assert_eq!(report.excluded_edges["model_of"], 1);
    assert_eq!(report.mapping_rows, 1);
    let rdf = std::fs::read_to_string(path.join("graph.ttl")).unwrap();
    assert!(rdf.contains("<http://www.w3.org/2000/01/rdf-schema#subClassOf>"));
    assert!(rdf.contains("<http://www.w3.org/2004/02/skos/core#exactMatch>"));
    assert!(rdf.contains("/id/OMIM%3A100001>"));
    assert!(!rdf.contains("/id/ORPHA%3A999>"));
    assert!(rdf.contains("a ra:Mapping"));
    assert!(rdf.contains("prov:qualifiedDerivation"));
    assert!(!rdf.contains("RESTRICTED_ASSET_SENTINEL"));
    assert!(!rdf.contains("ra:model_of")); // Restricted model relationship stays link-only.
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn restricted_text_and_credentials_never_reach_any_payload() {
    let (atlas, graph) = fixture();
    let path = output("payload");
    let report = export(&atlas, &graph, &open(), &path, "fixture-v1", Quarantine::default()).unwrap();
    assert_eq!(report.link_only_edges, 1);
    assert_eq!(report.asserted_edges, 0);
    for name in [
        "graph.ttl",
        "nodes.jsonl",
        "edges.jsonl",
        "integrity.json",
        "release-report.json",
        "source-policies.json",
    ] {
        let text = std::fs::read_to_string(path.join(name)).unwrap();
        assert!(!text.contains("COPYRIGHT_SENTINEL_DO_NOT_LEAK"), "leaked in {name}");
        assert!(!text.contains("DO_NOT_EXPORT_KEY"), "credential leaked in {name}");
    }
    let edges = std::fs::read_to_string(path.join("edges.jsonl")).unwrap();
    let mut e: atlas_release::ReleaseEdge = serde_json::from_str(edges.trim()).unwrap();
    e.relation = Some("about_condition".into());
    assert!(validate_edge(&e).is_err(), "a restricted relation must fail the build");
    let nodes = std::fs::read_to_string(path.join("nodes.jsonl")).unwrap();
    let mut paper: atlas_release::ReleaseNode = nodes
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .find(|n: &atlas_release::ReleaseNode| n.kind == "paper")
        .unwrap();
    paper.label = Some("leak".into());
    assert!(validate_node(&paper).is_err(), "a restricted title must fail the build");
    assert!(
        export(&atlas, &graph, &open(), &path, "fixture-v1", Quarantine::default()).is_err(),
        "immutable releases must not be overwritten"
    );
}

#[test]
fn url_filter_rejects_credentials_and_non_public_schemes() {
    assert_eq!(
        public_url("https://example.org/x?secret=y#z").unwrap(),
        "https://example.org/x"
    );
    for url in [
        "file:///secret",
        "javascript:alert(1)",
        "https://user:pw@example.org/x",
        "https://example.org/\nsecret",
    ] {
        assert!(public_url(url).is_none(), "{url:?}");
    }
}

#[test]
fn absent_checksum_is_rejected_before_writing_record() {
    let (mut atlas, graph) = fixture();
    atlas.provenance.entities[usize::from(EntityIdx(1).0)].sha256 = None;
    assert!(
        export(
            &atlas,
            &graph,
            &open(),
            &output("checksum"),
            "fixture-v1",
            Quarantine::default()
        )
        .is_err()
    );
}

#[test]
fn duplicate_releasable_nodes_remain_a_hard_error() {
    let (atlas, graph) = fixture();
    let mut data = graph.data().clone();
    data.papers.push(data.papers[0].clone());
    assert!(
        export(
            &atlas,
            &fixture_graph(data),
            &open(),
            &output("duplicate"),
            "fixture-v1",
            Quarantine::default()
        )
        .is_err()
    );
}

fn all_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(all_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn release_preserves_identity_rules_rows_activity_and_member_derivations() {
    use atlas_core::graph::{IdentityMapping, IdentityMember, IdentityMerge};
    let (atlas, graph) = fixture();
    // A source may retain an original alias node and endpoint in the published graph.
    let (hp, identity, prov, diseases) = atlas.parts();
    let mut diseases = diseases.to_vec();
    let mut alias = diseases[0].clone();
    alias.id = "DOID:1".into();
    diseases.push(alias);
    let atlas = Atlas::new(hp.to_vec(), identity.clone(), prov.clone(), diseases);
    let mut data = graph.into_data();
    let mut alias_edge = data.edges[0].clone();
    alias_edge.to = "DOID:1".into();
    data.edges.push(alias_edge);
    let entity = data.provenance.add_entity(entity(
        "cache/mappings/disease-xrefs.sssom.tsv",
        "https://example.org/mappings",
    ));
    let record = data.records.len() as u32;
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Line(2),
        id: "MONDO:0000001".into(),
        url: Some("https://example.org/source?key=DO_NOT_EXPORT_KEY".into()),
        fetched_at: Some("2026-10-04T00:00:00Z".into()),
        hash: RecordHash::TsvLine,
        sha256: [3; 32],
    });
    data.aliases.push(("DOID:1".into(), "MONDO:0000001".into()));
    data.identity_merges.push(IdentityMerge {
        canonical: "MONDO:0000001".into(),
        activity: Activity {
            id: "activity:identity-merge:fixture".into(),
            label: "identity merge".into(),
            used: vec![entity],
            started_at: Some("2026-10-04T00:00:00Z".into()),
            agent: Agent {
                name: "fixture-engine".into(),
                version: "1".into(),
                commit: None,
            },
            ..Default::default()
        },
        members: vec![
            IdentityMember {
                id: "MONDO:0000001".into(),
                derived_from: vec![record],
            },
            IdentityMember {
                id: "DOID:1".into(),
                derived_from: vec![record],
            },
        ],
        mappings: vec![IdentityMapping {
            subject: "MONDO:0000001".into(),
            object: "DOID:1".into(),
            mapping_set_id: "https://example.org/mappings".into(),
            mapping_set_version: "fixture-v1".into(),
            record,
            rule_id: "R-DIS-01".into(),
            rule_version: "1.0.0".into(),
            evidence_url: "https://example.org/source?key=DO_NOT_EXPORT_KEY".into(),
            evidence_sha256: "a".repeat(64),
            evidence_locator: "line 3".into(),
            assertion_id: "urn:rare-atlas:assertion:fixture".into(),
            decision_id: "urn:rare-atlas:decision:fixture".into(),
            gate_manifest_sha256: "b".repeat(64),
            mapping_tool: "fixture-align 1".into(),
        }],
    });
    let graph = fixture_graph(data);
    let path = output("identity-merge");
    export(&atlas, &graph, &open(), &path, "fixture-v1", Quarantine::default()).unwrap();
    let json = std::fs::read_to_string(path.join("identity-merges.jsonl")).unwrap();
    assert!(json.contains("R-DIS-01"));
    assert!(json.contains("row_sha256"));
    assert!(json.contains("prov:wasDerivedFrom"));
    assert!(!json.contains("DO_NOT_EXPORT_KEY"));
    let rdf = std::fs::read_to_string(path.join("graph.ttl")).unwrap();
    assert!(rdf.contains("ra:ruleId \"R-DIS-01\""));
    assert!(!rdf.contains("<http://www.w3.org/2004/02/skos/core#exactMatch>"));
    assert!(rdf.contains("identity%2Dmerge") || rdf.contains("identity-merge"));
    assert!(path.join("mappings/rules.json").exists());
    assert!(rdf.contains("urn:rare-atlas:decision:fixture"));
    let dependencies = std::fs::read_to_string(path.join("identity-statement-dependencies.jsonl")).unwrap();
    assert!(dependencies.contains("urn:rare-atlas:decision:fixture"));
    let rows: Vec<serde_json::Value> = dependencies.lines().map(|s| serde_json::from_str(s).unwrap()).collect();
    assert!(
        rows.iter()
            .any(|r| r["statement_id"].as_str().unwrap().contains("DOID:1"))
    );
    assert_eq!(
        rows.len(),
        2,
        "one dependency per original or canonical statement, without duplicates"
    );
}

#[test]
fn persons_and_embedded_names_never_reach_any_output_file() {
    use atlas_core::graph::{
        Channel, Contact, Grant, Official, OrgKind, Organisation, Person, PersonSource, Study, StudyContacts, StudyKind,
    };
    let (atlas, graph) = fixture();
    let mut data = graph.data().clone();
    let people_entity = data.provenance.add_entity(entity(
        "cache/people/overlap.json",
        "https://example.org/private-people",
    ));
    let people_activity = data.provenance.add_activity(Activity {
        id: "activity:people-PERSON_NAME_SENTINEL".into(),
        used: vec![people_entity],
        ..Default::default()
    });
    data.records.push(SourceRecord {
        entity: people_entity,
        locator: Locator::Record("records[0]".into()),
        id: "person:PERSON_NAME_SENTINEL".into(),
        url: Some("https://orcid.org/0000-0001-2345-6789".into()),
        fetched_at: None,
        hash: RecordHash::CanonicalJson,
        sha256: [2; 32],
    });
    for id in [
        "person:PERSON_NAME_SENTINEL",
        "ORCID:0000-0001-2345-6789",
        "REPORTER.PI:1",
        "arbitrary-person-id",
    ] {
        data.people.push(Person {
            id: id.into(),
            name: "PERSON_NAME_SENTINEL".into(),
            name_variants: vec!["PERSON_NAME_SENTINEL".into()],
            orcids: vec![],
            affiliations: vec!["PERSON_NAME_SENTINEL".into()],
            source: PersonSource::Resolved,
            genes: vec![],
            communities: vec![],
            cross_community: false,
            matched_by: vec![],
            merge_basis: vec![],
            records: vec![1],
        });
        data.edges.push(GraphEdge {
            from: id.into(),
            to: "PMID:1".into(),
            relation: Relation::AuthorOf,
            kind: EdgeKind::Observed,
            level: LinkLevel::Curated,
            reason: "PERSON_NAME_SENTINEL".into(),
            activity: people_activity,
            records: vec![1, 0],
        });
    }
    data.grants.push(Grant {
        id: "REPORTER:1".into(),
        title: "PERSON_NAME_SENTINEL".into(),
        activity_code: "".into(),
        agency: "".into(),
        organisation: "PERSON_NAME_SENTINEL".into(),
        country: "".into(),
        fiscal_years: vec![],
        award_total: None,
        start: "".into(),
        end: "".into(),
        url: "https://example.org/grant".into(),
        records: vec![0],
    });
    data.studies.push(Study {
        id: "NCT00000001".into(),
        title: "PERSON_NAME_SENTINEL".into(),
        status: "".into(),
        kind: StudyKind::Trial,
        phases: vec![],
        sponsor: "PERSON_NAME_SENTINEL".into(),
        sponsor_class: "".into(),
        start: "".into(),
        completion: "".into(),
        enrollment: None,
        countries: vec![],
        interventions: vec![],
        record: 0,
    });
    data.contacts.push(StudyContacts {
        study: "NCT00000001".into(),
        central: vec![Contact {
            name: "PERSON_NAME_SENTINEL".into(),
            role: "PERSON_NAME_SENTINEL".into(),
            phone: None,
            email: None,
        }],
        officials: vec![Official {
            name: "PERSON_NAME_SENTINEL".into(),
            affiliation: "PERSON_NAME_SENTINEL".into(),
            role: "PERSON_NAME_SENTINEL".into(),
        }],
        sites: vec![],
        last_update: "".into(),
        record: 0,
    });
    data.orgs.push(Organisation {
        id: "atlasorg:fixture".into(),
        name: "PERSON_NAME_SENTINEL".into(),
        kind: OrgKind::Institution,
        url: None,
        contact_url: Some("https://example.org/PERSON_NAME_SENTINEL".into()),
        country: None,
        country_basis: None,
        description: Some("Staff: PERSON_NAME_SENTINEL".into()),
        languages: vec![],
        verified_on: None,
        channels: vec![Channel {
            kind: "staff".into(),
            value: "PERSON_NAME_SENTINEL".into(),
            evidence_url: None,
        }],
        records: vec![0],
    });
    data.edges.push(GraphEdge {
        from: "REPORTER.PI:1".into(),
        to: "REPORTER:1".into(),
        relation: Relation::PrincipalInvestigatorOf,
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        reason: "PERSON_NAME_SENTINEL".into(),
        activity: people_activity,
        records: vec![1],
    });
    data.edges.push(GraphEdge {
        from: "arbitrary-person-id".into(),
        to: "ORCID:0000-0001-2345-6789".into(),
        relation: Relation::SameAs,
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        reason: "PERSON_NAME_SENTINEL".into(),
        activity: people_activity,
        records: vec![1],
    });
    let path = output("privacy");
    let report = export(
        &atlas,
        &fixture_graph(data),
        &open(),
        &path,
        "fixture-v1",
        Quarantine::default(),
    )
    .unwrap();
    assert!(!report.nodes.contains_key("person"));
    assert!(!report.sources.contains_key("people"));
    assert_eq!(report.excluded_nodes["person"], 4);
    assert_eq!(report.excluded_edges.values().sum::<usize>(), 6);
    for kind in ["paper", "grant", "study", "organisation"] {
        assert_eq!(report.nodes[kind], 1);
    }
    for path in all_files(&path) {
        let text = std::fs::read_to_string(&path).unwrap();
        for marker in [
            "PERSON_NAME_SENTINEL",
            "0000-0001-2345-6789",
            "ORCID:",
            "orcid.org/",
            "cache/people/",
            "arbitrary-person-id",
            "REPORTER.PI:",
        ] {
            assert!(!text.contains(marker), "{marker} leaked in {}", path.display());
        }
        if path.extension().is_some_and(|e| e == "jsonl") {
            for line in text.lines() {
                let row: serde_json::Value = serde_json::from_str(line).unwrap();
                for field in [
                    "pi",
                    "contact",
                    "contacts",
                    "author",
                    "authors",
                    "staff",
                    "name",
                    "name_variants",
                    "orcids",
                ] {
                    assert!(row.get(field).is_none(), "{field} leaked");
                }
            }
        }
    }
}

#[test]
fn quarantine_excludes_records_and_edges_from_all_formats() {
    let (atlas, graph) = fixture();
    let data = output("quarantine-data");
    std::fs::create_dir_all(data.join("cache")).unwrap();
    std::fs::write(
        data.join("cache/quarantine.json"),
        include_str!("fixtures/quarantine.json"),
    )
    .unwrap();
    let q = Quarantine::load(&data).unwrap();
    let target = output("quarantine-release");
    let report = export(&atlas, &graph, &open(), &target, "fixture-v1", q).unwrap();
    assert_eq!(report.quarantine.listed_records, 1);
    assert_eq!(report.quarantine.excluded_nodes, 1);
    assert_eq!(report.quarantine.excluded_edges, 1);
    assert_eq!(report.excluded_nodes["paper"], 1);
    assert_eq!(report.excluded_edges["about_condition"], 1);
    for path in all_files(&target) {
        let text = std::fs::read_to_string(&path).unwrap();
        for marker in ["PMID:1", "PMID%3A1", "records[0]", "paper/1"] {
            assert!(!text.contains(marker), "quarantined {marker} in {}", path.display());
        }
    }
}

#[test]
fn missing_quarantine_is_empty_but_malformed_manifest_is_fatal() {
    let path = output("missing-quarantine");
    assert!(!Quarantine::load(&path).unwrap().summary.present);
    std::fs::create_dir_all(path.join("cache")).unwrap();
    for contents in [
        "{",
        "{}",
        "{\"records\":[{}]}",
        "{\"records\":[{\"cache_file\":\"../secret\",\"record_locator\":\"/records/0\"}]}",
    ] {
        std::fs::write(path.join("cache/quarantine.json"), contents).unwrap();
        assert!(Quarantine::load(&path).is_err(), "accepted {contents}");
    }
}

#[test]
fn quarantine_uses_file_and_locator_and_excludes_mixed_lineage_and_endpoints() {
    let (atlas, graph) = fixture();
    let mut data = graph.data().clone();
    let other = data
        .provenance
        .add_entity(entity("cache/pubmed/OTHER.json", "https://example.org/other"));
    let mut record = data.records[0].clone();
    record.entity = other;
    record.id = "PMID:2".into();
    record.url = Some("https://example.org/paper/2".into());
    data.records.push(record); // Same locator in a different cache is retained.
    let mut record = data.records[0].clone();
    record.locator = Locator::Record("records[1]".into());
    record.id = "PMID:3".into();
    record.url = Some("https://example.org/paper/3".into());
    data.records.push(record); // Different locator in the listed cache is retained.
    for (id, records) in [("PMID:2", vec![1]), ("PMID:3", vec![2]), ("PMID:4", vec![0, 1])] {
        let mut paper = data.papers[0].clone();
        paper.id = id.into();
        paper.records = records;
        data.papers.push(paper);
    }
    let mut edge = data.edges[0].clone();
    edge.from = "PMID:1".into();
    edge.records = vec![1];
    // Replace the original edge with clean provenance but an excluded endpoint.
    data.edges[0] = edge;
    let data_dir = output("mixed-quarantine-data");
    std::fs::create_dir_all(data_dir.join("cache")).unwrap();
    std::fs::write(
        data_dir.join("cache/quarantine.json"),
        include_str!("fixtures/quarantine.json"),
    )
    .unwrap();
    let target = output("mixed-quarantine-release");
    let report = export(
        &atlas,
        &fixture_graph(data),
        &open(),
        &target,
        "fixture-v1",
        Quarantine::load(&data_dir).unwrap(),
    )
    .unwrap();
    assert_eq!(report.nodes["paper"], 2);
    assert_eq!(report.quarantine.excluded_nodes, 2);
    assert_eq!(report.exclusion_reasons["excluded_endpoint"], 1);
    assert_eq!(report.link_only_edges, 0);
}

#[test]
fn quarantine_source_urls_close_missing_curated_page_lineage() {
    let (atlas, graph) = fixture();
    let data_dir = output("url-quarantine-data");
    std::fs::create_dir_all(data_dir.join("cache")).unwrap();
    std::fs::write(
        data_dir.join("cache/quarantine.json"),
        r#"{"records":[{
        "cache_file":"orgs/candidates.json", "record_locator":"/records/99",
        "source_url":"https://example.org/paper/1?token=DO_NOT_EXPORT_KEY"}]}"#,
    )
    .unwrap();
    let target = output("url-quarantine-release");
    let report = export(
        &atlas,
        &graph,
        &open(),
        &target,
        "fixture-v1",
        Quarantine::load(&data_dir).unwrap(),
    )
    .unwrap();
    assert_eq!(report.quarantine.excluded_nodes, 1);
    assert_eq!(report.quarantine.excluded_edges, 1);
}

#[test]
fn quarantine_excludes_disease_mappings_and_reconciles_status_counts() {
    use atlas_core::term::Xref;
    let (original, graph) = fixture();
    let mut term = Term::new("MONDO:0000001");
    term.xrefs.push(Xref {
        id: "OMIM:1".into(),
        sources: vec!["MONDO:equivalentTo".into()],
    });
    let identity = DiseaseIdentity::new(&[term], std::iter::empty());
    let mut disease = original.diseases()[0].clone();
    disease.source_ids.insert("OMIM:1".into());
    // A raw record is explicitly named in the manifest; only file+locator matching matters.
    let mut prov = original.provenance.clone();
    prov.entities[1].file = "cache/labels/mondo.json".into();
    let atlas = Atlas::new(vec![Term::new("HP:0000001")], identity, prov, vec![disease]);
    let ungated = export(
        &atlas,
        &graph,
        &open(),
        &output("ungated-mapping-baseline"),
        "fixture-v1",
        Quarantine::default(),
    )
    .unwrap();
    assert_eq!(
        ungated.mapping_rows, 0,
        "a legacy native alias must not bypass accepted decisions"
    );
    let graph = accepted_native_mapping(&graph, "MONDO:0000001", "OMIM:1");
    let baseline = export(
        &atlas,
        &graph,
        &open(),
        &output("mapping-baseline"),
        "fixture-v1",
        Quarantine::default(),
    )
    .unwrap();
    assert_eq!(baseline.mapping_rows, 1);
    let data_dir = output("mapping-quarantine-data");
    std::fs::create_dir_all(data_dir.join("cache")).unwrap();
    std::fs::write(
        data_dir.join("cache/quarantine.json"),
        r#"{"records":[{
        "cache_file":"cache/labels/mondo.json", "record_locator":"MONDO:0000001"}]}"#,
    )
    .unwrap();
    let target = output("mapping-quarantine-release");
    let report = export(
        &atlas,
        &graph,
        &open(),
        &target,
        "fixture-v1",
        Quarantine::load(&data_dir).unwrap(),
    )
    .unwrap();
    assert_eq!(report.mapping_rows, 0);
    assert_eq!(report.quarantine.excluded_mapping_rows, 1);
    assert_eq!(report.excluded_disease_status["active"], 1);
    let mappings = std::fs::read_to_string(target.join("mappings/disease-identity.sssom.tsv")).unwrap();
    assert!(!mappings.contains("OMIM:1"));
}

#[test]
fn incomplete_retrieval_metadata_is_counted_and_never_fabricated() {
    let (atlas, graph) = fixture();
    let mut data = graph.data().clone();
    data.provenance.entities[0].retrieved_at = None;
    data.records[0].fetched_at = None;
    let path = output("missing-retrieval");
    let report = export(
        &atlas,
        &fixture_graph(data),
        &open(),
        &path,
        "fixture-v1",
        Quarantine::default(),
    )
    .unwrap();
    assert_eq!(report.exclusion_reasons["missing_retrieval_metadata"], 2);
    assert_eq!(report.excluded_nodes["paper"], 1);
    std::fs::remove_dir_all(path).unwrap();
}
