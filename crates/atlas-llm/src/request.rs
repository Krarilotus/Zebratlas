//! One request/response model for every provider kind.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default deadline for one call.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Prompt-cache breakpoint after this message (Anthropic `cache_control`): mark the *last*
    /// message of a prefix that stays identical across requests (system prompt, schemas,
    /// few-shot examples, a shared evidence preamble). Dynamic content goes after it.
    /// Not serialised: it never changes the content-cache key or the prompt hash.
    #[serde(skip_serializing, default)]
    pub cache: bool,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            cache: false,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            cache: false,
        }
    }
    /// Mark this message as the end of a stable, cacheable prefix.
    pub fn cached(mut self) -> Self {
        self.cache = true;
        self
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            cache: false,
        }
    }
}

/// A JSON schema for structured output. `name` only matters to OpenAI (`[A-Za-z0-9_-]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonSchema {
    pub name: String,
    pub schema: serde_json::Value,
}

impl JsonSchema {
    pub fn new(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
        }
    }
}

/// Sampling settings. `None` = provider default (and not sent).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// A completion request. The model is optional; the connection's default is used when absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: Option<String>,
    pub schema: Option<JsonSchema>,
    pub settings: Settings,
    /// Wall-clock bound for the whole call (HTTP request or CLI process lifetime).
    #[serde(with = "duration_ms")]
    pub deadline: Duration,
}

impl CompletionRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            model: None,
            schema: None,
            settings: Settings::default(),
            deadline: DEFAULT_DEADLINE,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
    pub fn with_schema(mut self, schema: JsonSchema) -> Self {
        self.schema = Some(schema);
        self
    }
    pub fn with_temperature(mut self, t: f64) -> Self {
        self.settings.temperature = Some(t);
        self
    }
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.settings.max_tokens = Some(n);
        self
    }
    pub fn with_deadline(mut self, d: Duration) -> Self {
        self.deadline = d;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    /// Tokens written to the provider's prompt cache (Anthropic bills them at a premium).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// What the provider says it charged, in USD (OpenRouter `usage.cost`, returned with every
    /// response). Preferred over the price table for the free tier's spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// What one provider call returned (before cache/provenance bookkeeping).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderOutput {
    pub text: String,
    /// The model the provider says it used (may differ from the requested alias).
    pub reported_model: Option<String>,
    pub usage: Usage,
    pub stop_reason: Option<String>,
    /// Software that made the call: CLI version (`claude 2.1.288`) or HTTP client (`genai 0.6.5`).
    pub agent_version: Option<String>,
    /// Settings that were actually put on the wire (`temperature`, `max_tokens`, `response_format`,
    /// CLI flags ...). Recorded as PROV-O parameters.
    pub sent: std::collections::BTreeMap<String, String>,
}

/// The response handed to callers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub connection: String,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub text: String,
    /// Parsed and schema-validated JSON when the request had a schema.
    pub json: Option<serde_json::Value>,
    pub usage: Usage,
    pub stop_reason: Option<String>,
    /// Latency of the original call (a cache replay keeps the original value).
    pub latency_ms: u64,
    /// `true` when served from the content cache.
    pub cached: bool,
    /// Content key of this request (`sha256`, hex).
    pub cache_key: String,
    /// USD cost by the connection's price table (free tier only; `0` on a cache hit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Which model produced this, for the UI (D48): catalog message key + params + fallback.
    #[serde(default)]
    pub model_label: crate::model_label::ModelLabel,
}

mod duration_ms {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}
