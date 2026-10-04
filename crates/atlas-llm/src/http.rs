//! HTTP providers on top of `genai` (D18): OpenAI, Anthropic, OpenRouter, Gemini and any
//! OpenAI-compatible base URL. The endpoint, key and model are passed per call as a
//! `ServiceTarget`, so no key is ever held in a client or a global resolver.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use genai::adapter::AdapterKind;
use genai::chat::{CacheControl, ChatMessage, ChatOptions, ChatRequest, ChatResponseFormat, JsonSpec};
use genai::resolver::{AuthData, Endpoint, ProviderConfig};
use genai::{Client, ModelIden, ServiceTarget};
use serde::{Deserialize, Serialize};

use crate::error::{LlmError, Result};
use crate::provider::{Availability, KeyPolicy, Provider, ProviderKind};
use crate::request::{CompletionRequest, ProviderOutput, Role, Usage};
use crate::secret::ApiKey;

const GENAI_AGENT: &str = "genai 0.6";
/// Probes are cheap and bounded.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Placeholder bearer for servers that need none (Ollama, LM Studio); never a real secret.
const NO_KEY: &str = "no-key";

/// The endpoint's supported structured-output contract. Local schema validation
/// remains mandatory in either mode; this never selects a different model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaMode {
    #[default]
    JsonSchema,
    /// For endpoints that explicitly support JSON objects but reject JSON Schema.
    JsonObject,
}

#[derive(Debug)]
pub struct HttpProvider {
    kind: ProviderKind,
    adapter: AdapterKind,
    /// Always ends with `/` (genai joins `chat/completions`).
    base_url: String,
    key_policy: KeyPolicy,
    /// Send `temperature` (off for models that reject sampling parameters, e.g. Claude Sonnet 5.5).
    sampling: bool,
    /// Anthropic prompt caching: send `cache_control` breakpoints (see [`breakpoints`]).
    prompt_cache: bool,
    /// OpenRouter `provider` routing object (D48: no data collection, ZDR endpoints only).
    provider_routing: Option<serde_json::Value>,
    schema_mode: SchemaMode,
    client: Client,
}

