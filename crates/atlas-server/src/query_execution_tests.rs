use crate::test_support::{call, state};
use atlas_ask::query::{QueryEngine, SchemaCard, schema::Lineage};
use atlas_llm::{ApiKey, Cache, CacheMode, FreeTierConfig, HOSTED_FREE, Llm, Registry};
use axum::Router;
use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path_regex};

fn plan() -> Value {
    json!({"version":1,"pattern":"traverse","focus":["MONDO:9999999"],
        "hops":[{"relation":"studies_condition","direction":"incoming"}],
        "filters":{"country":"Germany","recruiting":true,"kind":null},
        "output":"study","limit":10,"reasoning":false})
}

#[tokio::test]
async fn submitted_private_search_runs_linked_plan_and_main_rerun_needs_no_model() {
    let server = MockServer::start().await;
    Mock::given(path_regex(".*")).respond_with(|request: &Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let value = if body["response_format"]["json_schema"]["name"] == "atlas_query_plan" { plan() } else {
            json!({"terms":[{"kind":"condition","text":"Synthetic STXBP1 condition","english":"","gene":"","hgvs":"","negated":false}],"references":[],"intent":"find_study"})
        };
        ResponseTemplate::new(200).set_body_json(json!({"id":"synthetic","model":"fixture-model",
            "choices":[{"message":{"role":"assistant","content":value.to_string()},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}))
    }).mount(&server).await;
    let temp = tempfile::tempdir().unwrap();
    let mut registry = Registry::from_toml(&format!("replace_defaults=true\n[[connections]]\nname='hosted-free'\nkind='openai-compatible'\nbase_url='{}/v1'\ndefault_model='openai/gpt-oss-120b'\nkey_required=true\nfree_tier=true\n", server.uri()), false).unwrap();
    registry.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            server_key: Some(ApiKey::new("fixture")),
            spend_file: None,
            ..Default::default()
        },
    );
    let mut s = state();
    s.llm.llm = Some(Arc::new(Llm::new(
        registry,
        Cache::new(temp.path().join("private-cache"), CacheMode::ReadWrite),
    )));
    let rdf = temp.path().join("fixture.ttl");
    std::fs::write(&rdf, "@prefix ra: <https://w3id.org/rare-disease-atlas/vocab#> .\n<urn:study> a ra:Study; ra:studies_condition <urn:condition> .\n<urn:condition> a ra:Disease .").unwrap();
    let card = SchemaCard::generate(
        &rdf,
        Lineage {
            source_url: "https://example.invalid/fixture".into(),
            retrieved_at: "2026-10-04T00:00:00Z".into(),
            version: "synthetic-v1".into(),
            sha256: String::new(),
            record_locator: "fixture.ttl".into(),
        },
    )
    .unwrap();
    let engine = QueryEngine::new(card, "http://127.0.0.1:1/sparql", Some(s.graph.clone())).unwrap();
    assert!(s.query_engine.set(Ok(Some(Arc::new(engine)))).is_ok());
    let app = crate::routes::assembled(s.clone(), Router::new(), Router::new(), None);
    let (status, answer) = call(app.clone(), "POST", "/api/search", Some(json!({"q":"Patient: Alice Example\nEmail: alice@example.invalid\nFind trials for Synthetic STXBP1 condition", "lang":"en"}))).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{answer}");
    let execution = &answer["query_execution"];
    assert_eq!(execution["status"], "executed", "{answer}");
    assert_eq!(execution["answer"]["plan"], plan());
    assert_eq!(execution["linked"][0]["id"], "MONDO:9999999");
    assert_eq!(execution["linked"][0]["kind"], "disease");
    let result = &execution["answer"]["results"][0];
    assert_eq!(result["backend"], "rust");
    assert!(result["query"].is_null()); // never claim a SPARQL preview was executed
    assert!(!result["provenance"].as_array().unwrap().is_empty());
    assert!(result.to_string().contains("NCTTEST0"));
    assert!(!result.to_string().contains("NCTTEST1"));
    let mut edited = plan();
    edited["filters"]["country"] = json!("France");
    edited["filters"]["recruiting"] = Value::Null;
    let (status, rerun) = call(app, "POST", "/api/ask/query", Some(json!({"question":"","linked":execution["linked"].as_array().unwrap().iter().map(|n|json!({"id":n["id"],"label":n["label"]})).collect::<Vec<_>>(),"plan":edited}))).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{rerun}");
    assert!(rerun["results"].to_string().contains("NCTTEST1"));
    assert!(!rerun["results"].to_string().contains("NCTTEST0"));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests {
        let sent = String::from_utf8(request.body).unwrap();
        assert!(!sent.contains("Alice"));
        assert!(!sent.contains("alice@example"));
    }
    assert!(
        s.llm
            .llm
            .as_ref()
            .unwrap()
            .cache()
            .dir()
            .read_dir()
            .map_or(true, |mut d| d.next().is_none())
    );
}
