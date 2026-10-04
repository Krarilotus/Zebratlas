//! `hosted-free` (D23, D48) against a mock OpenRouter API (and one Anthropic-configured tier):
//! server key, private routing, clamping, limits, spend cap (reported cost first), kill switch,
//! quota listing, cache, fallback chain. No network.

use atlas_llm::{
    ApiKey, Cache, CacheMode, Call, CompletionRequest, FreeTierConfig, HOSTED_FREE, JsonSchema, Llm, LlmError, Message,
    Prices, QuotaReason, Registry,
};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SERVER_KEY: &str = "sk-or-server-secret";

/// OpenRouter chat completion; `cost` = OpenRouter's reported charge (`usage.cost`).
fn openrouter_reply(text: &str, input: u64, output: u64, cost: Option<f64>) -> ResponseTemplate {
    let mut usage = json!({"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output});
    if let Some(c) = cost {
        usage["cost"] = json!(c);
    }
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "gen-1", "object": "chat.completion", "created": 1, "model": "openai/gpt-oss-120b",
        "provider": "DeepInfra",
        "choices": [{"index": 0, "finish_reason": "stop",
                     "message": {"role": "assistant", "content": text}}],
        "usage": usage
    }))
}

async fn mock_with(input: u64, output: u64, cost: Option<f64>) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {SERVER_KEY}").as_str()))
        .respond_with(openrouter_reply("{\"answer\":\"ok\"}", input, output, cost))
        .mount(&s)
        .await;
    s
}

async fn mock(input: u64, output: u64) -> MockServer {
    mock_with(input, output, None).await
}

fn registry(server: &MockServer, extra: &str, cfg: FreeTierConfig) -> Registry {
    let toml = format!(
        "replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\npreset = \"hosted-free\"\nbase_url = \"{}/api/v1/\"\n{extra}",
        server.uri()
    );
    let mut reg = Registry::from_toml(&toml, false).unwrap();
    reg.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            server_key: Some(ApiKey::new(SERVER_KEY)),
            spend_file: None,
            ..cfg
        },
    );
    reg
}

fn llm(server: &MockServer, cfg: FreeTierConfig, cache: Option<&std::path::Path>) -> Llm {
    let cache = match cache {
        Some(dir) => Cache::new(dir, CacheMode::ReadWrite),
        None => Cache::new(std::env::temp_dir(), CacheMode::Off),
    };
    Llm::new(registry(server, "", cfg), cache)
}

fn req(text: &str) -> CompletionRequest {
    CompletionRequest::new(vec![Message::user(text)])
}

fn quota(e: &LlmError) -> Option<QuotaReason> {
    match e {
        LlmError::FreeQuota { reason, .. } => Some(*reason),
        _ => None,
    }
}

