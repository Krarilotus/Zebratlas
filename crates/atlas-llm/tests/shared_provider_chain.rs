//! In-process fake providers only: no credentials, probes or upstream requests.
use atlas_llm::request::{ProviderOutput, Usage};
use atlas_llm::{
    ApiKey, Availability, Cache, CacheMode, Call, CompletionRequest, FreeTierConfig, HOSTED_ANTHROPIC, HOSTED_FREE,
    HOSTED_GEMINI, HOSTED_KISSKI, JsonSchema, KeyPolicy, Llm, LlmError, Message, Prices, Provider, ProviderKind,
    QuotaReason, Registry,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
enum Reply {
    Status(u16),
    Text(&'static str),
    Slow,
}
#[derive(Debug)]
struct Fake {
    name: String,
    kind: ProviderKind,
    reply: Reply,
    seen: Arc<Mutex<Vec<String>>>,
}
#[async_trait::async_trait]
impl Provider for Fake {
    fn kind(&self) -> ProviderKind {
        self.kind
    }
    async fn probe(&self) -> Availability {
        Availability::ready("test-only")
    }
    async fn complete(
        &self,
        _req: &CompletionRequest,
        model: &str,
        _key: Option<&ApiKey>,
    ) -> atlas_llm::error::Result<ProviderOutput> {
        self.seen.lock().unwrap().push(self.name.clone());
        let text = match self.reply {
            Reply::Status(status) => {
                return Err(LlmError::Provider {
                    status: Some(status),
                    message: "fake upstream private metadata must not reach diagnostics".into(),
                });
            }
            Reply::Text(text) => text,
            Reply::Slow => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                "late"
            }
        };
        Ok(ProviderOutput {
            text: text.into(),
            reported_model: Some(model.into()),
            usage: Usage {
                input_tokens: Some(1),
                output_tokens: Some(1),
                ..Usage::default()
            },
            stop_reason: Some("stop".into()),
            agent_version: None,
            sent: BTreeMap::new(),
        })
    }
}
fn setup(replies: &[(&str, Reply)], cfg: FreeTierConfig) -> (Llm, Arc<Mutex<Vec<String>>>) {
    let mut registry=Registry::from_toml("replace_defaults=true\n[[connections]]\nname='hosted-free'\npreset='hosted-free'\n[[connections]]\nname='hosted-anthropic'\npreset='hosted-anthropic'\n[[connections]]\nname='hosted-gemini'\npreset='hosted-gemini'\n[[connections]]\nname='hosted-kisski'\npreset='hosted-kisski'\n",false).unwrap();
    registry.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            server_key: Some(ApiKey::new("fixture-primary-key")),
            ..cfg
        },
    );
    for name in [HOSTED_ANTHROPIC, HOSTED_GEMINI, HOSTED_KISSKI] {
        let mut view = registry.free_tier(name).unwrap().config().clone();
        view.server_key = Some(ApiKey::new("fixture-provider-key"));
        registry.set_free_tier(name, view);
    }
    let seen = Arc::new(Mutex::new(vec![]));
    for (name, reply) in replies {
        let mut connection = registry.get(name).unwrap().clone();
        connection.provider = Arc::new(Fake {
            name: (*name).into(),
            kind: connection.kind,
            reply: reply.clone(),
            seen: seen.clone(),
        });
        registry.insert(connection);
    }
    let llm = Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off)).with_fallbacks([
        HOSTED_ANTHROPIC,
        HOSTED_GEMINI,
        HOSTED_KISSKI,
    ]);
    (llm, seen)
}
fn cfg() -> FreeTierConfig {
    FreeTierConfig {
        spend_file: None,
        max_tokens: 32,
        ..FreeTierConfig::default()
    }
}
fn request() -> CompletionRequest {
    CompletionRequest::new(vec![Message::user("synthetic private input")])
}
fn call() -> Call {
    Call::new(HOSTED_FREE)
        .with_fallback(true)
        .with_private()
        .with_visitor("fixture-visitor")
}