impl HttpProvider {
    pub fn new(kind: ProviderKind, base_url: Option<&str>, key_policy: KeyPolicy) -> Result<Self> {
        let (adapter, default_url) = match kind {
            ProviderKind::OpenAi => (AdapterKind::OpenAI, "https://api.openai.com/v1/"),
            ProviderKind::Anthropic => (AdapterKind::Anthropic, "https://api.anthropic.com/v1/"),
            ProviderKind::OpenRouter => (AdapterKind::OpenRouter, "https://openrouter.ai/api/v1/"),
            ProviderKind::Gemini => (AdapterKind::Gemini, "https://generativelanguage.googleapis.com/v1beta/"),
            ProviderKind::OpenAiCompatible => (AdapterKind::OpenAI, ""),
            other => return Err(LlmError::Config(format!("{} is not an HTTP provider", other.as_str()))),
        };
        let url = base_url.unwrap_or(default_url).trim();
        if url.is_empty() {
            return Err(LlmError::Config("openai-compatible connections need a base_url".into()));
        }
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(LlmError::Config(format!(
                "base_url must be http(s): {}",
                redact_error(url, None)
            )));
        }
        let base_url = if url.ends_with('/') {
            url.to_owned()
        } else {
            format!("{url}/")
        };
        Ok(Self {
            kind,
            adapter,
            base_url,
            key_policy,
            sampling: true,
            prompt_cache: true,
            provider_routing: None,
            schema_mode: SchemaMode::default(),
            client: Client::default(),
        })
    }

    /// OpenRouter provider-routing preferences, sent as the request's `provider` object.
    pub fn with_provider_routing(mut self, routing: Option<serde_json::Value>) -> Self {
        self.provider_routing = routing.filter(|_| self.kind == ProviderKind::OpenRouter);
        self
    }

    pub fn with_schema_mode(mut self, mode: SchemaMode) -> Self {
        self.schema_mode = mode;
        self
    }

    /// Whether to send Anthropic `cache_control` breakpoints (default on).
    pub fn with_prompt_cache(mut self, on: bool) -> Self {
        self.prompt_cache = on;
        self
    }

    /// Whether to send `temperature`; models that reject non-default sampling get `false`.
    pub fn with_sampling(mut self, sampling: bool) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn key_policy(&self) -> &KeyPolicy {
        &self.key_policy
    }

    fn auth(&self, key: Option<&ApiKey>) -> AuthData {
        AuthData::from_single(key.map_or(NO_KEY, ApiKey::expose))
    }

    fn chat_request(&self, req: &CompletionRequest, marks: &[usize]) -> ChatRequest {
        let mut messages = req
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let msg = match m.role {
                    Role::System => ChatMessage::system(m.content.clone()),
                    Role::User => ChatMessage::user(m.content.clone()),
                    Role::Assistant => ChatMessage::assistant(m.content.clone()),
                };
                if marks.contains(&i) {
                    msg.with_options(CacheControl::Ephemeral)
                } else {
                    msg
                }
            })
            .collect::<Vec<_>>();
        if self.schema_mode == SchemaMode::JsonObject
            && let Some(schema) = &req.schema
        {
            messages.insert(0, ChatMessage::system(format!(
                "Return only one JSON object matching this JSON Schema, without prose or code fences. The full schema is validated by the application:\n{}",
                schema.schema
            )));
        }
        ChatRequest::from_messages(messages)
    }

    /// Options sent to the provider, and the same facts as PROV-O parameters.
    fn options(&self, req: &CompletionRequest) -> (ChatOptions, BTreeMap<String, String>) {
        let mut sent = BTreeMap::new();
        let mut opts = ChatOptions::default().with_capture_usage(true);
        if self.kind == ProviderKind::OpenRouter {
            // The raw body carries OpenRouter's `usage.cost` and the routed `provider`.
            opts = opts.with_capture_raw_body(true);
        }
        if let Some(routing) = &self.provider_routing {
            opts = opts.with_extra_body(serde_json::json!({ "provider": routing }));
            sent.insert("provider".into(), routing.to_string());
        }
        if let Some(t) = req.settings.temperature {
            if self.sampling {
                opts = opts.with_temperature(t);
                sent.insert("temperature".into(), t.to_string());
            } else {
                sent.insert(
                    "temperature".into(),
                    "not-sent (model rejects sampling parameters)".into(),
                );
            }
        }
        if let Some(n) = req.settings.max_tokens {
            opts = opts.with_max_tokens(n);
            sent.insert("max_tokens".into(), n.to_string());
        }
        if let Some(schema) = &req.schema {
            if self.schema_mode == SchemaMode::JsonObject {
                opts = opts.with_response_format(ChatResponseFormat::JsonMode);
                sent.insert(
                    "response_format".into(),
                    "json_object;schema-in-prompt;validated-by-atlas".into(),
                );
                return (opts, sent);
            }
            // Anthropic rejects some JSON-schema keywords; they are dropped from the wire schema
            // only. The full schema is still enforced by our validator on the answer.
            let wire = if self.kind == ProviderKind::Anthropic {
                anthropic_schema(&schema.schema)
            } else {
                schema.schema.clone()
            };
            let spec = JsonSpec::new(schema.name.clone(), wire);
            opts = opts.with_response_format(ChatResponseFormat::JsonSpec(spec));
            sent.insert("response_format".into(), format!("json_schema:{}", schema.name));
        }
        (opts, sent)
    }
}

impl HttpProvider {
    /// Metadata only. A configured key or public catalogue alone is not a successful connection.
    async fn probe_with_key(&self, key: Option<&ApiKey>) -> Availability {
        let config =
            ProviderConfig::from_endpoint(Endpoint::from_owned(self.base_url.clone())).with_auth(self.auth(key));
        let metadata = async {
            // The OpenRouter catalogue is public. Authenticate separately, without
            // reading/returning its sensitive key label, usage or account metadata.
            // https://openrouter.ai/docs/api/api-reference/api-keys/get-current-api-key
            if self.kind == ProviderKind::OpenRouter
                && let Some(key) = key
            {
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|_| "unreachable")?;
                let response = client
                    .get(format!("{}key", self.base_url))
                    .bearer_auth(key.expose())
                    .send()
                    .await
                    .map_err(|_| "unreachable")?;
                if matches!(response.status().as_u16(), 401 | 403) {
                    return Err("auth-failed");
                }
                if !response.status().is_success() {
                    return Err("unreachable");
                }
            }
            self.client
                .all_model_names(self.adapter, config)
                .await
                .map_err(|e| match map_error(e, key) {
                    LlmError::Auth(_) => "auth-failed",
                    _ => "unreachable",
                })
        };
        match tokio::time::timeout(PROBE_TIMEOUT, metadata).await {
            Ok(Ok(models)) => Availability {
                models,
                ..Availability::ready("reachable")
            },
            Ok(Err(reason)) => Availability::not_ready(reason),
            Err(_) => Availability::not_ready("probe-timeout"),
        }
    }
}

