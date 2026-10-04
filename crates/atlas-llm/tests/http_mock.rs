//! HTTP providers against a mock server: request shape, key handling, error mapping, cache
//! replay, provenance, and the guarded JSON helper. No network.

use std::time::Duration;

use atlas_core::provenance::Provenance;
use atlas_llm::{
    ApiKey, Cache, CacheMode, Call, CompletionRequest, JsonSchema, Llm, LlmError, Message, ProviderKind, Registry,
};
use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "sk-test-secret-123";

fn openai_reply(text: &str) -> Value {
    json!({
        "id": "chatcmpl-1", "object": "chat.completion", "created": 0, "model": "served-model-v2",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 11, "completion_tokens": 3, "total_tokens": 14}
    })
}

fn llm(toml: &str, cache: &std::path::Path, mode: CacheMode) -> Llm {
    Llm::new(Registry::from_toml(toml, false).unwrap(), Cache::new(cache, mode))
}

fn compat(server: &MockServer, key_required: bool) -> String {
    format!(
        "replace_defaults = true\n[[connections]]\nname = \"mock\"\nkind = \"openai-compatible\"\n\
         base_url = \"{}/v1\"\ndefault_model = \"m1\"\nkey_required = {key_required}\n",
        server.uri()
    )
}

#[test]
fn kind_names_round_trip() {
    for k in [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::OpenRouter,
        ProviderKind::Gemini,
        ProviderKind::OpenAiCompatible,
        ProviderKind::ClaudeCode,
        ProviderKind::CodexCli,
        ProviderKind::GeminiCli,
        ProviderKind::OpenCodeCli,
    ] {
        assert_eq!(serde_json::to_value(k).unwrap(), json!(k.as_str()));
    }
}

