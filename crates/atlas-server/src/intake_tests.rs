use super::*;
use crate::test_support::{call, response, state};
use axum::{
    Router,
    body::Body,
    extract::DefaultBodyLimit,
    http::Request,
    routing::{get, post},
};
use std::sync::Arc;

fn app(s: AppState) -> Router {
    Router::new()
        .route("/intake", post(intake))
        .route("/terms", post(edited))
        .route("/processor", get(processor))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(s)
}

#[tokio::test]
async fn lexical_paste_and_edited_terms_keep_unknowns() {
    let app = app(state());
    let (status, b) = call(
        app.clone(),
        "POST",
        "/intake",
        Some(json!({"text":"Patient: Alice Example\nEmail: alice@example.invalid\nSTXBP1 c.1162C>T and Seizure"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!b["sent_text"].as_str().unwrap().contains("Alice"));
    assert!(!b["sent_text"].as_str().unwrap().contains("alice@example"));
    assert_eq!(b["model"]["origin"], "lexical");
    assert_eq!(b["saved"], Value::Null);
    assert!(
        b["terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["kind"] == "gene" && t["status"] == "found")
    );
    assert!(b["terms"].as_array().unwrap().iter().any(|t| t["kind"] == "phenotype"));
    let (_,b)=call(app,"POST","/terms",Some(json!({"references":[],"terms":[{"kind":"condition","text":"Synthetic STXBP1 condition"},{"kind":"gene","text":"ZZZZ999"},{"kind":"phenotype","text":"Seizure","negated":true}]}))).await;
    assert_eq!(b["terms"][0]["status"], "found");
    assert_eq!(b["terms"][1]["status"], "not_found");
    assert_eq!(b["terms"][1]["match"], Value::Null);
    assert_eq!(b["terms"][0]["span"], Value::Null);
    assert_eq!(b["query"], "Synthetic STXBP1 condition");
}

#[tokio::test]
async fn edited_variants_require_verbatim_hgvs_and_errors_are_messages() {
    let app = app(state());
    let (status, b) = call(
        app.clone(),
        "POST",
        "/terms",
        Some(json!({"references":[],"terms":[
            {"kind":"variant","text":"not a variant","gene":"STXBP1","hgvs":"c.99A>C"},
            {"kind":"variant","text":"c.1A>C","gene":"STXBP1","hgvs":"invented"}
        ]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b["terms"][0]["status"], "not_found");
    assert_eq!(b["terms"][0]["hgvs"], Value::Null);
    assert_eq!(b["terms"][1]["hgvs"], "c.1A>C");
    let (status, b) = call(
        app,
        "POST",
        "/terms",
        Some(json!({"references":[],"terms":[{"kind":"unknown","text":"a"}]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["msg"]["key"], "intake.error.invalid");
}

#[tokio::test]
async fn multipart_text_limits_and_empty_input() {
    let app = app(state());
    let content = "--test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"synthetic.txt\"\r\nContent-Type: text/plain\r\n\r\nSTXBP1\r\n--test-boundary--\r\n";
    let req = Request::builder()
        .method("POST")
        .uri("/intake")
        .header("content-type", "multipart/form-data; boundary=test-boundary")
        .body(Body::from(content))
        .unwrap();
    let (status, b) = response(app.clone(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b["intake"]["format"], "txt");
    let (status, b) = call(
        app.clone(),
        "POST",
        "/intake",
        Some(json!({"text":"x".repeat(MAX_BYTES+1)})),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(b["msg"]["key"], "intake.error.too_large");
    let (status, b) = call(app, "POST", "/intake", Some(json!({"text":""}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["msg"]["key"], "intake.error.empty");
}

#[tokio::test]
async fn default_model_redaction_no_cache_grounded_terms_and_shared_quota() {
    use atlas_llm::{ApiKey, Cache, CacheMode, FreeTierConfig, HOSTED_FREE, Registry};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let server = MockServer::start().await;
    let envelope = json!({"references":[],"intent":"search","terms":[
        {"kind":"gene","text":"STXBP1","english":"","hgvs":"","gene":"","negated":false},
        {"kind":"gene","text":"ZZZZ999","english":"","hgvs":"","gene":"","negated":false},
        {"kind":"phenotype","text":"invented feature","english":"Seizure","hgvs":"","gene":"","negated":false}
    ]});
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"synthetic-completion","model":"openai/gpt-oss-120b","choices":[{"message":{"role":"assistant","content":envelope.to_string()},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}})))
        .expect(1).mount(&server).await;
    let mut registry=Registry::from_toml(&format!("replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\nkind = \"openai-compatible\"\nbase_url = \"{}/v1\"\ndefault_model = \"openai/gpt-oss-120b\"\nkey_required = true\nfree_tier = true\n",server.uri()),false).unwrap();
    registry.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            model: "openai/gpt-oss-120b".into(),
            server_key: Some(ApiKey::new("synthetic-server-key")),
            spend_file: None,
            per_visitor_per_hour: 1,
            ..Default::default()
        },
    );
    let cache = tempfile::tempdir().unwrap();
    let model = Arc::new(Llm::new(registry, Cache::new(cache.path(), CacheMode::ReadWrite)));
    let mut s = state();
    s.llm.llm = Some(model.clone());
    let app = app(s);
    let document = "Patient: Alice Example\nDOB: 12.03.2010\nEmail: alice@example.invalid\nSTXBP1 ZZZZ999";
    let (_, p) = call(app.clone(), "GET", "/processor", None).await;
    assert!(p["processor"].as_str().unwrap().contains("gpt-oss-120b"));
    let (status, b) = call(app.clone(), "POST", "/intake", Some(json!({"text":document}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b["model"]["used"], true);
    assert_eq!(b["model"]["id"], "openai/gpt-oss-120b");
    assert_eq!(b["terms"].as_array().unwrap().len(), 2);
    assert_eq!(b["terms"][1]["status"], "not_found");
    let requests = server.received_requests().await.unwrap();
    let sent = String::from_utf8(requests[0].body.clone()).unwrap();
    for secret in ["Alice", "alice@example", "12.03.2010"] {
        assert!(!sent.contains(secret));
    }
    let wire: Value = serde_json::from_str(&sent).unwrap();
    let input: Value = serde_json::from_str(wire["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(input["text"], b["sent_text"]);
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    assert!(b["model"]["cost_usd"].as_f64().unwrap() > 0.0);
    let (status, b) = call(app, "POST", "/intake", Some(json!({"text":document}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b["model"]["origin"], "lexical");
    assert_eq!(b["model"]["reason"]["key"], "intake.model.quota");
}

/// One explicitly requested live smoke check; only synthetic text and allowlisted metadata.
#[tokio::test]
#[ignore = "requires the main checkout's provider environment; makes one intake request"]
async fn live_synthetic_intake_measurement() {
    let model = Llm::from_env().unwrap();
    let mut s = state();
    s.llm.llm = Some(Arc::new(model));
    let (status,b) = call(app(s),"POST","/intake",Some(json!({"text":"Patient: Alice Example\nDOB: 12.03.2010\nSTXBP1 c.1162C>T. Seizures and developmental delay."}))).await;
    assert_eq!(status, StatusCode::OK);
    println!(
        "{}",
        json!({"intake":b["intake"],"model":b["model"],"timing_ms":b["timing_ms"]})
    );
    assert_eq!(
        b["model"]["used"], true,
        "default provider did not complete the synthetic intake"
    );
    assert!(!b["sent_text"].as_str().unwrap().contains("Alice"));
}
