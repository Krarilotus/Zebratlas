mod common;
use atlas_ask::query::{
    conversation::QueryRequest,
    plan::{Direction, Filters, Hop, Pattern},
    schema::Lineage,
    *,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

fn card() -> SchemaCard {
    let mut predicates = BTreeMap::new();
    for p in [
        format!("{RDF}type"),
        format!("{RDFS}label"),
        format!("{RA}serves_gene"),
        format!("{RA}has_associated_gene"),
        format!("{RA}nodeKind"),
    ] {
        predicates.insert(p, Default::default());
    }
    SchemaCard {
        schema_version: 1,
        activity: json!({"synthetic":true}),
        graph: Lineage {
            source_url: "urn:test:synthetic".into(),
            retrieved_at: "2026-10-04T00:00:00Z".into(),
            version: "synthetic-v1".into(),
            sha256: "unverified-test-fixture".into(),
            record_locator: "tests/query.rs".into(),
        },
        statements: 0,
        classes: [(format!("{RA}Node"), "urn:test:node".into())].into(),
        predicates,
        known_absent: Default::default(),
        prefixes: Default::default(),
        semantic_units: Default::default(),
        provenance_pattern: String::new(),
    }
}
fn engine() -> QueryEngine {
    let (_, graph) = common::fixture();
    QueryEngine::new(card(), "http://127.0.0.1:1/dataset/sparql", Some(graph)).unwrap()
}
fn plan() -> QueryPlan {
    QueryPlan {
        version: 1,
        pattern: Pattern::Traverse,
        focus: vec!["HGNC:11444".into()],
        hops: vec![Hop {
            relation: "serves_gene".into(),
            direction: Direction::Incoming,
        }],
        filters: Filters::default(),
        output: None,
        limit: 10,
        reasoning: false,
    }
}
fn linked() -> Vec<LinkedEntity> {
    vec![LinkedEntity {
        id: "HGNC:11444".into(),
        label: "STXBP1".into(),
    }]
}

#[test]
fn deterministic_neighborhood_guard_preserves_actual_query_and_rejects_shape_escapes() {
    let prefix = "PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#>\nPREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n";
    let seed = iri("id", "HGNC:11444");
    let predicate = format!("<{RA}has_associated_gene>");
    let query = format!(
        "{prefix}SELECT DISTINCT ?s ?p ?o WHERE {{ {{ VALUES ?p {{ {predicate} }} {seed} ?p ?o . BIND({seed} AS ?s) }} UNION {{ VALUES ?p {{ {predicate} }} ?s ?p {seed} . BIND({seed} AS ?o) }} }} LIMIT 160"
    );
    assert_eq!(
        guard::checked(&query, &card(), 160).unwrap(),
        query,
        "an actual compiled receipt runs byte-for-byte"
    );
    let edited = query.replace(prefix, "").replace("LIMIT 160", "LIMIT 80");
    assert_eq!(
        guard::checked(&edited, &card(), 80).unwrap(),
        edited,
        "prefix formatting and explicit lower cap preserve the bounded algebra"
    );
    assert!(
        guard::checked(&query, &card(), 100).is_err(),
        "never silently change a compiled receipt's cap"
    );
    for unsafe_query in [
        query.replace("LIMIT 160", "LIMIT 10000"),
        query.replace("VALUES ?p", "VALUES ?unbound"),
        query.replace(&seed, "?unboundSeed"),
        query.replace(&predicate, &format!("<{RA}about_gene>")),
        query.replace("WHERE { ", "WHERE { SERVICE <http://127.0.0.1/admin> { ?s ?p ?o } "),
        query.replace("SELECT DISTINCT ?s ?p ?o", "DELETE"),
        format!("{query}\n{query}"),
    ] {
        assert!(
            guard::checked(&unsafe_query, &card(), 160).is_err(),
            "accepted unsafe template edit: {unsafe_query}"
        );
    }
    assert!(guard::checked("SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 160", &card(), 160).is_err());
}

#[test]
fn parsed_guard_blocks_writes_and_nested_escape_routes() {
    for q in [
        "INSERT DATA { <urn:s> <urn:p> <urn:o> }",
        "LOAD <https://example.test/data>",
        "CONSTRUCT { ?s ?p ?o } WHERE {?s ?p ?o}",
        "SELECT * FROM <https://example.test/data> WHERE {?s ?p ?o}",
        "SELECT * WHERE { SERVICE <http://127.0.0.1/admin> {?s ?p ?o} }",
        "SELECT * WHERE { FILTER EXISTS { SERVICE <https://example.test> {?s ?p ?o} } }",
        "SELECT * WHERE { BIND(EXISTS {SERVICE <https://example.test> {?s ?p ?o}} AS ?x) }",
        "SELECT * WHERE {?s <urn:invented-predicate> ?o}",
        "SELECT * WHERE {?s ?p ?o}",
        "SELECT * WHERE {?s a <urn:invented-class>}",
        "SELECT * WHERE {?s !<urn:p> ?o}",
        "SELECT (<urn:custom-function>(?s) AS ?x) WHERE {?s a <https://w3id.org/rare-disease-atlas/vocab#Node>}",
    ] {
        assert!(guard::checked(q, &card(), 10).is_err(), "accepted {q}");
    }
    let q = format!("{PREFIXES}SELECT ?s WHERE {{ ?s rdfs:label \"SERVICE INSERT DELETE\" }} LIMIT 9999");
    let checked = guard::checked(&q, &card(), 10).unwrap();
    assert!(checked.contains("LIMIT 11"));
}
#[test]
fn plans_reject_invented_ids_relations_and_ignored_filters() {
    let e = engine();
    let mut p = plan();
    p.focus = vec!["HGNC:invented".into()];
    assert!(p.validate(&linked(), &e.relations()).is_err());
    p = plan();
    p.hops[0].relation = "invented".into();
    assert!(p.validate(&linked(), &e.relations()).is_err());
    p = plan();
    p.filters.country = Some("Germany".into());
    assert!(p.compile_sparql(&linked(), &e.relations()).is_err());
    p = plan();
    p.pattern = Pattern::AssociatedConditions;
    assert!(p.validate(&linked(), &e.relations()).is_err());
}

#[test]
fn projected_metadata_filters_are_kept_and_checked_against_the_loaded_schema() {
    let mut card = card();
    for property in ["country", "kind", "status"] {
        card.predicates.insert(format!("{RA}{property}"), Default::default());
    }
    let observed = card.predicates.keys().cloned().collect();
    let engine = QueryEngine::new(card.clone(), "http://127.0.0.1:1/sparql", None).unwrap();
    let mut p = plan();
    p.filters = Filters {
        country: Some("Germany".into()),
        kind: Some("trial".into()),
        recruiting: Some(true),
    };
    p.output = Some(atlas_core::node::NodeKind::Study);
    assert!(
        p.compile_sparql(&linked(), &engine.relations()).is_err(),
        "old schema-less compiler cannot assume new fields"
    );
    let query = p
        .compile_sparql_with_metadata(&linked(), &engine.relations(), &observed)
        .unwrap();
    assert!(query.contains("ra:country \"Germany\""));
    assert!(query.contains("ra:kind \"trial\""));
    assert!(query.contains("ra:status ?status"));
    assert!(query.contains("ra:nodeKind \"study\""));
    assert!(guard::checked(&query, &card, 10).is_ok());
    let only_country = [format!("{RA}country")].into_iter().collect();
    assert!(
        p.compile_sparql_with_metadata(&linked(), &engine.relations(), &only_country)
            .is_err(),
        "kind must not be silently dropped"
    );
}

#[test]
fn power_loop_constants_must_be_prelinked_and_literals_are_not_iris() {
    let good = format!(
        "{PREFIXES}SELECT ?s WHERE {{ ?s ra:has_associated_gene {} }}",
        iri("id", "HGNC:11444")
    );
    assert!(guard::linked_entities(&good, &linked()).is_ok());
    let bad = good.replace("HGNC%3A11444", "HGNC%3Ainvented");
    assert!(guard::linked_entities(&bad, &linked()).is_err());
    let text = format!(
        "{PREFIXES}SELECT ?s WHERE {{?s rdfs:label {}}}",
        literal("<https://w3id.org/rare-disease-atlas/id/invented>")
    );
    assert!(guard::linked_entities(&text, &[]).is_ok());
}
#[test]
fn model_queries_require_mandatory_target_joins_in_every_branch() {
    let id = iri("id", "HGNC:11444");
    for body in [
        format!("VALUES ?gene {{ {id} }} ?org ra:nodeKind \"organisation\" OPTIONAL {{ ?org ra:serves_gene ?gene }}"),
        format!("VALUES ?wrongVariable {{ {id} }} ?org ra:serves_gene ?gene"),
        format!("{{ ?org ra:serves_gene {id} }} UNION {{ ?org ra:nodeKind \"organisation\" }}"),
        format!("VALUES ?gene {{ {id} }} ?gene rdfs:label ?geneLabel OPTIONAL {{?asset ra:nodeKind \"asset\"}}"),
    ] {
        let query = format!("{PREFIXES}SELECT * WHERE {{ {body} }}");
        assert!(
            guard::linked_entities(&query, &linked()).is_err(),
            "accepted unrelated rows: {body}"
        );
    }
    for body in [
        format!("VALUES ?gene {{ {id} }} ?org ra:serves_gene ?gene OPTIONAL {{?org rdfs:label ?label}}"),
        format!("VALUES ?focus {{ {id} }} ?org ra:serves_gene ?gene FILTER(?gene=?focus)"),
        format!("{{ ?org ra:serves_gene {id} }} UNION {{ ?condition ra:has_associated_gene {id} }}"),
    ] {
        assert!(
            guard::linked_entities(&format!("{PREFIXES}SELECT * WHERE {{ {body} }}"), &linked()).is_ok(),
            "rejected grounded rows: {body}"
        );
    }
}
#[test]
fn guard_rejects_resource_variable_reused_as_literal_identifier() {
    let mut card = card();
    card.predicates.insert(format!("{DCT}identifier"), Default::default());
    assert!(
        guard::checked(
            &format!("{PREFIXES}SELECT ?condition WHERE {{?condition dcterms:identifier ?condition}}"),
            &card,
            10
        )
        .is_err()
    );
    assert!(
        guard::checked(
            &format!("{PREFIXES}SELECT ?condition ?identifier WHERE {{?condition dcterms:identifier ?identifier}}"),
            &card,
            10
        )
        .is_ok()
    );
}
#[test]
fn projected_entity_identifiers_must_be_bound_without_rewriting_the_query() {
    let missing = format!(
        "{PREFIXES}SELECT ?resource ?label WHERE {{ ?study ra:serves_gene {} . OPTIONAL {{?study rdfs:label ?label}} }}",
        iri("id", "HGNC:11444")
    );
    let error = guard::checked(&missing, &card(), 10).unwrap_err();
    assert!(error.contains("?resource") && error.contains("never bound"));
    let bound = missing.replace("OPTIONAL", "BIND(?study AS ?resource) OPTIONAL");
    assert!(guard::checked(&bound, &card(), 10).is_ok());
    assert!(
        guard::checked(
            &format!(
                "{PREFIXES}SELECT (COUNT(?study) AS ?count) WHERE {{?study ra:serves_gene {}}}",
                iri("id", "HGNC:11444")
            ),
            &card(),
            10
        )
        .is_ok()
    );
}

#[tokio::test]
#[ignore = "requires hash-bound operational schema and actual local nrese endpoint; no model calls"]
async fn operational_source_chain_uses_bounded_constant_subject_lookups() {
    let path = std::env::var("ATLAS_QUERY_SCHEMA").expect("operational schema path");
    let card: SchemaCard = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let endpoint = std::env::var("ATLAS_QUERY_ENDPOINT").expect("actual local nrese endpoint");
    let engine = QueryEngine::new(card, endpoint, None).unwrap();
    let query = format!(
        "{PREFIXES}SELECT ?gene ?label WHERE {{ BIND({} AS ?gene) ?gene rdfs:label ?label }} LIMIT 1",
        iri("id", "HGNC:11444")
    );
    let result = engine.run_sparql(&query, 1, false).await.unwrap();
    assert_eq!(result.backend, "nrese");
    assert_eq!(result.data["results"]["bindings"][0]["label"]["value"], "STXBP1");
    assert!(
        !result.provenance.is_empty(),
        "actual node records must resolve without a broad provenance scan: {:?}",
        result.notes
    );
    assert!(
        result
            .provenance
            .iter()
            .any(|r| r["hash"]["value"].as_str().is_some_and(|s| s.len() == 64)
                && r["locator"]["value"].is_string()
                && r["url"]["value"].is_string()
                && r["sourceHash"]["value"].is_string())
    );
    assert!(
        result
            .notes
            .iter()
            .all(|s| !s.contains("unavailable") && !s.contains("deadline")),
        "{:?}",
        result.notes
    );
    eprintln!(
        "actual operational source chain: {} records in {}ms",
        result.provenance.len(),
        result.latency_ms
    );
}
#[tokio::test]
async fn fast_path_filters_and_edited_plans_do_not_call_model() {
    let e = engine();
    let (llm, mock) = common::mock_llm(&[]);
    let p = plan();
    let result = e.run_plan(&p, &linked()).await.unwrap();
    assert_eq!(result.backend, "rust");
    assert_eq!(result.data["results"]["bindings"].as_array().unwrap().len(), 1);
    let mut p = plan();
    p.filters.country = Some("Germany".into());
    let r = e.run_plan(&p, &linked()).await.unwrap();
    assert!(r.data["results"]["bindings"].as_array().unwrap().is_empty());
    let req = QueryRequest {
        linked: linked(),
        plan: Some(plan()),
        ..Default::default()
    };
    let answer = e.understand(&llm, &req, None).await.unwrap();
    assert!(answer.model_provenance.is_empty());
    assert!(mock.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn model_fills_plan_and_retries_semantically_invalid_focus() {
    let p = plan();
    let mut bad = serde_json::to_value(&p).unwrap();
    bad["focus"] = json!(["HGNC:invented"]);
    let (llm, mock) = common::mock_llm(&[bad, serde_json::to_value(&p).unwrap()]);
    let e = engine();
    let answer = e
        .understand(
            &llm,
            &QueryRequest {
                question: "Find my group".into(),
                linked: linked(),
                connection: Some("mock".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(answer.model_provenance.len(), 2);
    assert_eq!(answer.results[0].backend, "rust");
    assert!(mock.requests.lock().unwrap().iter().all(|r| r.schema.is_some()));
}
#[tokio::test]
async fn same_dispatch_used_by_connected_models_enforces_tool_arguments() {
    let e = engine();
    assert!(
        e.dispatch("run_sparql", json!({"query":"DELETE WHERE {?s ?p ?o}"}))
            .await
            .is_err()
    );
    assert!(
        e.dispatch("describe_schema", json!({"endpoint":"https://example.test"}))
            .await
            .is_err()
    );
    assert!(e.dispatch("sample", json!({"class":"urn:unknown"})).await.is_err());
    assert!(e.dispatch("describe_schema", json!({})).await.unwrap()["card"].is_string());
}
#[test]
fn schema_generation_is_strict_versioned_and_checks_input_hash() {
    let path = std::env::temp_dir().join(format!("atlas-query-synthetic-{}.ttl", std::process::id()));
    std::fs::write(
        &path,
        format!("<urn:test:s> a <{RA}Node>; <{RDFS}label> \"synthetic\" ."),
    )
    .unwrap();
    let mut source = card().graph;
    source.sha256.clear();
    let s = SchemaCard::generate(&path, source.clone()).unwrap();
    assert_eq!(s.statements, 2);
    assert_eq!(s.graph.sha256.len(), 64);
    assert!(
        s.predicates[&format!("{RDFS}label")]
            .observed_domain
            .contains(&format!("{RA}Node"))
    );
    source.sha256 = "wrong".into();
    assert!(SchemaCard::generate(&path, source).is_err());
    std::fs::write(&path, "not turtle").unwrap();
    assert!(SchemaCard::generate(&path, card().graph).is_err());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn http_executor_enforces_row_and_byte_caps() {
    use axum::{Json, Router, routing::post};
    let data = json!({"head":{"vars":["label"]},"results":{"bindings":(0..12).map(|n|json!({"label":{"type":"literal","value":n.to_string()}})).collect::<Vec<_>>()}});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/sparql",
        post(move || {
            let data = data.clone();
            async move { Json(data) }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let e = QueryEngine::new(card(), format!("http://{addr}/sparql"), None).unwrap();
    let query = format!("{PREFIXES}SELECT ?s WHERE {{?s rdfs:label ?label}}");
    let r = e.run_sparql(&query, 10, false).await.unwrap();
    assert!(r.truncated);
    assert_eq!(r.data["results"]["bindings"].as_array().unwrap().len(), 10);
    assert_eq!(r.lineage.sha256.len(), 64);
    server.abort();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route("/sparql", post(|| async { "x".repeat(1024 * 1024 + 1) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let e = QueryEngine::new(card(), format!("http://{addr}/sparql"), None).unwrap();
    assert!(e.run_sparql(&query, 10, false).await.unwrap_err().contains("byte cap"));
    server.abort();
}

#[tokio::test]
async fn mcp_initialization_notifications_and_tool_errors() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let (llm, _) = common::mock_llm(&[]);
    let (atlas, graph) = common::fixture();
    let app = routes::router(routes::QueryState {
        engine: Arc::new(engine()),
        asker: atlas_ask::Asker::new(atlas, graph, llm),
    });
    for (rpc, status) in [
        (
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            StatusCode::OK,
        ),
        (
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            StatusCode::ACCEPTED,
        ),
        (json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), StatusCode::OK),
        (
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"run_sparql","arguments":{"query":"DELETE WHERE {?s ?p ?o}"}}}),
            StatusCode::OK,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/ask/query/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(rpc.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if rpc["id"] == 3 {
            let v: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
            assert_eq!(v["result"]["isError"], true);
        }
    }
}

#[tokio::test]
async fn search_boundary_uses_graph_ids_redacted_input_and_connected_provider() {
    use atlas_ask::query::boundary::{QueryConnection, SearchQuery};
    let (atlas, graph) = common::fixture();
    let (llm, mock) = common::mock_llm(&[serde_json::to_value(plan()).unwrap()]);
    let asker = atlas_ask::Asker::new(atlas, graph, llm);
    let ids = vec!["HGNC:11444".to_owned()];
    let prepared = atlas_intake::prepare(
        atlas_intake::Input::Paste("Contact: maria@example.test\nFind a group for STXBP1"),
        &Default::default(),
    )
    .unwrap();
    let answer = asker
        .understand_search_query(
            &engine(),
            SearchQuery {
                prepared: &prepared,
                linked_ids: &ids,
                lang: "de",
                input_activity_id: "activity:synthetic-redaction",
                plan: None,
                power_mode: false,
            },
            QueryConnection {
                connection: Some("mock".into()),
                model: Some("connected-model".into()),
                key: Some(atlas_llm::ApiKey::new("synthetic-key")),
                visitor: Some("synthetic-visitor".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(answer.semantic_focus, ids);
    assert_eq!(
        answer.answer.results[0].data["results"]["bindings"][0]["label"]["value"],
        "STXBP1 Foundation"
    );
    assert_eq!(answer.activity["prov:used"], "activity:synthetic-redaction");
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model.as_deref(), Some("connected-model"));
    assert!(requests[0].messages[1].content.contains("[EMAIL]"));
    assert!(!requests[0].messages[1].content.contains("maria@example.test"));
    assert!(requests[0].messages[1].content.contains("STXBP1"));
    assert!(mock.keys.lock().unwrap()[0].is_some());
    let sent = serde_json::to_string(&answer.answer).unwrap();
    assert!(!sent.contains("synthetic-key"));
    assert!(!sent.contains("maria@example.test"));
}

#[tokio::test]
async fn combined_router_reruns_verified_edits_and_rejects_unlinked_entities_without_model() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let (atlas, graph) = common::fixture();
    let (llm, mock) = common::mock_llm(&[]);
    let app = atlas_ask::router(atlas_ask::AskState::new(atlas, graph, llm).with_query_engine(engine()));
    let request = |body: serde_json::Value| {
        Request::post("/api/ask/query")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(request(
            json!({"question":"", "linked":[{"id":"HGNC:11444","label":"invented"}],"plan":plan()}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body["model_provenance"].as_array().unwrap().is_empty());
    let mut edited = plan();
    edited.filters.country = Some("DE".into());
    let response = app
        .clone()
        .oneshot(request(json!({"question":"", "linked":linked(),"plan":edited})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(
        body["results"][0]["data"]["results"]["bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for body in [
        json!({"question":"call the model", "linked":linked()}),
        json!({"question":"", "linked":[{"id":"STXBP1","label":"STXBP1"}],"plan":plan()}),
        json!({"question":"", "linked":[{"id":"HGNC:invented","label":"STXBP1"}],"plan":plan()}),
    ] {
        let response = app.clone().oneshot(request(body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(error["detail_msg"]["key"], "ask.error.generic");
        assert_eq!(error["detail"], error["detail_msg"]["fallback"]);
        assert!(error["diagnostic"].is_string());
    }
    assert!(mock.requests.lock().unwrap().is_empty());
    assert_eq!(
        app.oneshot(Request::get("/api/ask/schema").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[test]
fn gene_record_is_compiled_from_linked_id_not_display_label() {
    let e = engine();
    let mut p = plan();
    p.pattern = Pattern::GeneRecord;
    p.hops.clear();
    let links = vec![LinkedEntity {
        id: "HGNC:11444".into(),
        label: "arbitrary client label".into(),
    }];
    let q = p.compile_sparql(&links, &e.relations()).unwrap();
    assert!(q.contains(&format!("VALUES ?gene {{ {} }}", iri("id", "HGNC:11444"))));
    assert!(!q.contains("arbitrary client label"));
}

fn sourced_graph() -> Arc<atlas_core::Graph> {
    use atlas_core::{
        graph::{RecordHash, SourceRecord},
        provenance::{Activity, EntityIdx, Locator, SourceEntity},
    };
    let (_, graph) = common::fixture();
    let mut data = graph.data().clone();
    data.provenance.add_entity(SourceEntity {
        id: "source:synthetic".into(),
        url: "https://example.test/synthetic".into(),
        file: "synthetic.json".into(),
        version: Some("synthetic-v1".into()),
        retrieved_at: Some("2026-10-04T00:00:00Z".into()),
        sha256: Some("aa".repeat(32)),
        ..Default::default()
    });
    data.provenance.add_activity(Activity {
        id: "activity:synthetic".into(),
        ..Default::default()
    });
    data.records.push(SourceRecord {
        entity: EntityIdx(0),
        locator: Locator::Line(7),
        id: "synthetic-record".into(),
        url: Some("https://example.test/synthetic/record".into()),
        fetched_at: None,
        hash: RecordHash::CanonicalJson,
        sha256: [0xbb; 32],
    });
    data.orgs[0].records.push(0);
    data.edges[0].records.push(0);
    Arc::new(atlas_core::Graph::new(data))
}

#[tokio::test]
async fn query_card_suggestions_use_only_indexed_properties_and_source_proofs() {
    use atlas_ask::query::suggestions::SuggestionRequest;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let (atlas, _) = common::fixture();
    let graph = sourced_graph();
    let (llm, mock) = common::mock_llm(&[]);
    let asker = atlas_ask::Asker::new(atlas, graph.clone(), llm.clone());
    let request = SuggestionRequest {
        focus: "HGNC:11444".into(),
        ..Default::default()
    };
    let suggestions = asker.query_suggestions(&request).unwrap();
    assert!(suggestions.counts_exact);
    assert_eq!(suggestions.properties.len(), 1);
    let p = &suggestions.properties[0];
    assert_eq!(p["hop"], json!({"relation":"serves_gene","direction":"incoming"}));
    assert_eq!(p["distinct_targets"], 1);
    assert_eq!(p["outputs"], json!([{"kind":"organisation","count":1}]));
    assert_eq!(p["samples"][0]["node"]["label"], "STXBP1 Foundation");
    assert_eq!(p["samples"][0]["evidence"][0]["record_locator"], "L7");
    assert_eq!(p["samples"][0]["evidence"][0]["sha256"], "bb".repeat(32));
    assert_eq!(suggestions.activity["prov:used"][0]["sha256"], "aa".repeat(32));
    assert_eq!(
        suggestions.activity,
        asker.query_suggestions(&request).unwrap().activity
    );
    for focus in ["STXBP1", "HGNC:invented", &graph.edges()[0].id()] {
        assert!(
            asker
                .query_suggestions(&SuggestionRequest {
                    focus: focus.into(),
                    ..Default::default()
                })
                .is_err()
        );
    }
    let filtered = asker
        .query_suggestions(&SuggestionRequest {
            relation: Some("invented".into()),
            ..request
        })
        .unwrap();
    assert!(filtered.properties.is_empty());
    let engine = QueryEngine::new(card(), "http://127.0.0.1:1/sparql", Some(graph.clone())).unwrap();
    let app = atlas_ask::router(atlas_ask::AskState::new(asker.atlas.clone(), graph, llm).with_query_engine(engine));
    let response = app
        .oneshot(
            Request::post("/api/ask/query/suggestions")
                .header("content-type", "application/json")
                .body(Body::from(json!({"focus":"HGNC:11444"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["properties"][0]["hop"]["relation"], "serves_gene");
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[test]
fn query_card_suggestions_page_properties_bound_samples_and_hide_withheld_nodes() {
    use atlas_ask::query::suggestions::SuggestionRequest;
    use atlas_core::graph::{Quarantine, Relation};
    let (atlas, _) = common::fixture();
    let mut data = sourced_graph().data().clone();
    for relation in Relation::ALL.iter().take(7) {
        let mut edge = data.edges[0].clone();
        edge.relation = *relation;
        data.edges.push(edge);
    }
    let (llm, _) = common::mock_llm(&[]);
    let asker = atlas_ask::Asker::new(
        atlas.clone(),
        Arc::new(atlas_core::Graph::new(data.clone())),
        llm.clone(),
    );
    let req = SuggestionRequest {
        focus: "HGNC:11444".into(),
        ..Default::default()
    };
    let first = asker.query_suggestions(&req).unwrap();
    assert_eq!(first.properties.len(), 5);
    assert_eq!(first.next_offset, Some(5));
    let second = asker
        .query_suggestions(&SuggestionRequest { offset: 5, ..req })
        .unwrap();
    assert_eq!(second.properties.len(), 3);
    assert_eq!(second.next_offset, None);
    data.quarantine.push(Quarantine {
        record: 0,
        reason: "synthetic-withheld".into(),
    });
    let asker = atlas_ask::Asker::new(atlas.clone(), Arc::new(atlas_core::Graph::new(data)), llm.clone());
    let req = SuggestionRequest {
        focus: "HGNC:11444".into(),
        ..Default::default()
    };
    assert!(asker.query_suggestions(&req).unwrap().properties.is_empty());
    let mut data = sourced_graph().data().clone();
    let org_template = data.orgs[0].clone();
    let edge_template = data.edges[0].clone();
    data.orgs.clear();
    data.edges.clear();
    for i in 0..5001 {
        let mut org = org_template.clone();
        org.id = format!("org:synthetic-{i:05}");
        let mut edge = edge_template.clone();
        edge.from = org.id.clone();
        data.orgs.push(org);
        data.edges.push(edge);
    }
    let asker = atlas_ask::Asker::new(atlas, Arc::new(atlas_core::Graph::new(data)), llm);
    let result = asker.query_suggestions(&req).unwrap();
    assert!(!result.counts_exact);
    assert_eq!(result.scanned_assertions, 5000);
    assert_eq!(result.properties[0]["distinct_targets"], 5000);
    assert_eq!(result.properties[0]["samples"].as_array().unwrap().len(), 3);
    assert!(
        asker
            .query_suggestions(&SuggestionRequest { offset: 1001, ..req })
            .is_err()
    );
}

#[tokio::test]
async fn results_preserve_exact_record_provenance_and_withheld_focus_is_rejected() {
    use atlas_core::graph::Quarantine;
    let (atlas, _) = common::fixture();
    let graph = sourced_graph();
    let e = QueryEngine::new(card(), "http://127.0.0.1:1/sparql", Some(graph.clone())).unwrap();
    let result = e.run_plan(&plan(), &linked()).await.unwrap();
    assert_eq!(
        result.provenance[0]["source_url"],
        "https://example.test/synthetic/record"
    );
    assert_eq!(result.provenance[0]["sha256"], "bb".repeat(32));
    assert_eq!(result.provenance[0]["source_sha256"], "aa".repeat(32));
    assert_eq!(result.provenance[0]["version"], "synthetic-v1");
    assert_eq!(result.provenance[0]["hash_scope"], "canonical_json");
    assert_eq!(result.provenance[0]["record_locator"], "L7");
    let mut data = graph.data().clone();
    data.quarantine.push(Quarantine {
        record: 0,
        reason: "synthetic-withheld".into(),
    });
    let graph = Arc::new(atlas_core::Graph::new(data));
    let (llm, _) = common::mock_llm(&[]);
    let asker = atlas_ask::Asker::new(atlas, graph.clone(), llm);
    assert!(asker.link_query_entities(&["org:stxbp1-foundation".into()]).is_err());
    assert!(asker.link_query_entities(&[graph.edges()[0].id()]).is_err());
    let e = QueryEngine::new(card(), "http://127.0.0.1:1/sparql", Some(graph)).unwrap();
    assert!(
        e.run_plan(&plan(), &linked()).await.unwrap().data["results"]["bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