#[async_trait]
impl Provider for HttpProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn base_url(&self) -> Option<&str> {
        Some(&self.base_url)
    }

    async fn probe(&self) -> Availability {
        let key = self.key_policy.resolve(None);
        if self.key_policy.needs_key() && key.is_none() {
            // Cloud providers: a per-request key will make it available; nothing to call.
            return Availability::not_ready("key-needed");
        }
        self.probe_with_key(key.as_ref()).await
    }

    async fn probe_for_key(&self, key: Option<&ApiKey>) -> Availability {
        let key = self.key_policy.resolve(key);
        if self.key_policy.needs_key() && key.is_none() {
            return Availability::not_ready("key-needed");
        }
        self.probe_with_key(key.as_ref()).await
    }

    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput> {
        if self.key_policy.needs_key() && key.is_none() {
            return Err(LlmError::MissingKey {
                connection: self.kind.as_str().into(),
                hint: "pass a key with the request".into(),
            });
        }
        let target = ServiceTarget {
            endpoint: Endpoint::from_owned(self.base_url.clone()),
            auth: self.auth(key),
            model: ModelIden::new(self.adapter, model.to_owned()),
        };
        let (opts, mut sent) = self.options(req);
        sent.insert("base_url".into(), self.base_url.clone());
        sent.insert("adapter".into(), format!("{:?}", self.adapter));
        let marks = if self.kind == ProviderKind::Anthropic && self.prompt_cache {
            breakpoints(req)
        } else {
            vec![]
        };
        if !marks.is_empty() {
            let at: Vec<String> = marks
                .iter()
                .map(|i| format!("{}#{i}", format!("{:?}", req.messages[*i].role).to_ascii_lowercase()))
                .collect();
            sent.insert("cache_breakpoints".into(), at.join(","));
        }
        let call = self
            .client
            .exec_chat(target, self.chat_request(req, &marks), Some(&opts));
        let res = tokio::time::timeout(req.deadline, call)
            .await
            .map_err(|_| LlmError::Timeout(req.deadline))?
            .map_err(|e| map_error(e, key))?;
        let stop_reason = res.stop_reason.as_ref().map(|s| s.raw().to_owned());
        if stop_reason.as_deref() == Some("refusal") {
            return Err(LlmError::Provider {
                status: None,
                message: "the model declined this request (refusal)".into(),
            });
        }
        let text = res.first_text().unwrap_or_default().to_owned();
        let reported = res.provider_model_iden.model_name.to_string();
        let usage = Usage {
            input_tokens: res.usage.prompt_tokens.and_then(|n| u64::try_from(n).ok()),
            output_tokens: res.usage.completion_tokens.and_then(|n| u64::try_from(n).ok()),
            cached_input_tokens: res
                .usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
                .and_then(|n| u64::try_from(n).ok()),
            cache_creation_input_tokens: res
                .usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cache_creation_tokens)
                .and_then(|n| u64::try_from(n).ok()),
            cost_usd: res
                .captured_raw_body
                .as_ref()
                .and_then(|b| b.pointer("/usage/cost"))
                .and_then(serde_json::Value::as_f64),
        };
        if let Some(p) = res
            .captured_raw_body
            .as_ref()
            .and_then(|b| b.get("provider"))
            .and_then(serde_json::Value::as_str)
        {
            // OpenRouter: which upstream endpoint served the call (PROV parameter).
            sent.insert("routed_provider".into(), p.to_owned());
        }
        Ok(ProviderOutput {
            text,
            reported_model: Some(reported).filter(|m| !m.is_empty()),
            usage,
            stop_reason,
            agent_version: Some(GENAI_AGENT.into()),
            sent,
        })
    }
}

/// Anthropic allows at most this many `cache_control` breakpoints per request.
pub const MAX_BREAKPOINTS: usize = 4;

/// Message indexes that get a prompt-cache breakpoint: the messages marked `cache` (the last
/// [`MAX_BREAKPOINTS`], which give the longest cached prefixes); if none is marked, the last
/// message of the leading run of system messages (by convention the stable prefix), unless it
/// is the only message.
pub fn breakpoints(req: &CompletionRequest) -> Vec<usize> {
    let marked: Vec<usize> = (0..req.messages.len())
        .filter(|i| req.messages[*i].cache && !req.messages[*i].content.is_empty())
        .collect();
    if !marked.is_empty() {
        return marked[marked.len().saturating_sub(MAX_BREAKPOINTS)..].to_vec();
    }
    let leading = req.messages.iter().take_while(|m| m.role == Role::System).count();
    if leading > 0 && leading < req.messages.len() && !req.messages[leading - 1].content.is_empty() {
        vec![leading - 1]
    } else {
        vec![]
    }
}

