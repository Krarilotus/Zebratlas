//! Hosted routing against local HTTP fixtures; never uses account keys or public APIs.
use atlas_llm::{
    ApiKey, Cache, CacheMode, Call, CompletionRequest, FreeTierConfig, Llm, LlmError, Message, QuotaReason, Registry,
};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup(hourly: u32) -> (MockServer, Llm) {
    let server = MockServer::start().await;
    let mut registry = Registry::from_toml(
        &format!(
            "replace_defaults=true\n[[connections]]\nname='gemini-free'\nbase_url='{u}/v1beta/'\n\
        [[connections]]\nname='gemini-lite-free'\nbase_url='{u}/v1beta/'\n\
        [[connections]]\nname='openai-hosted'\nbase_url='{u}/v1/'\n",
            u = server.uri()
        ),
        false,
    )
    .unwrap();
    for name in ["gemini-free", "gemini-lite-free", "openai-hosted"] {
        let cfg = registry.free_tier(name).unwrap().config().clone();
        registry.set_free_tier(
            name,
            FreeTierConfig {
                server_key: Some(ApiKey::new(format!("fixture-{name}"))),
                spend_file: None,
                per_visitor_per_hour: hourly,
                ..cfg
            },
        );
    }
    (
        server,
        Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off))
            .with_fallbacks(["gemini-free", "openai-hosted", "kisski"]),
    )
}

fn request() -> CompletionRequest {
    CompletionRequest::new(vec![Message::user("Fixture")]).with_max_tokens(100)
}
fn gemini_reply() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "candidates": [{"content":{"role":"model", "parts":[{"text":"lite"}]},"finishReason":"STOP"}],
        "usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5}
    }))
}