#[tokio::test]
async fn configured_chain_exhausts_remote_failures_and_records_only_safe_metadata() {
    let (llm, seen) = setup(
        &[
            (HOSTED_FREE, Reply::Status(402)),
            (HOSTED_ANTHROPIC, Reply::Status(429)),
            (HOSTED_GEMINI, Reply::Status(503)),
            (HOSTED_KISSKI, Reply::Text("ok")),
        ],
        cfg(),
    );
    let done = llm.complete(&call(), request()).await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [HOSTED_FREE, HOSTED_ANTHROPIC, HOSTED_GEMINI, HOSTED_KISSKI]
    );
    assert_eq!(done.response.connection, HOSTED_KISSKI);
    assert_eq!(done.provenance.activity.parameters["fallback_attempt_count"], "4");
    let meta = serde_json::to_string(&done.provenance).unwrap();
    assert!(
        !meta.contains("fixture-provider-key")
            && !meta.contains("synthetic private input")
            && !meta.contains("private metadata")
    );
    assert!(done.provenance.prompt.file.is_empty() && done.provenance.response.file.is_empty());
}
#[tokio::test]
async fn schema_retry_stays_with_one_provider_before_next_validated_provider() {
    let (llm, seen) = setup(
        &[
            (HOSTED_FREE, Reply::Text("bad json")),
            (HOSTED_ANTHROPIC, Reply::Text("{\"answer\":\"valid\"}")),
        ],
        cfg(),
    );
    #[derive(Debug, serde::Deserialize)]
    struct Answer {
        answer: String,
    }
    let req = request().with_schema(JsonSchema::new(
        "answer",
        json!({"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}}),
    ));
    let done = llm.complete_json::<Answer>(&call(), req).await.unwrap();
    assert_eq!(done.value.answer, "valid");
    assert_eq!(*seen.lock().unwrap(), [HOSTED_FREE, HOSTED_FREE, HOSTED_ANTHROPIC]);
    assert_eq!(
        done.calls[0].provenance.activity.parameters["fallback_attempt_count"],
        "2"
    );
}
#[tokio::test]
async fn timeouts_leave_time_for_each_configured_provider_inside_one_deadline() {
    let (llm, seen) = setup(
        &[
            (HOSTED_FREE, Reply::Slow),
            (HOSTED_ANTHROPIC, Reply::Slow),
            (HOSTED_GEMINI, Reply::Text("ok")),
        ],
        cfg(),
    );
    let llm = llm.with_fallbacks([HOSTED_ANTHROPIC, HOSTED_GEMINI]);
    let start = Instant::now();
    let done = llm
        .complete(&call(), request().with_deadline(Duration::from_millis(240)))
        .await
        .unwrap();
    assert_eq!(done.response.connection, HOSTED_GEMINI);
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(*seen.lock().unwrap(), [HOSTED_FREE, HOSTED_ANTHROPIC, HOSTED_GEMINI]);
}
#[tokio::test]
async fn persistent_shared_views_use_one_ledger_and_model_specific_nonzero_prices() {
    let dir = tempfile::tempdir().unwrap();
    let (llm, _) = setup(
        &[],
        FreeTierConfig {
            spend_file: Some(dir.path().join("spend.json")),
            prices: Prices {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            ..cfg()
        },
    );
    let reg = llm.registry();
    let primary = reg.free_tier(HOSTED_FREE).unwrap();
    let anth = reg.free_tier(HOSTED_ANTHROPIC).unwrap();
    let gem = reg.free_tier(HOSTED_GEMINI).unwrap();
    let zero = reg.free_tier(HOSTED_KISSKI).unwrap();
    assert!(!primary.is_disabled() && !anth.is_disabled() && !gem.is_disabled() && !zero.is_disabled());
    assert!(anth.config().prices.input > 0.0 && gem.config().prices.output > 0.0);
    let cost = anth.estimate(20, 10);
    anth.admit("v", cost).unwrap().settle(Some(cost));
    assert!(primary.spent_today() > 0.0);
    assert_eq!(primary.spent_today(), gem.spent_today());
    let second = atlas_llm::free_tier::FreeTier::new(primary.config().clone());
    assert!(
        second.is_disabled(),
        "a genuinely independent writer must still fail the existing lock"
    );
}
#[tokio::test]
async fn zero_cost_fallback_survives_money_exhaustion_but_never_visitor_quota() {
    let dir = tempfile::tempdir().unwrap();
    let spend_file = dir.path().join("previous-spend.json");
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 86_400;
    // Valid prior spend can exceed a subsequently lowered monetary cap without
    // being an accounting mismatch or an operator kill switch.
    std::fs::write(&spend_file, serde_json::to_vec(&json!({"day":day,"usd":1.0})).unwrap()).unwrap();
    let (llm, seen) = setup(
        &[
            (HOSTED_FREE, Reply::Text("paid")),
            (HOSTED_ANTHROPIC, Reply::Text("paid")),
            (HOSTED_GEMINI, Reply::Text("paid")),
            (HOSTED_KISSKI, Reply::Text("zero")),
        ],
        FreeTierConfig {
            daily_usd: 0.0,
            per_visitor_per_hour: 1,
            spend_file: Some(spend_file),
            ..cfg()
        },
    );
    assert_eq!(llm.registry().free_tier(HOSTED_FREE).unwrap().spent_today(), 1.0);
    assert_eq!(
        llm.complete(&call(), request()).await.unwrap().response.connection,
        HOSTED_KISSKI
    );
    let err = llm.complete(&call(), request()).await.unwrap_err();
    assert!(matches!(
        err,
        LlmError::FreeQuota {
            reason: QuotaReason::VisitorHourly,
            ..
        }
    ));
    assert_eq!(*seen.lock().unwrap(), [HOSTED_KISSKI]);
}
#[tokio::test]
async fn global_rate_concurrency_and_disabled_never_escape_to_zero_cost_route() {
    for config in [
        FreeTierConfig { per_minute: 0, ..cfg() },
        FreeTierConfig {
            max_concurrent: 1,
            ..cfg()
        },
    ] {
        let (llm, seen) = setup(&[], config.clone());
        let permit = if config.per_minute > 0 {
            Some(
                llm.registry()
                    .free_tier(HOSTED_FREE)
                    .unwrap()
                    .admit("another", 0.0)
                    .unwrap(),
            )
        } else {
            None
        };
        let err = llm.complete(&call(), request()).await.unwrap_err();
        assert!(matches!(
            err,
            LlmError::FreeQuota {
                reason: QuotaReason::GlobalRate | QuotaReason::Busy,
                ..
            }
        ));
        assert!(seen.lock().unwrap().is_empty());
        drop(permit);
    }
    let (llm, seen) = setup(&[], cfg());
    llm.registry().free_tier(HOSTED_FREE).unwrap().set_disabled(true);
    assert_eq!(llm.default_chain(), [HOSTED_FREE]);
    let err = llm
        .complete(&Call::new(llm.default_connection(false)).with_fallback(true), request())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        LlmError::FreeQuota {
            reason: QuotaReason::Disabled,
            ..
        }
    ));
    assert!(seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn explicit_choice_own_key_and_selected_model_do_not_switch() {
    let (llm, seen) = setup(
        &[
            (HOSTED_FREE, Reply::Status(503)),
            (HOSTED_ANTHROPIC, Reply::Text("must not run")),
        ],
        cfg(),
    );
    assert!(llm.complete(&Call::new(HOSTED_FREE), request()).await.is_err());
    assert_eq!(*seen.lock().unwrap(), [HOSTED_FREE]);
    seen.lock().unwrap().clear();
    assert!(
        llm.complete(&call(), request().with_model("openai/gpt-oss-120b"))
            .await
            .is_err()
    );
    assert_eq!(*seen.lock().unwrap(), [HOSTED_FREE]);
    seen.lock().unwrap().clear();
    let mut registry = llm.registry().clone();
    let mut own = registry.get(HOSTED_FREE).unwrap().clone();
    own.config.name = "own".into();
    own.config.free_tier = None;
    own.config.shared_budget = None;
    own.key_policy = KeyPolicy::Required {
        env: None,
        env_fallback: false,
    };
    registry.insert(own);
    let llm = llm.with_registry(registry);
    assert!(
        llm.complete(
            &Call::new("own")
                .with_key(ApiKey::new("fixture-own-key"))
                .with_fallback(true),
            request()
        )
        .await
        .is_err()
    );
    assert_eq!(*seen.lock().unwrap(), [HOSTED_FREE]);
}

#[tokio::test]
async fn internal_aliases_stay_in_chain_but_not_public_picker_or_personal_fallback() {
    let (llm, _) = setup(&[], cfg());
    let public = llm.list_connections(false).await;
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].name, HOSTED_FREE);
    assert_eq!(
        llm.default_chain(),
        [HOSTED_FREE, HOSTED_ANTHROPIC, HOSTED_GEMINI, HOSTED_KISSKI]
    );
    let mut registry = llm.registry().clone();
    let mut personal = registry.get(HOSTED_KISSKI).unwrap().clone();
    personal.config.name = "personal-connector".into();
    personal.config.free_tier = None;
    personal.config.shared_budget = None;
    personal.kind = ProviderKind::Connector;
    personal.key_policy = KeyPolicy::None;
    registry.insert(personal);
    let llm = llm
        .with_registry(registry)
        .with_fallbacks(["personal-connector", HOSTED_KISSKI]);
    assert_eq!(llm.default_chain(), [HOSTED_FREE, HOSTED_KISSKI]);
}

#[test]
fn academic_zero_price_is_pinned_to_the_declared_endpoint() {
    assert!(Prices::for_model("openai-gpt-oss-120b").is_none());
    for override_text in [
        "base_url='https://other.invalid/v1'",
        "default_model='unknown-paid-model'",
        "kind='openrouter'",
    ] {
        let config = format!(
            "replace_defaults=true\n[[connections]]\nname='hosted-free'\npreset='hosted-free'\n[[connections]]\nname='hosted-kisski'\npreset='hosted-kisski'\n{override_text}\n"
        );
        assert!(Registry::from_toml(&config, false).is_err());
    }
}