#[tokio::test]
async fn explicit_json_object_endpoint_keeps_full_local_schema_validation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "model":"apodex/apodex-1.1-mini:free",
            "response_format":{"type":"json_object"},
            "provider":{
                "data_collection":"deny","zdr":true,"require_parameters":true,
                "allow_fallbacks":false,"only":["Novita"],
                "max_price":{"prompt":0,"completion":0}
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply(r#"{"gene":"STXBP1"}"#)))
        .expect(1)
        .mount(&server)
        .await;
    let config = format!(
        "replace_defaults=true\n[[connections]]\nname='json-object-endpoint'\nkind='openrouter'\nbase_url='{}/v1'\ndefault_model='apodex/apodex-1.1-mini:free'\nkey_required=false\nschema_mode='json-object'\nprovider_routing={{data_collection='deny',zdr=true,require_parameters=true,allow_fallbacks=false,only=['Novita'],max_price={{prompt=0,completion=0}}}}\n",
        server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&config, dir.path(), CacheMode::Off);
    let schema = json!({"type":"object","additionalProperties":false,"required":["gene"],"properties":{"gene":{"type":"string","enum":["STXBP1"]}}});
    let req = CompletionRequest::new(vec![Message::user("Find studies for STXBP1")])
        .with_schema(JsonSchema::new("gene", schema.clone()));
    let done = llm
        .complete_json::<Value>(&Call::new("json-object-endpoint"), req.clone())
        .await
        .unwrap();
    assert_eq!(done.json["gene"], "STXBP1");
    assert!(atlas_llm::check_json(&schema, r#"{"gene":"invented"}"#).is_err());
    assert_eq!(done.calls.len(), 1);
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["response_format"], json!({"type":"json_object"}));
    let schema_prompt = body["messages"][0]["content"].as_str().unwrap();
    assert!(schema_prompt.contains(&schema.to_string()));
    assert!(done.calls[0].provenance.activity.parameters["sent.response_format"].starts_with("json_object"));
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({"response_format":{"type":"json_object"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply(r#"{"gene":"invented"}"#)))
        .expect(2)
        .mount(&server)
        .await;
    assert!(matches!(
        llm.complete_json::<Value>(&Call::new("json-object-endpoint"), req)
            .await,
        Err(LlmError::SchemaValidation(_))
    ));
}

#[tokio::test]
async fn openai_compatible_call_cache_and_provenance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {SECRET}").as_str()))
        .and(body_partial_json(
            json!({"model": "m1", "temperature": 0.0, "max_tokens": 50}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply("hello there")))
        .expect(1) // the second call must be a cache replay
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, true), dir.path(), CacheMode::ReadWrite);
    let req = CompletionRequest::new(vec![Message::system("be brief"), Message::user("hi")])
        .with_temperature(0.0)
        .with_max_tokens(50);
    let call = Call::new("mock")
        .with_key(ApiKey::new(SECRET))
        .with_inputs(["PMID:1", "ORPHA:558"]);

    let first = llm.complete(&call, req.clone()).await.unwrap();
    assert_eq!(first.response.text, "hello there");
    assert_eq!(first.response.reported_model.as_deref(), Some("served-model-v2"));
    assert_eq!(first.response.usage.input_tokens, Some(11));
    assert!(!first.response.cached);

    // Replay: no key needed, identical output, marked as a hit.
    let second = llm.complete(&Call::new("mock"), req).await.unwrap();
    assert!(second.response.cached);
    assert_eq!(second.response.text, first.response.text);
    assert_eq!(second.response.cache_key, first.response.cache_key);

    // Provenance: agent, used, generated, parameters actually sent.
    let p = &first.provenance;
    assert!(p.activity.id.starts_with("activity:llm-call:"));
    assert_eq!(p.activity.agent.name, "llm:openai-compatible/served-model-v2");
    assert_eq!(p.activity.parameters["model.requested"], "m1");
    assert_eq!(p.activity.parameters["sent.temperature"], "0");
    assert_eq!(p.activity.parameters["sent.max_tokens"], "50");
    assert_eq!(p.activity.parameters["cache"], "miss");
    assert_eq!(second.provenance.activity.parameters["cache"], "hit");
    assert_eq!(p.inputs, vec!["PMID:1", "ORPHA:558"]);
    assert!(p.prompt.id.starts_with("llm-prompt:") && p.response.id.starts_with("llm-response:"));
    let mut reg = Provenance::default();
    let idx = p.register(&mut reg);
    assert_eq!(reg.activity(idx).used.len(), 1);
    assert_eq!(reg.entities.len(), 2);

    // The key is nowhere on disk or in the record.
    let file = std::fs::read_to_string(dir.path().join(format!("{}.json", first.response.cache_key))).unwrap();
    assert!(!file.contains(SECRET));
    assert!(!serde_json::to_string(&first).unwrap().contains(SECRET));
}

#[tokio::test]
async fn replay_only_mode_misses_without_calling() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, false), dir.path(), CacheMode::ReplayOnly);
    let err = llm
        .complete(&Call::new("mock"), CompletionRequest::new(vec![Message::user("x")]))
        .await
        .unwrap_err();
    assert!(matches!(err, LlmError::CacheMiss(_)), "{err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn missing_key_auth_rate_limit_and_timeout() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, true), dir.path(), CacheMode::Off);
    let req = |s: &str| CompletionRequest::new(vec![Message::user(s)]);

    let e = llm.complete(&Call::new("mock"), req("a")).await.unwrap_err();
    assert!(matches!(e, LlmError::MissingKey { .. }), "{e}");

    Mock::given(body_partial_json(json!({"messages": [{"content": "auth"}]})))
        .respond_with(ResponseTemplate::new(401).set_body_string("{\"error\":\"bad key\"}"))
        .mount(&server)
        .await;
    Mock::given(body_partial_json(json!({"messages": [{"content": "busy"}]})))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .mount(&server)
        .await;
    Mock::given(body_partial_json(json!({"messages": [{"content": "slow"}]})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(openai_reply("late"))
                .set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let call = Call::new("mock").with_key(ApiKey::new(SECRET));
    let e = llm.complete(&call, req("auth")).await.unwrap_err();
    assert!(matches!(e, LlmError::Auth(_)), "{e}");
    assert!(!e.to_string().contains(SECRET));
    let e = llm.complete(&call, req("busy")).await.unwrap_err();
    assert!(matches!(e, LlmError::RateLimited(_)), "{e}");
    let e = llm
        .complete(&call, req("slow").with_deadline(Duration::from_millis(300)))
        .await
        .unwrap_err();
    assert!(matches!(e, LlmError::Timeout(_)), "{e}");
}

#[tokio::test]
async fn provider_errors_redact_echoed_keys_and_query_parameters() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, true), dir.path(), CacheMode::Off);
    for status in [401, 429, 500] {
        Mock::given(body_partial_json(
            json!({"messages": [{"content": status.to_string()}]}),
        ))
        .respond_with(ResponseTemplate::new(status).set_body_string(format!(
            "rejected {SECRET}; https://x.org/?api_key=query-secret&token=token-secret&db=pubmed"
        )))
        .mount(&server)
        .await;
        let err = llm
            .complete(
                &Call::new("mock").with_key(ApiKey::new(SECRET)),
                CompletionRequest::new(vec![Message::user(status.to_string())]),
            )
            .await
            .unwrap_err();
        for rendered in [err.to_string(), format!("{err:?}")] {
            for secret in [SECRET, "query-secret", "token-secret"] {
                assert!(!rendered.contains(secret), "secret leaked for {status}");
            }
            assert!(
                !rendered.contains("db=pubmed"),
                "provider metadata must not cross the error boundary"
            );
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct Pick {
    id: String,
}

#[tokio::test]
async fn provider_quota_json_never_exposes_account_metadata_or_upsell() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, true), dir.path(), CacheMode::Off);
    for status in [429, 500] {
        Mock::given(method("POST")).and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                "error":{"code":status,"message":"Upgrade your plan and add credits", "metadata":{"provider_name":"fixture-provider","raw":"sensitive-provider-json"}},
                "user_id":"fixture-provider-user-id", "api_key":SECRET
            }))).mount(&server).await;
        let error = llm
            .complete(
                &Call::new("mock").with_key(ApiKey::new(SECRET)),
                CompletionRequest::new(vec![Message::user("fixture query")]),
            )
            .await
            .unwrap_err();
        match (&error, status) {
            (LlmError::RateLimited(message), 429) => assert!(message.starts_with("model_rate_limited:")),
            (
                LlmError::Provider {
                    status: Some(received), ..
                },
                500,
            ) => assert_eq!(*received, 500),
            _ => panic!("provider status lost: {error:?}"),
        }
        for rendered in [error.to_string(), format!("{error:?}")] {
            for forbidden in [
                "Upgrade",
                "credits",
                "user_id",
                "fixture-provider-user-id",
                "fixture-provider",
                "sensitive-provider-json",
                SECRET,
            ] {
                assert!(!rendered.contains(forbidden), "raw provider metadata leaked");
            }
        }
        server.reset().await;
    }
}