#[tokio::test]
async fn gemini_outage_falls_back_to_openai_with_its_own_key() {
    let (server, llm) = setup(20).await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .and(header("x-goog-api-key", "fixture-gemini-free"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer fixture-openai-hosted"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"fixture", "object":"chat.completion", "created":0, "model":"gpt-5.4-mini",
            "choices":[{"index":0,"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let done = llm
        .complete(
            &Call::new("gemini-free").with_visitor("visitor").with_private().with_fallback(true),
            request().with_temperature(0.0),
        )
        .await
        .unwrap();
    assert_eq!(done.response.connection, "openai-hosted");
    assert_eq!(done.response.requested_model, "gpt-5.4-mini");
    assert_eq!(
        done.provenance.activity.parameters["fallback.failed_connections"],
        "gemini-free"
    );
    let calls = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&calls[1].body).unwrap();
    assert!(body.get("temperature").is_none());
    assert_eq!(body["max_completion_tokens"], 100);
    assert!(!serde_json::to_string(&done).unwrap().contains("fixture-openai-hosted"));
}

#[tokio::test]
async fn easy_task_routes_to_lite_and_retains_private_visitor_context() {
    let (server, llm) = setup(20).await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.5-flash-lite:generateContent"))
        .and(header("x-goog-api-key", "fixture-gemini-lite-free"))
        .respond_with(gemini_reply())
        .expect(1)
        .mount(&server)
        .await;
    let call = Call::new("gemini-free")
        .with_visitor("visitor")
        .with_private()
        .with_inputs(["PMID:1"]);
    let routed = llm.for_easy_task(&call);
    assert!(routed.private);
    assert_eq!(routed.visitor, call.visitor);
    assert_eq!(routed.inputs, call.inputs);
    let done = llm.complete(&routed, request()).await.unwrap();
    assert_eq!(done.response.requested_model, "gemini-3.5-flash-lite");
    assert_eq!(call.connection, "gemini-free", "the original complex route is retained");
    assert_eq!(
        llm.for_easy_task(&Call::new("openai").with_key(ApiKey::new("BYO")))
            .connection,
        "openai"
    );
}

#[tokio::test]
async fn visitor_limits_and_explicit_models_do_not_trigger_fallback() {
    let (server, llm) = setup(0).await;
    let error = llm
        .complete(&Call::new("gemini-free").with_visitor("visitor"), request())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LlmError::FreeQuota {
            reason: QuotaReason::VisitorHourly,
            ..
        }
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    let error = llm
        .complete(&Call::new("gemini-free"), request().with_model("unapproved"))
        .await
        .unwrap_err();
    assert!(matches!(error, LlmError::InvalidRequest(_)));
}

#[test]
fn missing_openai_key_is_unconfigured_and_guardrails_are_terminal() {
    let mut registry = Registry::from_toml(
        "replace_defaults=true\n[[connections]]\nname='openai-hosted'\nkey_env='ATLAS_ROUTING_TEST_UNSET_KEY'\n",
        false,
    )
    .unwrap();
    let cfg = registry.free_tier("openai-hosted").unwrap().config();
    assert_eq!(cfg.model, "gpt-5.4-mini");
    assert!(cfg.spend_file.as_ref().unwrap().ends_with("openai-hosted-spend.json"));
    registry.set_free_tier(
        "openai-hosted",
        FreeTierConfig {
            spend_file: None,
            ..cfg.clone()
        },
    );
    for reason in [
        QuotaReason::Disabled,
        QuotaReason::DailyBudget,
        QuotaReason::VisitorDaily,
        QuotaReason::TooLarge,
    ] {
        assert!(!atlas_llm::routing::can_fallback(&LlmError::FreeQuota {
            reason,
            retry_after_secs: None
        }));
    }
}

#[tokio::test]
async fn lite_outage_escalates_to_flash_without_replaying_lite() {
    let (server, llm) = setup(20).await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.5-flash-lite:generateContent"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .respond_with(gemini_reply())
        .expect(1)
        .mount(&server)
        .await;
    let call = llm.for_easy_task(&Call::new("gemini-free").with_visitor("visitor").with_fallback(true));
    let done = llm.complete(&call, request()).await.unwrap();
    assert_eq!(done.response.requested_model, "gemini-3.8-flash");
    assert_eq!(
        done.provenance.activity.parameters["fallback.failed_connections"],
        "gemini-lite-free"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[test]
fn hierarchy_is_explicit_and_has_no_duplicate_attempts() {
    assert_eq!(
        atlas_llm::routing::fallback_order("hosted-free"),
        ["hosted-free", "kisski"]
    );
    assert_eq!(
        atlas_llm::routing::fallback_order("gemini-lite-free"),
        [
            "gemini-lite-free",
            "hosted-free",
            "kisski"
        ]
    );
    assert!(!atlas_llm::routing::is_hosted("openai"));
    assert!(!atlas_llm::routing::is_hosted("ollama"));
}


#[tokio::test]
async fn kisski_only_policy_never_sends_easy_text_to_installed_gemini() {
    let (server, llm) = setup(20).await;
    let llm = llm.with_fallbacks(["kisski"]);
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.5-flash-lite:generateContent"))
        .respond_with(gemini_reply()).expect(0).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"fixture", "object":"chat.completion", "created":0, "model":"gpt-5.4-mini",
            "choices":[{"index":0,"message":{"role":"assistant","content":"approved"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}
        }))).expect(1).mount(&server).await;
    let call = Call::new("openai-hosted").with_visitor("visitor").with_private();
    let routed = llm.for_easy_task(&call);
    assert_eq!(routed.connection, "openai-hosted");
    assert_eq!(llm.for_easy_task(&Call::new("hosted-free")).connection, "hosted-free");
    assert!(!llm.default_chain().iter().any(|name| name.starts_with("gemini")));
    let result = llm.complete(&routed, request()).await.unwrap();
    assert_eq!(result.response.connection, "openai-hosted");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn selected_hosted_connection_without_fallback_never_changes_provider() {
    let (server, llm) = setup(20).await;
    Mock::given(method("POST")).and(path("/v1beta/models/gemini-3.8-flash:generateContent"))
        .respond_with(ResponseTemplate::new(503)).expect(1).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
    assert!(llm.complete(&Call::new("gemini-free").with_private(), request()).await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