#[tokio::test]
async fn query_sol_uses_verified_prices_and_the_same_visitor_and_spend_guard() {
    let server = mock(100, 10).await;
    let original = llm(
        &server,
        FreeTierConfig {
            per_visitor_per_hour: 1,
            ..Default::default()
        },
        None,
    );
    let query = original.with_registry(original.registry().for_query_planning().unwrap());
    let call = Call::new(HOSTED_FREE).with_private().with_visitor("same-reader");
    let done = query.complete(&call, req("synthetic query plan")).await.unwrap();
    assert_eq!(done.response.requested_model, "openai/gpt-6.1-sol");
    assert!((done.response.cost_usd.unwrap() - 0.0003).abs() < 1e-9);
    assert_eq!(
        quota(&original.complete(&call, req("extract")).await.unwrap_err()),
        Some(QuotaReason::VisitorHourly)
    );
    let sent: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["model"], "openai/gpt-6.1-sol");
    assert_eq!(sent["provider"]["zdr"], true);
    assert_eq!(sent["provider"]["max_price"]["completion"], 10.0);
    let capped = llm(
        &server,
        FreeTierConfig {
            daily_usd: 0.001,
            ..Default::default()
        },
        None,
    );
    let query = capped.with_registry(capped.registry().for_query_planning().unwrap());
    assert_eq!(
        quota(&query.complete(&Call::new(HOSTED_FREE), req("plan")).await.unwrap_err()),
        Some(QuotaReason::DailyBudget)
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn hosted_model_override_is_rejected_before_network() {
    let s = mock(10, 1).await;
    let llm = llm(&s, FreeTierConfig::default(), None);
    let mut request = req("hello");
    request.model = Some("claude-opus-5-5".into());
    assert!(matches!(
        llm.complete(&Call::new(HOSTED_FREE), request).await,
        Err(LlmError::InvalidRequest(_))
    ));
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn private_calls_neither_read_nor_write_shared_cache() {
    let s = mock(10, 1).await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&s, FreeTierConfig::default(), Some(dir.path()));
    let public = Call::new(HOSTED_FREE).with_visitor("a");
    llm.complete(&public, req("same prompt")).await.unwrap();
    let done = llm.complete(&public.with_private(), req("same prompt")).await.unwrap();
    assert!(!done.response.cached);
    assert!(done.provenance.prompt.file.is_empty());
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    let call = Call::new(HOSTED_FREE).with_private().with_visitor("b");
    llm.complete(&call, req("private medical question")).await.unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn call_uses_server_key_private_routing_clamps_and_records_cost() {
    let s = mock(1_000, 500).await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(
        &s,
        FreeTierConfig {
            max_tokens: 1000,
            ..FreeTierConfig::default()
        },
        Some(dir.path()),
    );
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["answer"],
        "properties": {"answer": {"type": "string", "maxLength": 50}}});
    let r = req("hi")
        .with_temperature(0.0)
        .with_max_tokens(50_000)
        .with_schema(JsonSchema::new("a", schema));
    let call = Call::new(HOSTED_FREE).with_visitor("v1").with_inputs(["PMID:1"]);
    let done = llm.complete(&call, r.clone()).await.unwrap();
    assert_eq!(done.response.requested_model, "openai/gpt-oss-120b");
    assert_eq!(done.response.model_label.key, "llm.model.open_weight");
    assert_eq!(done.response.model_label.fallback, "gpt-oss-120b (OpenAI open-weight)");
    // No reported cost: worst-case ZDR table, 1000 in × $0.35/M + 500 out × $0.95/M = $0.000825
    assert!((done.response.cost_usd.unwrap() - 0.000825).abs() < 1e-12);
    let p = &done.provenance.activity.parameters;
    assert_eq!(p["cost_usd"], "0.000825");
    assert_eq!(p["free_tier"], "true");
    assert_eq!(p["model.label"], "gpt-oss-120b (OpenAI open-weight)");
    assert_eq!(p["sent.routed_provider"], "DeepInfra");

    let body: Value = serde_json::from_slice(&s.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["model"], "openai/gpt-oss-120b");
    assert_eq!(body["max_tokens"], 1000, "clamped to the free-tier cap");
    assert_eq!(
        body["provider"]["data_collection"], "deny",
        "D41/D48: no data collection"
    );
    assert_eq!(body["provider"]["zdr"], true, "zero-data-retention endpoints only");
    assert_eq!(body["provider"]["require_parameters"], true);
    assert_eq!(body["provider"]["max_price"]["prompt"], 0.35);
    assert_eq!(body["provider"]["max_price"]["completion"], 0.95);
    assert_eq!(body["response_format"]["type"], "json_schema");
    // No secret in the record or on disk.
    assert!(!serde_json::to_string(&done).unwrap().contains(SERVER_KEY));
    let cached = std::fs::read_to_string(dir.path().join(format!("{}.json", done.response.cache_key))).unwrap();
    assert!(!cached.contains(SERVER_KEY));

    // Cache hit: free, not counted against the visitor.
    let again = llm.complete(&call, r).await.unwrap();
    assert!(again.response.cached);
    assert_eq!(again.response.cost_usd, Some(0.0));
    assert_eq!(s.received_requests().await.unwrap().len(), 1);
    let info = llm.list_connections_for(false, Some("v1")).await;
    let free = info[0].free_tier.as_ref().unwrap();
    assert_eq!(free.visitor_remaining_hour, Some(19));
    assert!((free.spent_usd_today - 0.000825).abs() < 1e-4, "rounded to 4 decimals");
    assert_eq!(free.model_label.fallback, "gpt-oss-120b (OpenAI open-weight)");
    assert!(free.prices_source.contains("openrouter.ai"));
    assert_eq!(info[0].model_label.as_ref().unwrap().params["vendor"], "OpenAI");
}