#[tokio::test]
async fn complete_json_sends_schema_and_retries_once() {
    let server = MockServer::start().await;
    let schema = json!({"type": "object", "required": ["id"], "additionalProperties": false,
                        "properties": {"id": {"type": "string", "enum": ["ORPHA:558", "ORPHA:33"]}}});
    // First answer violates the enum; the retry (which carries the validation error) is fixed.
    Mock::given(body_partial_json(
        json!({"messages": [{"role": "user", "content": "pick"}]}),
    ))
    .and(body_partial_json(json!({"response_format": {"type": "json_schema"}})))
    .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply("{\"id\": \"ORPHA:999\"}")))
    .up_to_n_times(1)
    .mount(&server)
    .await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply("```json\n{\"id\": \"ORPHA:558\"}\n```")))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&compat(&server, false), dir.path(), CacheMode::ReadWrite);
    let req = CompletionRequest::new(vec![Message::user("pick")]).with_schema(JsonSchema::new("pick", schema.clone()));
    let out = llm
        .complete_json::<Pick>(&Call::new("mock"), req.clone())
        .await
        .unwrap();
    assert_eq!(out.value, Pick { id: "ORPHA:558".into() });
    assert_eq!(out.calls.len(), 2);
    let reqs = server.received_requests().await.unwrap();
    let retry: Value = serde_json::from_slice(&reqs[1].body).unwrap();
    let last = retry["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(last.contains("did not validate"), "{last}");

    // Both answers invalid → typed error (caller falls back to a template).
    let bad = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply("not json at all")))
        .expect(2)
        .mount(&bad)
        .await;
    let llm = Llm::new(
        Registry::from_toml(&compat(&bad, false), false).unwrap(),
        Cache::new(dir.path().join("b"), CacheMode::Off),
    );
    let e = llm.complete_json::<Pick>(&Call::new("mock"), req).await.unwrap_err();
    assert!(matches!(e, LlmError::SchemaValidation(_)), "{e}");
}

