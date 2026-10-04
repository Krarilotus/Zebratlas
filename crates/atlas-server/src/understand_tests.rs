use super::*;
use crate::test_support::{call, state};
use atlas_llm::{ApiKey, FreeTierConfig, HOSTED_FREE, Registry};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use std::sync::Arc;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path_regex};

fn term(kind: &str, text: &str, english: &str, negated: bool) -> Value {
    json!({"kind":kind,"text":text,"english":english,"gene":"","hgvs":"","negated":negated})
}

async fn mocked(answer: Value) -> (AppState, MockServer, tempfile::TempDir) {
    let server = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"fixture","object":"chat.completion","created":0,"model":"openai/gpt-oss-120b",
            "choices":[{"message":{"role":"assistant","content":answer.to_string()},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}
        })))
        .mount(&server)
        .await;
    let mut registry=Registry::from_toml(&format!("replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\nkind = \"openai-compatible\"\nbase_url = \"{}/v1\"\ndefault_model = \"openai/gpt-oss-120b\"\nkey_required = true\nfree_tier = true\n",server.uri()),false).unwrap();
    registry.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            model: "openai/gpt-oss-120b".into(),
            server_key: Some(ApiKey::new("fixture-key")),
            spend_file: None,
            per_visitor_per_hour: 100,
            ..Default::default()
        },
    );
    let cache = tempfile::tempdir().unwrap();
    let mut s = state();
    s.llm.llm = Some(Arc::new(Llm::new(
        registry,
        Cache::new(cache.path(), CacheMode::ReadWrite),
    )));
    (s, server, cache)
}

