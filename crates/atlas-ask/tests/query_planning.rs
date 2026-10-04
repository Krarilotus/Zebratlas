//! Scripted provider and loopback dataset only: no hosted/paid model calls.
mod common;
use atlas_ask::query::{conversation::QueryRequest, schema::Lineage, *};
use axum::{Json, Router, routing::post};
use serde_json::json;

fn diagnostic_engine() -> QueryEngine {
    QueryEngine::new(
        SchemaCard {
            schema_version: 1,
            activity: json!({}),
            graph: Lineage {
                source_url: "urn:synthetic".into(),
                retrieved_at: "fixture".into(),
                version: "synthetic-v1".into(),
                sha256: "a".repeat(64),
                record_locator: "tests/query_planning.rs".into(),
            },
            statements: 0,
            classes: Default::default(),
            predicates: Default::default(),
            known_absent: Default::default(),
            prefixes: Default::default(),
            semantic_units: Default::default(),
            provenance_pattern: String::new(),
        },
        "http://127.0.0.1:1/sparql",
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn exhausted_failed_tools_preserve_receipts_and_do_not_establish_no_records() {
    let bad = json!({"action":"call","tool":"run_sparql","arguments":{"query":"DELETE WHERE {?s ?p ?o}"}});
    let (llm, mock) = common::mock_llm(&[bad]);
    let answer = diagnostic_engine()
        .understand(
            &llm,
            &QueryRequest {
                question: "Find patient groups".into(),
                power_mode: true,
                connection: Some("mock".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert!(answer.results.is_empty());
    assert_eq!(answer.model_provenance.len(), 6);
    assert_eq!(mock.requests.lock().unwrap().len(), 6);
    assert_eq!(
        answer
            .tool_trace
            .iter()
            .filter(|step| step["tool"] == "run_sparql" && step["output"]["error"].is_string())
            .count(),
        6
    );
    let terminal = answer.tool_trace.last().unwrap();
    assert_eq!(terminal["stage"], "query_status");
    assert_eq!(terminal["status"], "not_executed");
    assert_eq!(terminal["reason"], "budget_exhausted");
    assert_eq!(terminal["successful_queries"], 0);
    let coverage = &answer.tool_trace[answer.tool_trace.len() - 2];
    assert_eq!(coverage["items"][0]["status"], "not_checked");
    assert!(
        coverage["items"][0]["limitation"]
            .as_str()
            .unwrap()
            .contains("Failed attempts do not establish")
    );
}

#[tokio::test]
async fn exhausted_invalid_model_steps_are_retained_as_failures() {
    let (llm, _) = common::mock_llm(&[json!({"invented_action":"claim success"})]);
    let answer = diagnostic_engine()
        .understand(
            &llm,
            &QueryRequest {
                power_mode: true,
                connection: Some("mock".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert!(answer.results.is_empty());
    assert_eq!(answer.model_provenance.len(), 6);
    assert_eq!(
        answer
            .tool_trace
            .iter()
            .filter(|step| step["stage"] == "tool_step_validation")
            .count(),
        6
    );
    assert_eq!(answer.tool_trace.last().unwrap()["status"], "not_executed");
}

#[tokio::test]
async fn initial_provider_rejection_remains_an_error_without_fake_receipts() {
    let (llm, mock) = common::quota_llm();
    let error = diagnostic_engine()
        .understand(
            &llm,
            &QueryRequest {
                power_mode: true,
                connection: Some("mock".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap_err();
    assert!(error.contains("quota"));
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
}

#[derive(Debug)]
struct StopAfterOne {
    first: serde_json::Value,
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl atlas_llm::provider::Provider for StopAfterOne {
    fn kind(&self) -> atlas_llm::provider::ProviderKind {
        atlas_llm::provider::ProviderKind::OpenAiCompatible
    }
    fn base_url(&self) -> Option<&str> {
        Some("mock://")
    }
    async fn probe(&self) -> atlas_llm::provider::Availability {
        atlas_llm::provider::Availability::ready("synthetic")
    }
    async fn complete(
        &self,
        _: &atlas_llm::CompletionRequest,
        _: &str,
        _: Option<&atlas_llm::ApiKey>,
    ) -> atlas_llm::Result<atlas_llm::request::ProviderOutput> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            return Err(atlas_llm::LlmError::RateLimited("synthetic quota stop".into()));
        }
        Ok(atlas_llm::request::ProviderOutput {
            text: self.first.to_string(),
            reported_model: Some("synthetic-model".into()),
            usage: Default::default(),
            stop_reason: Some("stop".into()),
            agent_version: Some("test".into()),
            sent: Default::default(),
        })
    }
}

#[tokio::test]
async fn provider_stop_after_query_preserves_partial_rows_and_model_receipt() {
    use atlas_llm::{
        Cache, CacheMode, Llm, Registry,
        provider::{KeyPolicy, ProviderKind},
        registry::{Connection, ConnectionConfig},
    };
    use std::sync::{Arc, atomic::Ordering};
    let data = json!({"head":{"vars":["label"]},"results":{"bindings":[{"label":{"type":"literal","value":"Actual synthetic result"}}]}});
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
    let mut card = diagnostic_engine().schema.as_ref().clone();
    card.predicates.insert(format!("{RDFS}label"), Default::default());
    let engine = QueryEngine::new(card, format!("http://{addr}/sparql"), None).unwrap();
    let provider = Arc::new(StopAfterOne {
        first: json!({"action":"call","tool":"run_sparql","arguments":{"query":format!("{PREFIXES}SELECT ?label WHERE {{ ?resource rdfs:label ?label }} LIMIT 1")}}),
        calls: Default::default(),
    });
    let mut registry = Registry::default();
    registry.insert(Connection {
        config: ConnectionConfig {
            name: "stopmock".into(),
            default_model: Some("synthetic".into()),
            ..Default::default()
        },
        kind: ProviderKind::OpenAiCompatible,
        key_policy: KeyPolicy::None,
        provider: provider.clone(),
    });
    let llm = Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off));
    let answer = engine
        .understand(
            &llm,
            &QueryRequest {
                power_mode: true,
                connection: Some("stopmock".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    server.abort();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(answer.model_provenance.len(), 1);
    assert_eq!(answer.results.len(), 1);
    assert_eq!(
        answer.results[0].data["results"]["bindings"][0]["label"]["value"],
        "Actual synthetic result"
    );
    let terminal = answer.tool_trace.last().unwrap();
    assert_eq!(terminal["status"], "partial");
    assert_eq!(terminal["reason"], "model_error");
    assert!(terminal["error"].as_str().unwrap().contains("quota"));
    assert_eq!(
        answer.tool_trace[answer.tool_trace.len() - 2]["items"][0]["status"],
        "not_checked"
    );
}

#[tokio::test]
async fn research_loop_retains_question_and_full_evidence_but_repairs_invalid_coverage() {
    let data = json!({"head":{"vars":["org","label"]},"results":{"bindings":(0..100).map(|i|json!({
        "org":{"type":"uri","value":format!("urn:synthetic:org-{i}")},
        "label":{"type":"literal","value":format!("Synthetic organisation {i}: {}","source detail ".repeat(150))}
    })).collect::<Vec<_>>()}});
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
    let card = SchemaCard {
        schema_version: 1,
        activity: json!({}),
        graph: Lineage {
            source_url: "urn:synthetic".into(),
            retrieved_at: "fixture".into(),
            version: "synthetic-v1".into(),
            sha256: "a".repeat(64),
            record_locator: "tests/query_planning.rs".into(),
        },
        statements: 0,
        classes: Default::default(),
        predicates: [format!("{RA}serves_gene"), format!("{RDFS}label")]
            .into_iter()
            .map(|p| (p, Default::default()))
            .collect(),
        known_absent: Default::default(),
        prefixes: [("ra".into(), RA.into()), ("rdfs".into(), RDFS.into())].into(),
        semantic_units: Default::default(),
        provenance_pattern: String::new(),
    };
    let engine = QueryEngine::new(card, format!("http://{addr}/sparql"), None).unwrap();
    let query = format!(
        "{PREFIXES}SELECT DISTINCT ?org ?label WHERE {{ VALUES ?gene {{ {} }} ?org ra:serves_gene ?gene; rdfs:label ?label. }} LIMIT 100",
        iri("id", "HGNC:11444")
    );
    let call = json!({"action":"call","tool":"run_sparql","arguments":{"query":query,"limit":100}});
    let bad = json!({"action":"answer","tool":null,"arguments":{"coverage":[{"request":"Find official contact records","status":"answered","evidence_query_indexes":[9]}]}});
    let good = json!({"action":"answer","tool":null,"arguments":{"coverage":[
        {"request":"Find organisation records","status":"answered","evidence_query_indexes":[0]},
        {"request":"Validate permission to reuse models","status":"unsupported_in_release","evidence_query_indexes":[],"limitation":"No model permission property is recorded in this synthetic schema","schema_evidence":{"missing_predicates":["ra:model_permission"]}}
    ]}});
    let (llm, mock) = common::mock_llm(&[call, bad, good]);
    let question = "Find organisations for STXBP1, plus model access permissions; preserve both requested parts.";
    let answer = engine
        .understand(
            &llm,
            &QueryRequest {
                question: question.into(),
                linked: vec![LinkedEntity {
                    id: "HGNC:11444".into(),
                    label: "STXBP1".into(),
                }],
                connection: Some("mock".into()),
                power_mode: true,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    server.abort();
    assert_eq!(answer.results.len(), 1);
    assert_eq!(
        answer.results[0].data["results"]["bindings"].as_array().unwrap().len(),
        100
    );
    assert!(
        answer.results[0].data["results"]["bindings"][0]["label"]["value"]
            .as_str()
            .unwrap()
            .len()
            > 1800
    );
    let trace = answer.tool_trace.last().unwrap();
    assert_eq!(trace["stage"], "coverage");
    assert_eq!(trace["items"][1]["status"], "unsupported_in_release");
    assert_eq!(trace["scientific_completeness_verified"], false);
    let sent = mock.requests.lock().unwrap();
    assert_eq!(sent.len(), 3);
    for req in sent.iter() {
        let user: serde_json::Value = serde_json::from_str(&req.messages[1].content).unwrap();
        assert_eq!(user["question"], question);
        assert_eq!(user["linked"][0]["iri"], iri("id", "HGNC:11444"));
    }
    let preview = sent[1]
        .messages
        .last()
        .unwrap()
        .content
        .strip_prefix("Tool result preview:\n")
        .unwrap();
    assert!(preview.len() <= prompt::TOOL_CONTEXT_BYTES);
    let preview: serde_json::Value = serde_json::from_str(preview).unwrap();
    assert_eq!(preview["row_count"], 100);
    assert!(preview["omitted_rows"].as_u64().unwrap() > 0);
    assert!(
        sent[2]
            .messages
            .last()
            .unwrap()
            .content
            .contains("unknown successful query")
    );
}
