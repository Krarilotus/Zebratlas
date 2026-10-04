use super::initiatives::{for_condition, for_gene, select};
use atlas_core::disease::Disease;
use atlas_core::evidence::GeneLink;
use atlas_core::graph::{
    GraphData, GraphEdge, Initiative, InitiativeScope, LinkLevel, OfficialAction, OrgKind, Organisation, Quarantine,
    Relation,
};
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{ActivityIdx, EntityIdx, Provenance, RecordRef};
use atlas_core::{Atlas, DiseaseIdentity, Graph, Term};

const DRAGONFLY_SOURCE: &str = "https://www.scn2a.org/research/clinical-trials-and-research-opportunities-2/";

fn atlas() -> Atlas {
    let mut diseases = vec![];
    for (id, symbol, hgnc) in [
        ("MONDO:0012812", "STXBP1", "HGNC:11444"),
        ("MONDO:0019990", "SNAP25", "HGNC:11132"),
    ] {
        let mut d = Disease::new(id, ActivityIdx(0));
        d.name = format!("{symbol} condition");
        d.rare = true;
        d.genes.push(GeneLink {
            symbol: symbol.into(),
            hgnc: Some(hgnc.into()),
            ncbi_gene: None,
            association: "Disease-causing germline mutation(s) in".into(),
            source: "fixture".into(),
            source_disease: id.into(),
            pmids: vec![],
            assessed: Some(true),
            record: RecordRef::line(EntityIdx(0), 1),
        });
        diseases.push(d);
    }
    Atlas::new(
        vec![Term {
            id: "HP:0000001".into(),
            name: "All".into(),
            ..Term::default()
        }],
        DiseaseIdentity::default(),
        Provenance::default(),
        diseases,
    )
}

fn initiative(data: &mut GraphData, id: &str, broad: bool) {
    data.orgs.push(Organisation {
        id: id.into(),
        name: id.into(),
        kind: OrgKind::Other,
        url: Some("https://example.org/".into()),
        contact_url: None,
        country: None,
        country_basis: None,
        description: Some("Connects families with research.".into()),
        languages: vec!["en".into()],
        verified_on: None,
        channels: vec![],
        records: vec![],
    });
    data.initiatives.push(Initiative {
        id: id.into(),
        verified: true,
        scopes: if broad {
            vec![InitiativeScope {
                kind: "all_rare_diseases".into(),
                ..InitiativeScope::default()
            }]
        } else {
            vec![]
        },
        actions: vec![OfficialAction {
            action: "Check participation steps".into(),
            url: "https://example.org/research/".into(),
            audience: "families".into(),
            retrieved_at: "2026-10-04T00:00:00Z".into(),
            ..OfficialAction::default()
        }],
        ..Initiative::default()
    });
}

fn edge(from: &str, to: &str, relation: Relation) -> GraphEdge {
    GraphEdge {
        from: from.into(),
        to: to.into(),
        relation,
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        reason: "Exact publisher scope".into(),
        activity: ActivityIdx(0),
        records: vec![],
    }
}

#[test]
fn exact_gene_support_wins_over_general_and_is_bounded() {
    let atlas = atlas();
    let mut data = GraphData::default();
    initiative(&mut data, "ecosystem:general", true);
    for id in [
        "ecosystem:simons-searchlight",
        "ecosystem:a",
        "ecosystem:b",
        "ecosystem:c",
    ] {
        initiative(&mut data, id, false);
        data.edges.push(edge(id, "HGNC:11444", Relation::ServesGene));
    }
    let g = Graph::new(data);
    let v = for_condition(&atlas, &g, atlas.disease_idx("MONDO:0012812").unwrap());
    assert_eq!(v.len(), 3);
    assert!(v.iter().all(|r| r["scope"] == "gene"));
    assert!(v.iter().all(|r| r["id"] != "ecosystem:general"));
    let gene = for_gene(&atlas, &g, atlas.gene("SNAP25").unwrap());
    assert_eq!(gene[0]["scope"], "all_rare_diseases");
    assert!(select(&g, &[], false).is_empty());
}