/// Keywords Anthropic structured outputs do not accept (numeric, string-length and array-size
/// constraints other than `minItems` 0/1).
fn anthropic_schema(schema: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match schema {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, v) in m {
                let drop = match k.as_str() {
                    "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" | "minLength"
                    | "maxLength" | "maxItems" | "uniqueItems" => true,
                    "minItems" => v.as_u64().is_some_and(|n| n > 1),
                    _ => false,
                };
                if !drop {
                    out.insert(k.clone(), anthropic_schema(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(anthropic_schema).collect()),
        other => other.clone(),
    }
}

/// Redact before truncating, including credentials echoed by a provider in response bodies.
fn redact_error(text: &str, key: Option<&ApiKey>) -> String {
    use regex::{Captures, Regex};
    use std::sync::LazyLock;
    static PARAM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r#"(?i)(\b(?:api[-_]?key|key|token|access[-_]?token|refresh[-_]?token|auth|authorization|password|secret|client[-_]?secret|signature|credential)=)[^&\s"'<>]*"#
    ).unwrap()
    });
    let mut clean = text.to_owned();
    if let Some(key) = key.filter(|k| !k.expose().is_empty()) {
        clean = clean.replace(key.expose(), "[REDACTED]");
        // Providers may JSON-escape an echoed key.
        let escaped = serde_json::to_string(key.expose()).unwrap();
        clean = clean.replace(&escaped[1..escaped.len() - 1], "[REDACTED]");
        let encoded: String = key
            .expose()
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        clean = clean
            .replace(&encoded, "[REDACTED]")
            .replace(&encoded.replace("%20", "+"), "[REDACTED]");
    }
    PARAM
        .replace_all(&clean, |caps: &Captures<'_>| format!("{}[REDACTED]", &caps[1]))
        .into_owned()
}

/// Retain actionable error types/statuses, never provider bodies or account metadata.
fn map_error(e: genai::Error, _key: Option<&ApiKey>) -> LlmError {
    use genai::Error as G;
    let status_err = |status: u16| match status {
        401 | 403 => LlmError::Auth("The selected model connection could not be authenticated.".into()),
        429 => LlmError::RateLimited("model_rate_limited: The selected model has reached its request limit.".into()),
        _ => LlmError::Provider {
            status: Some(status),
            message: format!("The selected model provider returned HTTP {status}."),
        },
    };
    match e {
        G::HttpError { status, .. } => status_err(status.as_u16()),
        G::WebModelCall { webc_error, .. } | G::WebAdapterCall { webc_error, .. } => match webc_error {
            genai::webc::Error::ResponseFailedStatus { status, .. } => status_err(status.as_u16()),
            genai::webc::Error::Reqwest(r) if r.is_connect() || r.is_timeout() => {
                LlmError::Unavailable("The selected model connection is unavailable.".into())
            }
            _ => LlmError::Provider {
                status: None,
                message: "The selected model provider returned an unreadable response.".into(),
            },
        },
        G::RequiresApiKey { .. } | G::NoAuthData { .. } => LlmError::MissingKey {
            connection: "http".into(),
            hint: "pass a key with the request".into(),
        },
        G::ChatResponseGeneration { .. } => {
            LlmError::BadOutput("The selected model returned an invalid response.".into())
        }
        G::ChatResponse { .. } => LlmError::Provider {
            status: None,
            message: "The selected model provider returned an unreadable response.".into(),
        },
        _ => LlmError::Provider {
            status: None,
            message: "The selected model provider returned an unreadable response.".into(),
        },
    }
}

#[cfg(test)]
mod connection_metadata_tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    #[tokio::test]
    async fn openrouter_connection_authenticates_before_public_catalogue_without_generation() {
        let server = MockServer::start().await;
        let key = ApiKey::new("fixture-connection-secret");
        let provider = HttpProvider::new(
            ProviderKind::OpenRouter,
            Some(&format!("{}/v1", server.uri())),
            KeyPolicy::None,
        )
        .unwrap();
        Mock::given(method("GET"))
            .and(path("/v1/key"))
            .and(header("authorization", "Bearer fixture-connection-secret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":{"label":"sensitive-key-label","usage":999}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"configured-model"}]})))
            .expect(1)
            .mount(&server)
            .await;
        let availability = provider.probe_for_key(Some(&key)).await;
        assert!(availability.available);
        assert_eq!(availability.models, vec!["configured-model"]);
        let public = serde_json::to_string(&availability).unwrap();
        assert!(!public.contains("sensitive-key-label") && !public.contains("fixture-connection-secret"));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method.as_str() == "GET")
        );
        server.verify().await;
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/v1/key"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error":"fixture-connection-secret"})))
            .expect(1)
            .mount(&server)
            .await;
        let failed = provider.probe_for_key(Some(&key)).await;
        assert!(!failed.available);
        assert_eq!(failed.reason, "auth-failed");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "failed credentials do not continue to the public catalogue"
        );
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("fixture-connection-secret")
        );
    }
}
