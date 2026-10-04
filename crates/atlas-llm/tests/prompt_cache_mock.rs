//! Anthropic prompt caching against a mock: where the `cache_control` breakpoints sit, that other
//! providers get none, that breakpoints never change the content-cache key, and that cache read /
//! write tokens are priced and recorded. No network.

use atlas_llm::cache::cache_key;
use atlas_llm::tasks::PREAMBLE;
use atlas_llm::{
    ApiKey, Cache, CacheMode, Call, CompletionRequest, Fact, FreeTierConfig, HOSTED_FREE, Llm, Message, ProviderKind,
    Registry, plain_summary,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-ant-test";

fn reply(text: &str, input: u64, write: u64, read: u64, output: u64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
        "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
        "usage": {"input_tokens": input, "cache_creation_input_tokens": write,
                  "cache_read_input_tokens": read, "output_tokens": output}
    }))
}

async fn anthropic(text: &str, usage: (u64, u64, u64, u64)) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(reply(text, usage.0, usage.1, usage.2, usage.3))
        .mount(&s)
        .await;
    s
}

fn llm(toml_body: &str) -> Llm {
    let toml = format!("replace_defaults = true\n{toml_body}");
    Llm::new(
        Registry::from_toml(&toml, false).unwrap(),
        Cache::new(std::env::temp_dir(), CacheMode::Off),
    )
}

fn anthropic_conn(server: &MockServer, extra: &str) -> String {
    format!(
        "[[connections]]\nname = \"a\"\npreset = \"anthropic\"\nbase_url = \"{}/v1/\"\ndefault_model = \"claude-sonnet-5-5\"\nsampling = false\n{extra}",
        server.uri()
    )
}

async fn body(server: &MockServer, i: usize) -> Value {
    serde_json::from_slice(&server.received_requests().await.unwrap()[i].body).unwrap()
}

fn has_cc(v: &Value) -> bool {
    v.get("cache_control").is_some_and(|c| c["type"] == "ephemeral")
}

#[tokio::test]
async fn breakpoints_sit_on_the_stable_prefix() {
    let s = anthropic("ok", (10, 0, 0, 1)).await;
    let llm = llm(&anthropic_conn(&s, ""));
    let call = Call::new("a").with_key(ApiKey::new(KEY));

    // Explicit: shared preamble + task instructions marked, request last and unmarked.
    let req = CompletionRequest::new(vec![
        Message::system("shared preamble").cached(),
        Message::system("task instructions").cached(),
        Message::user("dynamic facts + question"),
    ]);
    let done = llm.complete(&call, req).await.unwrap();
    let b = body(&s, 0).await;
    let system = b["system"].as_array().expect("system as blocks");
    assert_eq!(system.len(), 2);
    assert_eq!(system[0]["text"], "shared preamble");
    assert!(has_cc(&system[0]) && has_cc(&system[1]));
    assert_eq!(
        b["messages"][0]["content"], "dynamic facts + question",
        "dynamic tail stays uncached"
    );
    assert_eq!(
        done.provenance.activity.parameters["sent.cache_breakpoints"],
        "system#0,system#1"
    );

    // Automatic: nothing marked → the last leading system message.
    let req = CompletionRequest::new(vec![Message::system("stable system"), Message::user("q")]);
    llm.complete(&call, req).await.unwrap();
    let b = body(&s, 1).await;
    assert!(has_cc(&b["system"][0]), "{b}");
    assert_eq!(b["messages"][0]["content"], "q");

    // At most 4 breakpoints: the last four marked messages win.
    let mut msgs: Vec<Message> = (0..5).map(|i| Message::system(format!("s{i}")).cached()).collect();
    msgs.push(Message::user("turn").cached());
    msgs.push(Message::user("tail"));
    let done = llm.complete(&call, CompletionRequest::new(msgs)).await.unwrap();
    let b = body(&s, 2).await;
    let marked = b["system"].as_array().unwrap().iter().filter(|v| has_cc(v)).count()
        + b["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["content"].as_array().is_some_and(|c| c.iter().any(has_cc)))
            .count();
    assert_eq!(marked, 4, "{b}");
    assert!(!has_cc(&b["system"][0]) && has_cc(&b["system"][4]));
    assert_eq!(
        done.provenance.activity.parameters["sent.cache_breakpoints"],
        "system#2,system#3,system#4,user#5"
    );
}

#[tokio::test]
async fn tasks_put_the_shared_preamble_first() {
    let answer = json!({"sentences": [
        {"text": "X syndrome is a rare condition.", "cites": ["E1"]},
        {"text": "Children often have seizures.", "cites": ["E2"]}
    ]});
    let s = anthropic(&answer.to_string(), (40, 900, 0, 30)).await;
    let llm = llm(&anthropic_conn(&s, ""));
    let facts = [
        Fact::new("E1", "X syndrome is a rare condition."),
        Fact::new("E2", "Children often have seizures."),
    ];
    let out = plain_summary(
        &llm,
        &Call::new("a").with_key(ApiKey::new(KEY)),
        "X syndrome",
        &facts,
        "de",
    )
    .await;
    assert!(out.validation.passed, "{:?}", out.validation);
    let b = body(&s, 0).await;
    assert_eq!(b["system"][0]["text"], PREAMBLE, "byte-identical shared preamble first");
    assert!(has_cc(&b["system"][0]) && has_cc(&b["system"][1]));
    let task = b["system"][1]["text"].as_str().unwrap();
    assert!(
        !task.contains("German") && !task.contains("X syndrome"),
        "no request data in the cached prefix"
    );
    let user = b["messages"][0]["content"].as_str().unwrap();
    assert!(
        user.contains("German") && user.contains("[E1]"),
        "language and facts in the dynamic tail"
    );
    // Cache write tokens are counted in the PROV record.
    assert_eq!(
        out.calls[0].provenance.activity.counts["cache_creation_input_tokens"],
        900
    );
}

#[tokio::test]
async fn other_providers_and_opt_out_get_no_cache_control() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c", "object": "chat.completion", "created": 0, "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })))
        .mount(&s)
        .await;
    let llm = llm(&format!(
        "[[connections]]\nname = \"o\"\nkind = \"openai-compatible\"\nbase_url = \"{}/v1\"\ndefault_model = \"m\"\nkey_required = false\n",
        s.uri()
    ));
    let req = CompletionRequest::new(vec![Message::system("stable").cached(), Message::user("q")]);
    llm.complete(&Call::new("o"), req).await.unwrap();
    assert!(!body(&s, 0).await.to_string().contains("cache_control"));

    let a = anthropic("ok", (1, 0, 0, 1)).await;
    let llm = llm_off(&a);
    llm.complete(
        &Call::new("a").with_key(ApiKey::new(KEY)),
        CompletionRequest::new(vec![Message::system("stable").cached(), Message::user("q")]),
    )
    .await
    .unwrap();
    assert!(!body(&a, 0).await.to_string().contains("cache_control"));
}