#[tokio::test]
async fn reported_cost_wins_over_the_price_table() {
    let s = mock_with(1_000, 500, Some(0.000123)).await;
    let llm = llm(&s, FreeTierConfig::default(), None);
    let done = llm
        .complete(&Call::new(HOSTED_FREE).with_visitor("v"), req("hi"))
        .await
        .unwrap();
    assert_eq!(done.response.cost_usd, Some(0.000123));
    assert_eq!(done.response.usage.cost_usd, Some(0.000123));
    assert_eq!(done.provenance.activity.parameters["cost_usd.reported"], "0.000123");
    let t = llm.registry().free_tier(HOSTED_FREE).unwrap();
    assert!((t.spent_today() - 0.000123).abs() < 1e-12);
}

#[tokio::test]
async fn anthropic_configured_free_tier_still_works() {
    // Provider-agnostic: a TOML override can put the tier on another provider (here Anthropic).
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", SERVER_KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
            "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 1000, "output_tokens": 500}
        })))
        .mount(&s)
        .await;
    let toml = format!(
        "replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\npreset = \"hosted-free\"\nkind = \"anthropic\"\n\
         default_model = \"claude-sonnet-5-5\"\nsampling = false\nbase_url = \"{}/v1/\"\n",
        s.uri()
    );
    let mut reg = Registry::from_toml(&toml, false).unwrap();
    reg.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            server_key: Some(ApiKey::new(SERVER_KEY)),
            spend_file: None,
            model: "claude-sonnet-5-5".into(),
            prices: Prices::for_model("claude-sonnet-5-5").unwrap(),
            ..FreeTierConfig::default()
        },
    );
    let llm = Llm::new(reg, Cache::new(std::env::temp_dir(), CacheMode::Off));
    let done = llm
        .complete(
            &Call::new(HOSTED_FREE).with_visitor("v"),
            req("hi").with_temperature(0.0),
        )
        .await
        .unwrap();
    // 1000 in × $2/M + 500 out × $10/M = $0.007
    assert!((done.response.cost_usd.unwrap() - 0.007).abs() < 1e-9);
    assert_eq!(done.response.model_label.fallback, "claude-sonnet-5-5 (Anthropic)");
    let body: Value = serde_json::from_slice(&s.received_requests().await.unwrap()[0].body).unwrap();
    assert!(
        body.get("temperature").is_none(),
        "Sonnet 5.5 rejects sampling parameters"
    );
    assert!(
        body.get("provider").is_none(),
        "routing preferences are OpenRouter-only"
    );
}

#[tokio::test]
async fn visitor_limit_blocks_without_calling() {
    let s = mock(10, 10).await;
    let llm = llm(
        &s,
        FreeTierConfig {
            per_visitor_per_hour: 2,
            ..FreeTierConfig::default()
        },
        None,
    );
    let call = Call::new(HOSTED_FREE).with_visitor("v");
    llm.complete(&call, req("a")).await.unwrap();
    llm.complete(&call, req("b")).await.unwrap();
    let e = llm.complete(&call, req("c")).await.unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::VisitorHourly));
    assert!(e.to_string().contains("bring your own key or try later"), "{e}");
    assert!(matches!(
        e,
        LlmError::FreeQuota {
            retry_after_secs: Some(_),
            ..
        }
    ));
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    // Another visitor is unaffected.
    llm.complete(&Call::new(HOSTED_FREE).with_visitor("w"), req("d"))
        .await
        .unwrap();
}