#[test]
fn condition_scope_precedes_gene_and_duplicate_links_make_one_card() {
    let atlas = atlas();
    let mut data = GraphData::default();
    initiative(&mut data, "ecosystem:gene", false);
    initiative(&mut data, "ecosystem:condition", false);
    data.edges
        .push(edge("ecosystem:gene", "HGNC:11444", Relation::ServesGene));
    data.edges
        .push(edge("ecosystem:condition", "HGNC:11444", Relation::ServesGene));
    data.edges
        .push(edge("ecosystem:condition", "MONDO:0012812", Relation::ServesCondition));
    let graph = Graph::new(data);
    let v = for_condition(&atlas, &graph, atlas.disease_idx("MONDO:0012812").unwrap());
    assert_eq!(v.len(), 2);
    assert_eq!(v[0]["id"], "ecosystem:condition");
    assert_eq!(v[0]["scope"], "condition");
    assert_eq!(v[0]["links"].as_array().unwrap().len(), 2);
    assert_eq!(v[0]["official_action"]["date"], "2026-10-04");
}

#[test]
fn quarantined_action_is_never_returned() {
    let mut data = GraphData::default();
    initiative(&mut data, "ecosystem:hidden", true);
    data.initiatives[0].actions[0].records = vec![0];
    data.quarantine.push(Quarantine {
        record: 0,
        reason: "removed source route".into(),
    });
    let graph = Graph::new(data);
    assert!(select(&graph, &[], true).is_empty());
}