#[tokio::test]
async fn exact_entity_bypasses_model_but_sentence_with_same_gene_does_not() {
    let (s, server, _cache) =
        mocked(json!({"references":[],"terms":[term("gene","STXBP1","",false)],"intent":"find_group"})).await;
    let one = text(&s, &HeaderMap::new(), "STXBP1".into(), "en", true).await.unwrap();
    assert_eq!(one.metadata["used"], false);
    let sentence = text(&s, &HeaderMap::new(), "Find a group for STXBP1".into(), "en", true)
        .await
        .unwrap();
    assert_eq!(sentence.metadata["used"], true);
    assert_eq!(sentence.intent, Intent::FindGroup);
    assert_eq!(sentence.focus, vec!["HGNC:11444"]);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn long_private_text_is_redacted_once_never_cached_or_registered_publicly() {
    let (s,server,cache)=mocked(json!({"references":[],"terms":[term("gene","STXBP1","",false),term("gene","made-up","",false)],"intent":"find_study"})).await;
    let document = format!(
        "Patient: Alice Example\nDOB: 12.03.2010\nEmail: alice@example.invalid\nFind trials for STXBP1. {}",
        "letter text ".repeat(80)
    );
    let found = text(&s, &HeaderMap::new(), document, "en", true).await.unwrap();
    assert_eq!(found.focus, vec!["HGNC:11444"]);
    assert_eq!(found.metadata["rejected_mentions"], 1);
    let requests = server.received_requests().await.unwrap();
    let sent = String::from_utf8(requests[0].body.clone()).unwrap();
    for secret in ["Alice", "alice@example", "12.03.2010"] {
        assert!(!sent.contains(secret));
    }
    let result = found.json(&s).to_string();
    for secret in ["Alice", "alice@example", "12.03.2010"] {
        assert!(!result.contains(secret));
    }
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    let id = found.metadata["activities"][0]["activity"]["id"].as_str().unwrap();
    assert!(s.llm.runtime.lookup(id).is_none());
}

#[tokio::test]
async fn multilingual_mentions_link_to_real_ids_and_preserve_absence() {
    let (s,_server,_cache)=mocked(json!({"references":[],"terms":[term("gene","STXBP1","",false),term("phenotype","keine Anfälle","Seizure",true)],"intent":"find_study"})).await;
    let found = text(
        &s,
        &HeaderMap::new(),
        "Studien für STXBP1; keine Anfälle".into(),
        "de",
        true,
    )
    .await
    .unwrap();
    assert_eq!(found.metadata["used"], true);
    assert_eq!(found.present, Vec::<u32>::new());
    assert_eq!(found.excluded, vec![0]);
    assert!(
        found
            .terms
            .iter()
            .any(|t| t["match"]["node"]["id"] == "HP:0001250" && t["negated"] == true)
    );
    assert!(
        found
            .hits
            .iter()
            .any(|h| h.node.id == "HGNC:11444" && h.why.contains("Name"))
    );
    assert_eq!(found.json(&s)["excluded"], json!(["HP:0001250"]));
    assert!(found.terms.iter().any(|t| t["english"] == "Seizure"));
}

#[tokio::test]
async fn missing_or_disabled_model_returns_real_choices_without_reformulation_advice() {
    let s = state();
    let found = text(
        &s,
        &HeaderMap::new(),
        "Please find a group for STXBP1".into(),
        "en",
        true,
    )
    .await
    .unwrap();
    assert_eq!(found.metadata["status"], "unavailable");
    assert!(!found.hits.is_empty());
    assert_eq!(found.metadata["reason"]["key"], "search.model_unavailable");
    let found = text(&s, &HeaderMap::new(), "Find a group for STXBP1".into(), "de", false)
        .await
        .unwrap();
    assert_eq!(found.metadata["status"], "disabled");
    assert!(!found.json(&s).to_string().contains("type a gene"));
}

#[tokio::test]
async fn both_search_and_resolve_return_the_same_interpretation_and_ranked_reasons() {
    let app = Router::new()
        .route("/search", get(crate::find::search).post(crate::find::search_text))
        .route("/resolve", get(crate::find::resolve))
        .with_state(state());
    let (status, body) = call(app.clone(), "POST", "/search", Some(json!({"q":"STXBP1","llm":0}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["understood"]["focus_ids"], json!(["HGNC:11444"]));
    assert!(
        body["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["why_messages"].is_array())
    );
    let (_, resolved) = call(app, "GET", "/resolve?q=STXBP1&llm=0", None).await;
    assert_eq!(resolved["understood"], body["understood"]);
}

#[derive(Debug)]
struct Personal {
    calls: std::sync::Mutex<Vec<CompletionRequest>>,
    answer: Value,
    offline: bool,
}
#[async_trait::async_trait]
impl atlas_llm::connector::ConnectorTransport for Personal {
    async fn available(&self) -> bool {
        !self.offline
    }
    async fn dispatch(&self, req: CompletionRequest) -> atlas_llm::Result<atlas_llm::request::ProviderOutput> {
        self.calls.lock().unwrap().push(req);
        if self.offline {
            return Err(atlas_llm::LlmError::Unavailable("fixture offline".into()));
        }
        Ok(atlas_llm::request::ProviderOutput {
            text: self.answer.to_string(),
            reported_model: Some("fixture-codex".into()),
            usage: Default::default(),
            stop_reason: None,
            agent_version: Some("fixture".into()),
            sent: Default::default(),
        })
    }
}

#[tokio::test]
async fn personal_connector_receives_redacted_prompt_and_never_falls_back_to_hosted() {
    let (mut s, hosted, _cache) = mocked(json!({"references":[],"terms":[],"intent":"search"})).await;
    let personal = Arc::new(Personal {
        calls: Default::default(),
        answer: json!({"references":[],"terms":[term("gene","STXBP1","",false)],"intent":"connect"}),
        offline: false,
    });
    let model = s.llm.llm.as_ref().unwrap();
    let mut registry = model.registry().clone();
    registry.insert(atlas_llm::connector::ConnectorProvider::connection(
        "connector:fixture".into(),
        Some("fixture-codex".into()),
        personal.clone(),
    ));
    s.llm.llm = Some(Arc::new(Llm::new(registry, model.cache().clone())));
    let mut headers = HeaderMap::new();
    headers.insert("x-llm-connection", "connector:fixture".parse().unwrap());
    let found = text(
        &s,
        &headers,
        "Patient: Alice Example\nConnect us with STXBP1 researchers".into(),
        "en",
        true,
    )
    .await
    .unwrap();
    assert_eq!(found.intent, Intent::Connect);
    assert_eq!(found.metadata["connection"], "connector:fixture");
    assert_eq!(hosted.received_requests().await.unwrap().len(), 0);
    assert!(!format!("{:?}", personal.calls.lock().unwrap()).contains("Alice"));
}

#[tokio::test]
async fn unavailable_personal_connection_does_not_send_to_a_different_provider() {
    let (mut s, hosted, _cache) = mocked(json!({"references":[],"terms":[],"intent":"search"})).await;
    let personal = Arc::new(Personal {
        calls: Default::default(),
        answer: Value::Null,
        offline: true,
    });
    let model = s.llm.llm.as_ref().unwrap();
    let mut registry = model.registry().clone();
    registry.insert(atlas_llm::connector::ConnectorProvider::connection(
        "connector:fixture".into(),
        Some("fixture-codex".into()),
        personal,
    ));
    s.llm.llm = Some(Arc::new(Llm::new(registry, model.cache().clone())));
    let mut headers = HeaderMap::new();
    headers.insert("x-llm-connection", "connector:fixture".parse().unwrap());
    let found = text(&s, &headers, "Find studies for STXBP1".into(), "en", true)
        .await
        .unwrap();
    assert_eq!(found.metadata["status"], "unavailable");
    assert!(!found.hits.is_empty());
    assert_eq!(hosted.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn generic_references_are_grounded_and_intent_orders_real_studies_first() {
    let (s, _server, _cache) = mocked(json!({"intent":"find_study","terms":[term("gene","STXBP1","",false)],
        "references":[{"kind":"study","text":"NCTTEST0","english":"","negated":false},
                      {"kind":"study","text":"imaginary study","english":"","negated":false}]}))
    .await;
    let found = text(
        &s,
        &HeaderMap::new(),
        "Find STXBP1 studies including NCTTEST0".into(),
        "en",
        true,
    )
    .await
    .unwrap();
    assert_eq!(found.metadata["used"], true, "{}", found.metadata);
    assert!(found.focus.contains(&"NCTTEST0".to_owned()), "{}", found.json(&s));
    assert!(found.terms.iter().all(|t| t["text"] != "imaginary study"));
    assert_eq!(found.hits[0].node.kind, NodeKind::Study);
    assert!(found.hits.iter().all(|h| !h.strong));
    assert!(found.hits.iter().all(|h| !h.evidence.nodes.is_empty()));
}

#[tokio::test]
async fn first_character_typeahead_never_calls_the_model() {
    let (s, server, _cache) = mocked(json!({"references":[],"terms":[],"intent":"search"})).await;
    let app = Router::new().route("/suggest", get(crate::find::suggest)).with_state(s);
    let (status, body) = call(app, "GET", "/suggest?q=S&lang=de", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["method"], "atlas-typeahead-v1");
    assert!(!body["suggestions"].as_array().unwrap().is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn a_model_cannot_attach_a_gene_from_a_substring_or_english_variant_field() {
    let (s, _server, _cache) = mocked(json!({"references":[],"intent":"search","terms":[
        {"kind":"variant","text":"c.1A>T","english":"STXBP1","gene":"STXBP1","hgvs":"c.1A>T","negated":false}
    ]}))
    .await;
    let found = text(&s, &HeaderMap::new(), "Variant XSTXBP1 c.1A>T".into(), "en", true)
        .await
        .unwrap();
    assert_eq!(found.metadata["used"], true);
    assert!(found.focus.is_empty());
    assert_eq!(found.terms[0]["gene"], Value::Null);
    assert_eq!(found.terms[0]["status"], "not_found");
    assert_eq!(found.terms[0]["hgvs"], "c.1A>T");
}