#[tokio::test]
async fn daily_spend_cap() {
    // Two 10-input / 1000-output calls fit the cap; a third reservation is refused.
    let s = mock(10, 1000).await;
    let llm = llm(
        &s,
        FreeTierConfig {
            daily_usd: 0.004,
            max_tokens: 1000,
            ..FreeTierConfig::default()
        },
        None,
    );
    let call = Call::new(HOSTED_FREE).with_visitor("v");
    llm.complete(&call, req("a")).await.unwrap();
    llm.complete(&call, req("b")).await.unwrap();
    let e = llm.complete(&call, req("c")).await.unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::DailyBudget));
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    assert!(llm.registry().free_tier(HOSTED_FREE).unwrap().spent_today() <= 0.004);
}

#[tokio::test]
async fn kill_switch_byo_key_size_and_default() {
    let s = mock(10, 10).await;
    let llm = llm(
        &s,
        FreeTierConfig {
            max_input_chars: 100,
            ..FreeTierConfig::default()
        },
        None,
    );
    let call = Call::new(HOSTED_FREE);
    if std::env::var_os("ATLAS_LLM_DEFAULT").is_none() {
        assert_eq!(llm.default_connection(false), HOSTED_FREE);
        assert_eq!(llm.default_connection(true), "kisski");
    }
    // A visitor's own key is not accepted on the project's connection.
    let e = llm
        .complete(&Call::new(HOSTED_FREE).with_key(ApiKey::new("sk-user")), req("x"))
        .await
        .unwrap_err();
    assert!(matches!(e, LlmError::InvalidRequest(_)), "{e}");
    let e = llm.complete(&call, req(&"x".repeat(200))).await.unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::TooLarge));

    llm.registry().free_tier(HOSTED_FREE).unwrap().set_disabled(true);
    let e = llm.complete(&call, req("x")).await.unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::Disabled));
    let info = llm.list_connections(false).await;
    assert_eq!(info[0].free_tier.as_ref().unwrap().blocked, Some(QuotaReason::Disabled));
    if std::env::var_os("ATLAS_LLM_DEFAULT").is_none() {
        // An operator-disabled accounting gate cannot escape to a free provider.
        assert_eq!(llm.default_connection(false), HOSTED_FREE);
    }
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn missing_server_key_is_not_configured() {
    let s = MockServer::start().await;
    let toml = format!(
        "replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\npreset = \"hosted-free\"\n\
         base_url = \"{}/api/v1/\"\nkey_env = \"ATLAS_TEST_DEFINITELY_UNSET_KEY\"\n",
        s.uri()
    );
    let mut registry = Registry::from_toml(&toml, false).unwrap();
    // Test credential absence independently of other parallel fixtures' ledger locks.
    registry.set_free_tier(HOSTED_FREE, FreeTierConfig { spend_file: None, ..FreeTierConfig::default() });
    let llm = Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off));
    let e = llm.complete(&Call::new(HOSTED_FREE), req("x")).await.unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::NotConfigured));
    let info = llm.list_connections(false).await;
    assert!(!info[0].needs_key, "visitors never need a key for hosted-free");
    assert!(!info[0].key_from_env_available);
}

/// A KISSKI-like OpenAI-compatible fallback that needs no key (the real KISSKI needs one).
fn fallback_toml(uri: &str) -> String {
    format!(
        "[[connections]]\nname = \"backup\"\nkind = \"openai-compatible\"\nbase_url = \"{uri}/v1/\"\n\
         default_model = \"openai-gpt-oss-120b\"\nkey_required = false\n"
    )
}

