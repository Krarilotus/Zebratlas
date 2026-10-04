//! The `Provider` trait every connection implements (HTTP API, local server, subscription CLI).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::request::{CompletionRequest, ProviderOutput};
use crate::secret::ApiKey;

/// Which adapter family a connection uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
    #[serde(rename = "openrouter")]
    OpenRouter,
    Gemini,
    /// Any `/v1/chat/completions` server: Ollama, LM Studio, vLLM, KISSKI, ...
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// Claude Code CLI on the user's machine (their own subscription login).
    ClaudeCode,
    /// OpenAI Codex CLI on the user's machine (their own ChatGPT login).
    CodexCli,
    /// Google Gemini CLI on the user's machine (their own Google login).
    GeminiCli,
    /// OpenCode's native CLI, restricted to verified zero-cost Zen models.
    #[serde(rename = "opencode-cli")]
    OpenCodeCli,
    /// Authenticated outbound session to a user's own machine.
    Connector,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::OpenRouter => "openrouter",
            Self::Gemini => "gemini",
            Self::OpenAiCompatible => "openai-compatible",
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex-cli",
            Self::GeminiCli => "gemini-cli",
            Self::OpenCodeCli => "opencode-cli",
            Self::Connector => "connector",
        }
    }

    /// Subscription CLIs run on the user's own machine (in-process locally, or via the connector).
    pub fn is_cli(self) -> bool {
        matches!(
            self,
            Self::ClaudeCode | Self::CodexCli | Self::GeminiCli | Self::OpenCodeCli
        )
    }
}

/// How a connection gets its key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum KeyPolicy {
    /// No key (local servers, CLIs that own their login).
    None,
    /// A key is required: per request (BYO from the UI), or from `env` when `env_fallback` is on
    /// (server-side batch jobs; off for paid providers by default so a hosted server never spends
    /// its own key on behalf of a visitor).
    Required { env: Option<String>, env_fallback: bool },
    /// A key may be sent (e.g. a local server behind a proxy) but is not needed.
    Optional { env: Option<String> },
}

impl KeyPolicy {
    pub fn needs_key(&self) -> bool {
        matches!(self, Self::Required { .. })
    }

    pub fn env_var(&self) -> Option<&str> {
        match self {
            Self::Required { env, .. } | Self::Optional { env } => env.as_deref(),
            Self::None => None,
        }
    }

    /// Per-request key wins; env only where allowed.
    pub(crate) fn resolve(&self, per_request: Option<&ApiKey>) -> Option<ApiKey> {
        if let Some(k) = per_request {
            return Some(k.clone());
        }
        match self {
            Self::Required {
                env: Some(var),
                env_fallback: true,
            }
            | Self::Optional { env: Some(var) } => ApiKey::from_env(var),
            _ => None,
        }
    }
}

/// What a probe saw. Probes never make a model call, never sign in, never read credentials.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    /// Ready to take a call (as far as a probe can tell).
    pub available: bool,
    /// CLI executable found (CLIs only).
    pub installed: Option<bool>,
    /// CLI version (CLIs only).
    pub version: Option<String>,
    /// CLI login state; `None` = could not tell.
    pub logged_in: Option<bool>,
    /// `subscription`, `api-key` or `unknown` (CLIs only).
    pub auth_mode: Option<String>,
    /// Models the server advertised (`GET /models`), if it was asked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Short, non-secret reason (`ready`, `cli-absent`, `signed-out`, `key-needed`, `unreachable` ...).
    pub reason: String,
}

impl Availability {
    pub fn ready(reason: impl Into<String>) -> Self {
        Self {
            available: true,
            reason: reason.into(),
            ..Self::default()
        }
    }
    pub fn not_ready(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: reason.into(),
            ..Self::default()
        }
    }
}

/// One LLM backend. Implementations are cheap to share (`Arc<dyn Provider>`).
#[async_trait]
pub trait Provider: Send + Sync + std::fmt::Debug {
    fn kind(&self) -> ProviderKind;

    /// Base URL for HTTP connections (non-secret); `None` for CLIs.
    fn base_url(&self) -> Option<&str> {
        None
    }

    /// Check reachability / installation / login without a model call.
    async fn probe(&self) -> Availability;

    /// Metadata-only probe using a request-scoped key, never retained by the provider.
    async fn probe_for_key(&self, _key: Option<&ApiKey>) -> Availability {
        self.probe().await
    }

    /// Run one completion. `model` is already resolved (request or connection default).
    /// `key` is the resolved key (never stored). Must respect `req.deadline`.
    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput>;
}