fn llm_off(s: &MockServer) -> Llm {
    llm(&anthropic_conn(s, "prompt_cache = false\n"))
}

#[test]
fn breakpoints_do_not_change_the_content_cache_key() {
    let plain = CompletionRequest::new(vec![Message::system("s"), Message::user("q")]);
    let marked = CompletionRequest::new(vec![Message::system("s").cached(), Message::user("q")]);
    assert_eq!(
        cache_key(ProviderKind::Anthropic, None, "m", &plain),
        cache_key(ProviderKind::Anthropic, None, "m", &marked)
    );
}

#[tokio::test]
async fn cached_tokens_are_priced_in_the_spend_cap() {
    // 50 uncached + 2000 written + 10000 read input, 100 output (Sonnet 5.5 prices):
    // 50×2 + 2000×2.5 + 10000×0.2 + 100×10 = 100 + 5000 + 2000 + 1000 = 8100 µ$ = $0.0081
    let s = anthropic("ok", (50, 2000, 10_000, 100)).await;
    let toml = format!(
        "replace_defaults = true\n[[connections]]\nname = \"hosted-free\"\npreset = \"hosted-free\"\nkind = \"anthropic\"\n\
         default_model = \"claude-sonnet-5-5\"\nsampling = false\nbase_url = \"{}/v1/\"\n",
        s.uri()
    );
    // The free tier is provider-agnostic (D48); this one is configured on Anthropic.
    let mut reg = Registry::from_toml(&toml, false).unwrap();
    reg.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            server_key: Some(ApiKey::new(KEY)),
            spend_file: None,
            model: "claude-sonnet-5-5".into(),
            prices: atlas_llm::Prices::for_model("claude-sonnet-5-5").unwrap(),
            ..FreeTierConfig::default()
        },
    );
    let llm = Llm::new(reg, Cache::new(std::env::temp_dir(), CacheMode::Off));
    let req = CompletionRequest::new(vec![Message::system("stable").cached(), Message::user("q")]);
    let done = llm
        .complete(&Call::new(HOSTED_FREE).with_visitor("v"), req)
        .await
        .unwrap();
    assert!(
        (done.response.cost_usd.unwrap() - 0.0081).abs() < 1e-9,
        "{:?}",
        done.response.cost_usd
    );
    let p = &done.provenance.activity;
    assert_eq!(p.counts["cached_input_tokens"], 10_000);
    assert_eq!(p.counts["cache_creation_input_tokens"], 2000);
    assert_eq!(
        p.counts["input_tokens"], 12_050,
        "total prompt = uncached + written + read"
    );
    assert_eq!(p.parameters["free_tier.spent_usd_today"], "0.008100");
    assert_eq!(p.parameters["free_tier.daily_cap_usd"], "30.00");
    assert_eq!(p.parameters["free_tier.day"].len(), 10);
}