#[tokio::test]
async fn default_chain_falls_back_on_outage_but_not_on_quota() {
    // hosted-free answers 503 (all ZDR endpoints down); the backup serves the call.
    let free = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_string("no healthy upstream"))
        .mount(&free)
        .await;
    let backup = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(openrouter_reply("{\"answer\":\"from backup\"}", 5, 5, None))
        .mount(&backup)
        .await;
    let reg = registry(
        &free,
        &fallback_toml(&backup.uri()),
        FreeTierConfig {
            per_visitor_per_hour: 1,
            ..FreeTierConfig::default()
        },
    );
    let llm = Llm::new(reg, Cache::new(std::env::temp_dir(), CacheMode::Off)).with_fallbacks(["backup"]);
    assert_eq!(llm.default_chain(), vec![HOSTED_FREE.to_owned(), "backup".into()]);

    #[derive(Debug, serde::Deserialize)]
    struct A {
        answer: String,
    }
    let schema = json!({"type": "object", "required": ["answer"], "properties": {"answer": {"type": "string"}}});
    let out = llm
        .complete_default_json::<A>(
            Some("v"),
            &["doc:sha256:abc".to_owned()],
            req("hi").with_schema(JsonSchema::new("a", schema.clone())),
        )
        .await
        .unwrap();
    assert_eq!(out.value.answer, "from backup");
    let c = &out.calls[0];
    assert_eq!(c.response.connection, "backup");
    assert_eq!(c.response.model_label.fallback, "gpt-oss-120b (OpenAI open-weight)");
    assert_eq!(c.provenance.activity.parameters["fallback_from"], HOSTED_FREE);
    assert_eq!(c.provenance.activity.parameters["fallback_reason"], "provider_error");
    assert_eq!(c.provenance.activity.parameters["inputs"], "doc:sha256:abc");

    // The visitor's hourly limit (1) is used up: a quota error is returned, never routed around.
    let e = llm
        .complete_default_json::<A>(Some("v"), &[], req("again").with_schema(JsonSchema::new("a", schema)))
        .await
        .unwrap_err();
    assert_eq!(quota(&e), Some(QuotaReason::VisitorHourly));
    assert_eq!(backup.received_requests().await.unwrap().len(), 1);

    // Explicit fallback opt-in preserves visitor context.
    let selected = llm
        .complete(&Call::new(HOSTED_FREE).with_visitor("w").with_fallback(true), req("x"))
        .await
        .unwrap();
    assert_eq!(selected.response.connection, "backup");
    assert_eq!(selected.provenance.activity.parameters["fallback_from"], HOSTED_FREE);
}

#[tokio::test]
async fn private_fallback_keeps_cache_isolation_and_visitor_quota() {
    let free = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&free)
        .await;
    let backup = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(openrouter_reply("private answer", 5, 5, None))
        .mount(&backup)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let reg = registry(
        &free,
        &fallback_toml(&backup.uri()),
        FreeTierConfig {
            per_visitor_per_hour: 1,
            ..FreeTierConfig::default()
        },
    );
    let llm = Llm::new(reg, Cache::new(dir.path(), CacheMode::ReadWrite)).with_fallbacks(["backup"]);
    let call = Call::new(HOSTED_FREE)
        .with_private()
        .with_fallback(true)
        .with_visitor("private-visitor");
    let out = llm.complete(&call, req("private medical question")).await.unwrap();
    assert_eq!(out.response.connection, "backup");
    assert!(out.provenance.prompt.file.is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let error = llm.complete(&call, req("again")).await.unwrap_err();
    assert_eq!(quota(&error), Some(QuotaReason::VisitorHourly));
    assert_eq!(backup.received_requests().await.unwrap().len(), 1);
}

#[test]
fn paid_providers_need_free_tier_for_env_keys() {
    let e = Registry::from_toml("[[connections]]\nname = \"openai\"\nenv_fallback = true\n", false).unwrap_err();
    assert!(e.to_string().contains("D23"), "{e}");
    // Presets include hosted-free; it is the only paid connection allowed a server key.
    let r = Registry::presets(false).unwrap();
    assert!(r.free_tier(HOSTED_FREE).is_some());
    assert!(r.free_tier("anthropic").is_none());
    assert!(
        r.free_tier("openrouter").is_none(),
        "BYO OpenRouter never uses the server key"
    );
}