#[test]
fn reported_forms_stay_inactive_and_quarantine_still_applies() {
    use atlas_core::graph::ReportedAction;
    let mut data = GraphData::default();
    initiative(&mut data, "ecosystem:reported", false);
    data.initiatives[0].verified = false;
    data.initiatives[0].exclusion_reason = Some("direct HTTP 403".into());
    data.initiatives[0].reported_actions = vec![ReportedAction {
        url: "https://example.org/reported".into(),
        verification: "founder-reported; original email not supplied".into(),
        destination_status: "blocked".into(),
        records: vec![0],
        ..ReportedAction::default()
    }];
    let graph = Graph::new(data.clone());
    assert!(select(&graph, &[], true).is_empty());
    let refs = super::initiatives::references(&graph);
    assert!(refs[0]["official_action"].is_null());
    assert_eq!(refs[0]["reported_actions"][0]["active"], false);
    assert_eq!(refs[0]["reported_actions"][0]["destination_status"], "blocked");
    data.quarantine.push(Quarantine {
        record: 0,
        reason: "removed reference".into(),
    });
    let refs = super::initiatives::references(&Graph::new(data));
    assert!(refs[0]["reported_actions"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires RARE_ATLAS_DATA ecosystem cache and RARE_ATLAS_REFERENCE_DATA atlas snapshot"]
async fn acquired_partner_promises_reach_the_api_with_honest_verification_status() {
    use atlas_analytics::{Matcher, ScoringParams};
    use atlas_core::provenance::Agent;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;
    let path = std::env::var_os("RARE_ATLAS_DATA").expect("set RARE_ATLAS_DATA to acquired ecosystem cache");
    let path = std::path::PathBuf::from(path);
    let reference_data = std::env::var_os("RARE_ATLAS_REFERENCE_DATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| path.clone());
    let atlas = Arc::new(
        atlas_core::snapshot::load(&reference_data.join("cache/atlas.snapshot"))
            .expect("acquired atlas snapshot")
            .0,
    );
    let filter = atlas_ingest::withhold::load(&path, atlas_ingest::withhold::salt_from_env()).unwrap();
    let mut builder = atlas_ingest::graph::builder::Builder::new(
        &atlas,
        Agent {
            name: "ecosystem-http-test".into(),
            version: "1".into(),
            commit: None,
        },
    );
    atlas_ingest::graph::ecosystem::ingest(&mut builder, &path, &filter).unwrap();
    let mut graph = Graph::new(builder.data);
    graph.set_withhold(filter);
    let graph = Arc::new(graph);
    let mut condition_urls = vec![];
    for symbol in ["STXBP1", "SNAP25"] {
        let gene = atlas.gene(symbol).unwrap();
        let values = for_gene(&atlas, &graph, gene);
        let s = values
            .iter()
            .find(|r| r["id"] == "ecosystem:simons-searchlight")
            .expect("partner promise");
        assert_eq!(
            s["official_action"]["url"],
            "https://www.simonssearchlight.org/research/"
        );
        assert_eq!(s["scope"], "gene");
        assert_eq!(s["official_action"]["source"]["sha256"].as_str().unwrap().len(), 64);
        let d = *atlas
            .gene_at(gene)
            .diseases
            .iter()
            .find(|&&d| {
                let dis = atlas.disease_at(d);
                dis.rare && dis.is_active() && dis.genes.iter().any(|l| l.symbol == symbol && l.is_causal())
            })
            .expect("real rare causal condition");
        let id = crate::nodes::encode(&atlas.disease_at(d).id);
        condition_urls.extend([
            format!("/api/condition/{id}"),
            format!("/api/condition/{id}/connections"),
            format!("/api/condition/{id}/jobs"),
            format!("/api/disease/{id}"),
        ]);
        assert!(
            for_condition(&atlas, &graph, d)
                .iter()
                .any(|r| r["id"] == "ecosystem:simons-searchlight")
        );
    }
    for (symbol, required) in [
        (
            "SCN2A",
            vec!["ecosystem:nord-iamrare-dragonfly", "ecosystem:czi-rare-as-one"],
        ),
        ("KCNQ2", vec!["ecosystem:czi-rare-as-one"]),
    ] {
        let gene = atlas.gene(symbol).unwrap();
        for id in required {
            assert!(
                for_gene(&atlas, &graph, gene).iter().any(|r| r["id"] == id),
                "{symbol}: {id}"
            );
        }
    }
    let state = crate::routes::AppState {
        search: Arc::new(atlas_core::search::domain::Index::build(&atlas, &graph)),
        questions: Arc::new(Default::default()),
        matcher: Arc::new(Matcher::new(atlas.clone(), ScoringParams::default())),
        atlas: atlas.clone(),
        graph: graph.clone(),
        data: Arc::new(path.clone()),
        llm: crate::llm::LlmState {
            llm: None,
            runtime: Arc::new(crate::llm::Runtime::default()),
        },
        integrity: Arc::new(OnceLock::new()),
        related: Arc::new(OnceLock::new()),
        clusters: Arc::new(OnceLock::new()),
        units: Arc::new(OnceLock::new()),
        query_engine: Arc::new(OnceLock::new()),
        query_suggestions: Arc::new(atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph)),
        explore_index: Arc::new(OnceLock::new()),
        withhold: crate::privacy::WithholdState::new(path, graph),
    };
    // Exercise the production assembly without opening any shared account/device store.
    let temp = tempfile::tempdir().unwrap();
    let router = crate::routes::assembled(
        state,
        Router::new(),
        Router::new(),
        Some(crate::connector::Config {
            accounts: atlas_accounts::AccountsConfig::for_tests(),
            devices: Arc::new(atlas_connector::store::Store::open(&temp.path().join("devices.sqlite")).unwrap()),
            conversations: None,
            public_origin: "https://example.invalid".into(),
        }),
    );
    let mut urls: Vec<String> = [
        "/api/gene/STXBP1",
        "/api/gene/HGNC%3A11132",
        "/api/gene/SNAP25/jobs",
        "/api/gene/SCN2A",
        "/api/gene/KCNQ2/jobs",
        "/api/initiatives",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    urls.extend(condition_urls);
    for url in urls {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(&url).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let items = if url == "/api/initiatives" {
            &body["initiatives"]
        } else {
            &body["already_working_on_this"]
        };
        if url == "/api/initiatives" {
            for id in ["ecosystem:nord", "ecosystem:nord-iamrare", "ecosystem:every-cure"] {
                let r = body["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == id)
                    .unwrap();
                assert_eq!(r["verification"], "unverified");
                assert!(r["official_action"].is_null());
            }
            let cure = body["references"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == "ecosystem:every-cure")
                .unwrap();
            for url in [
                "https://everycure.org/disease-interest-form/",
                "https://everycure.org/ideas",
            ] {
                let route = cure["reported_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["url"] == url)
                    .unwrap();
                assert_eq!(route["active"], false);
                assert_eq!(route["destination_status"], "blocked");
                assert_eq!(route["source"]["sha256"].as_str().unwrap().len(), 64);
            }
        }
        if url.contains("SCN2A") || url.contains("KCNQ2") {
            assert!(
                items
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["id"] == "ecosystem:czi-rare-as-one")
            );
            if url.contains("SCN2A") {
                let dragonfly = items
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == "ecosystem:nord-iamrare-dragonfly")
                    .unwrap();
                assert_eq!(
                    dragonfly["official_action"]["url"],
                    crate::initiatives_tests::DRAGONFLY_SOURCE
                );
            }
            continue;
        }
        let searchlight = items
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "ecosystem:simons-searchlight")
            .unwrap();
        assert_eq!(
            searchlight["official_action"]["url"],
            "https://www.simonssearchlight.org/research/"
        );
        assert!(
            !items
                .to_string()
                .contains("research.simonssearchlight.org/account/create")
        );
    }
    let missing = router
        .oneshot(
            Request::builder()
                .uri("/api/gene/NOT_A_GENE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(missing.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(body["detail_msg"]["key"], "api.error.gene_unknown");
    assert_eq!(body["detail_msg"]["params"]["id"], "NOT_A_GENE");
}