#[tokio::test]
async fn anthropic_and_gemini_adapters() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", SECRET))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test-1",
            "content": [{"type": "text", "text": "from claude"}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 4, "output_tokens": 2}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1beta/models/gemini-test:generateContent$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "from gemini"}], "role": "model"}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5},
            "modelVersion": "gemini-test-001"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_reply("from openrouter")))
        .mount(&server)
        .await;
    let toml = format!(
        "replace_defaults = true\n\
         [[connections]]\nname = \"a\"\npreset = \"anthropic\"\nbase_url = \"{u}/v1/\"\ndefault_model = \"claude-test\"\n\
         [[connections]]\nname = \"g\"\npreset = \"gemini\"\nbase_url = \"{u}/v1beta/\"\ndefault_model = \"gemini-test\"\n\
         [[connections]]\nname = \"o\"\npreset = \"openrouter\"\nbase_url = \"{u}/api/v1/\"\n",
        u = server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let llm = llm(&toml, dir.path(), CacheMode::Off);
    let req = CompletionRequest::new(vec![Message::user("hi")]);
    for (conn, text, model) in [
        ("a", "from claude", Some("claude-test-1")),
        ("g", "from gemini", None),
        ("o", "from openrouter", Some("served-model-v2")),
    ] {
        let out = llm
            .complete(&Call::new(conn).with_key(ApiKey::new(SECRET)), req.clone())
            .await
            .unwrap();
        assert_eq!(out.response.text, text, "{conn}");
        if model.is_some() {
            assert_eq!(out.response.reported_model.as_deref(), model, "{conn}");
        }
        // Cloud providers never use a server env key by default.
        let e = llm.complete(&Call::new(conn), req.clone()).await.unwrap_err();
        assert!(matches!(e, LlmError::MissingKey { .. }), "{conn}: {e}");
    }
}

#[tokio::test]
async fn probe_and_list_connections() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"object": "list", "data": [{"id": "m1"}, {"id": "m2"}]})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let toml = format!(
        "{}\n[[connections]]\nname = \"down\"\nkind = \"openai-compatible\"\nbase_url = \"http://127.0.0.1:9/v1\"\nkey_required = false\n",
        compat(&server, false)
    );
    let llm = llm(&toml, dir.path(), CacheMode::Off);
    let list = llm.list_connections(true).await;
    let mock = list.iter().find(|c| c.name == "mock").unwrap();
    let a = mock.availability.as_ref().unwrap();
    assert!(a.available, "{a:?}");
    assert_eq!(a.models, vec!["m1", "m2"]);
    assert!(!mock.needs_key);
    let down = list.iter().find(|c| c.name == "down").unwrap();
    assert!(!down.availability.as_ref().unwrap().available);
    // Listing never makes a completion call.
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET")
    );
}
