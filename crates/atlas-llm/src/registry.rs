//! Named connections: built-in presets, overridable from TOML (`ATLAS_LLM_CONFIG`).
//!
//! ```toml
//! [[connections]]
//! name = "my-vllm"
//! preset = "vllm"                      # start from a preset (optional)
//! base_url = "http://gpu-box:8000/v1"
//! default_model = "Qwen/Qwen3-32B"
//!
//! [[connections]]
//! name = "hosted-free"                 # D23/D48: the only paid connection on a server key
//! default_model = "openai/gpt-oss-120b" # limits: ATLAS_FREE_* env vars (see free_tier.rs)
//! provider_routing = { data_collection = "deny", zdr = true }   # OpenRouter only
//! ```
//!
//! Paid cloud providers (OpenAI, Anthropic, OpenRouter, Gemini) take the key with the request.
//! A server env key is allowed only for a connection with `free_tier = true` (D23), which puts
//! it behind the free-tier limits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cli::{claude::ClaudeCode, codex, codex::CodexCli, gemini::GeminiCli, opencode::OpenCodeCli};
use crate::error::{LlmError, Result};
use crate::free_tier::{
    DEFAULT_FREE_MODEL, FreeTier, FreeTierConfig, FreeTierStatus, GPT_OSS_120B_MAX_PRICE, HOSTED_ANTHROPIC,
    HOSTED_FREE, HOSTED_GEMINI, HOSTED_KISSKI, Prices,
};
use crate::http::{HttpProvider, SchemaMode};
use crate::provider::{Availability, KeyPolicy, Provider, ProviderKind};

pub const KISSKI_BASE_URL: &str = "https://chat-ai.academiccloud.de/v1";
pub const KISSKI_MODEL: &str = "openai-gpt-oss-120b";

/// One connection as configured (TOML / presets). No secrets in here: only env var *names*.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionConfig {
    pub name: String,
    pub preset: Option<String>,
    pub kind: Option<ProviderKind>,
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    /// Env var with a server-side key (batch jobs, demo presets).
    pub key_env: Option<String>,
    /// `Some(false)` for servers that need no key; default: cloud APIs need one.
    pub key_required: Option<bool>,
    /// Allow the env key when no per-request key is given. Off for paid cloud providers by default.
    pub env_fallback: Option<bool>,
    /// Shown as "demo only" in the UI (e.g. KISSKI's free academic endpoint).
    pub demo_only: Option<bool>,
    /// CLIs: explicit native executable (no shell shims).
    pub executable: Option<PathBuf>,
    /// CLIs: concurrent calls (default 1).
    pub max_parallel: Option<usize>,
    /// D23: the project's server key behind the free-tier limits (`ATLAS_FREE_*`).
    pub free_tier: Option<bool>,
    /// Model-specific hosted guard sharing another connection's entire budget.
    /// Required for the additional hosted presets; server keys remain distinct.
    pub shared_budget: Option<String>,
    /// Implementation-only hosted aliases are omitted from the public model picker.
    pub internal: Option<bool>,
    /// Send `temperature` (default true; false for models that reject sampling parameters).
    pub sampling: Option<bool>,
    /// Anthropic prompt-cache breakpoints (default true).
    pub prompt_cache: Option<bool>,
    /// HTTP endpoints that reject `json_schema` can explicitly use `json-object`.
    /// The schema is then included in the prompt and still validated locally.
    pub schema_mode: Option<SchemaMode>,
    /// OpenRouter provider-routing preferences, sent as the request's `provider` object
    /// (https://openrouter.ai/docs/guides/routing/routers/provider-routing: `data_collection`,
    /// `zdr`, `require_parameters`, `max_price`, `order`, `only`, `sort`, ...). OpenRouter only.
    pub provider_routing: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    /// Drop the built-in presets and use only the listed connections.
    #[serde(default)]
    replace_defaults: bool,
    #[serde(default)]
    connections: Vec<ConnectionConfig>,
}

fn preset(name: &str) -> Option<ConnectionConfig> {
    use ProviderKind as K;
    let c = |kind, label: &str, base: Option<&str>, model: Option<&str>, key_env: Option<&str>, key_required| {
        ConnectionConfig {
            name: name.into(),
            kind: Some(kind),
            label: Some(label.into()),
            base_url: base.map(Into::into),
            default_model: model.map(Into::into),
            key_env: key_env.map(Into::into),
            key_required: Some(key_required),
            ..ConnectionConfig::default()
        }
    };
    let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5-mini".into());
    let free_model = std::env::var("ATLAS_FREE_MODEL")
        .ok()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_FREE_MODEL.into());
    let ollama_url = std::env::var("ATLAS_OLLAMA_URL")
        .ok()
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "http://localhost:11434/v1".into());
    Some(match name {
        "openai" => c(
            K::OpenAi,
            "OpenAI (your key)",
            None,
            Some(&openai_model),
            Some("OPENAI_API_KEY"),
            true,
        ),
        "openai-hosted" => ConnectionConfig {
            env_fallback: Some(true),
            free_tier: Some(true),
            sampling: Some(false),
            models: vec!["gpt-5.4-mini".into()],
            ..c(
                K::OpenAi,
                "OpenAI (hosted backup)",
                None,
                Some("gpt-5.4-mini"),
                Some("OPENAI_API_KEY"),
                true,
            )
        },
        "anthropic" => c(
            K::Anthropic,
            "Anthropic (your key)",
            None,
            Some("claude-sonnet-4-6"),
            Some("ANTHROPIC_API_KEY"),
            true,
        ),
        HOSTED_ANTHROPIC => ConnectionConfig {
            internal: Some(true),
            free_tier: Some(true),
            env_fallback: Some(true),
            shared_budget: Some(HOSTED_FREE.into()),
            ..c(
                K::Anthropic,
                "Hosted Anthropic (shared budget)",
                None,
                Some("claude-sonnet-4-6"),
                Some("ANTHROPIC_API_KEY"),
                true,
            )
        },
        HOSTED_GEMINI => ConnectionConfig {
            internal: Some(true),
            free_tier: Some(true),
            env_fallback: Some(true),
            shared_budget: Some(HOSTED_FREE.into()),
            ..c(
                K::Gemini,
                "Hosted Gemini (shared budget)",
                None,
                Some("gemini-2.5-flash"),
                Some("GEMINI_API_KEY"),
                true,
            )
        },
        HOSTED_KISSKI => ConnectionConfig {
            internal: Some(true),
            free_tier: Some(true),
            env_fallback: Some(true),
            shared_budget: Some(HOSTED_FREE.into()),
            demo_only: Some(true),
            ..c(
                K::OpenAiCompatible,
                "Hosted KISSKI (shared request limits)",
                Some(KISSKI_BASE_URL),
                Some(KISSKI_MODEL),
                Some("KISSKI_API_KEY"),
                true,
            )
        },
        // D48: OpenAI open-weight model via OpenRouter, private endpoints only.
        HOSTED_FREE => ConnectionConfig {
            env_fallback: Some(true),
            free_tier: Some(true),
            models: vec![free_model.clone()],
            provider_routing: Some(free_routing(&free_model)),
            ..c(
                K::OpenRouter,
                "Free assistant (no key needed)",
                None,
                Some(&free_model),
                Some("OPENROUTER_API_KEY"),
                true,
            )
        },
        "openrouter" => c(
            K::OpenRouter,
            "OpenRouter (your key)",
            None,
            Some("openai/gpt-oss-120b"),
            Some("OPENROUTER_API_KEY"),
            true,
        ),
        "gemini" => c(
            K::Gemini,
            "Google Gemini (your key)",
            None,
            Some("gemini-3.8-flash"),
            Some("GEMINI_API_KEY"),
            true,
        ),
        "gemini-free" | "gemini-lite-free" => {
            let model = if name == "gemini-lite-free" {
                "gemini-3.5-flash-lite"
            } else {
                "gemini-3.8-flash"
            };
            ConnectionConfig {
                env_fallback: Some(true),
                free_tier: Some(true),
                models: vec![model.into()],
                ..c(
                    K::Gemini,
                    "Google Gemini (hosted)",
                    None,
                    Some(model),
                    Some("GEMINI_API_KEY"),
                    true,
                )
            }
        }
        // D48: local OpenAI open-weight model; `ATLAS_OLLAMA_URL` points at the server and puts
        // it into the free tier's fallback chain.
        "ollama" => ConnectionConfig {
            models: vec!["gpt-oss:20b".into(), "gpt-oss:120b".into()],
            ..c(
                K::OpenAiCompatible,
                "Ollama (local)",
                Some(&ollama_url),
                Some("gpt-oss:20b"),
                None,
                false,
            )
        },
        "lmstudio" => c(
            K::OpenAiCompatible,
            "LM Studio (local)",
            Some("http://localhost:1234/v1"),
            None,
            None,
            false,
        ),
        "vllm" => c(
            K::OpenAiCompatible,
            "vLLM (local)",
            Some("http://localhost:8000/v1"),
            None,
            None,
            false,
        ),
        "kisski" => ConnectionConfig {
            env_fallback: Some(true),
            demo_only: Some(true),
            models: vec![KISSKI_MODEL.into()],
            ..c(
                K::OpenAiCompatible,
                "KISSKI Chat AI (demo only)",
                Some(KISSKI_BASE_URL),
                Some(KISSKI_MODEL),
                Some("KISSKI_API_KEY"),
                true,
            )
        },
        "claude-code" => ConnectionConfig {
            models: vec!["sonnet".into(), "opus".into(), "haiku".into()],
            ..c(
                K::ClaudeCode,
                "Claude Code (your subscription)",
                None,
                Some("sonnet"),
                None,
                false,
            )
        },
        "codex" => c(
            K::CodexCli,
            "Codex CLI (your ChatGPT plan)",
            None,
            Some(codex::CLI_DEFAULT_MODEL),
            None,
            false,
        ),
        "gemini-cli" => c(
            K::GeminiCli,
            "Gemini CLI (your Google login)",
            None,
            Some("gemini-2.5-flash"),
            None,
            false,
        ),
        "opencode" => c(
            K::OpenCodeCli,
            "OpenCode (free models)",
            None,
            Some(crate::cli::opencode::DEFAULT_MODEL),
            None,
            false,
        ),
        _ => return None,
    })
}

/// OpenRouter routing for the hosted free tier (D41 privacy, D48). Field names from
/// https://openrouter.ai/docs/guides/routing/routers/provider-routing (read 2026-10-04):
/// `data_collection: "deny"` (only providers that do not store or train on prompts),
/// `zdr: true` (only zero-data-retention endpoints), `require_parameters: true` (only endpoints
/// that honour every parameter sent, incl. `response_format` json_schema), and for the default
/// model `max_price` (USD per MTok) at the worst-case ZDR price the spend cap is priced with.
pub fn free_routing(model: &str) -> serde_json::Value {
    let mut v = serde_json::json!({
        "data_collection": "deny",
        "zdr": true,
        "require_parameters": true,
    });
    if model == DEFAULT_FREE_MODEL {
        let (prompt, completion) = GPT_OSS_120B_MAX_PRICE;
        v["max_price"] = serde_json::json!({ "prompt": prompt, "completion": completion });
    }
    v
}

pub const PRESETS: &[&str] = &[
    HOSTED_FREE,
    HOSTED_ANTHROPIC,
    HOSTED_GEMINI,
    HOSTED_KISSKI,
    "openai",
    "openai-hosted",
    "anthropic",
    "openrouter",
    "gemini",
    "gemini-free",
    "gemini-lite-free",
    "ollama",
    "lmstudio",
    "vllm",
    "kisski",
    "claude-code",
    "codex",
    "gemini-cli",
    "opencode",
];

/// Overlay `over` on `base` (fields set in `over` win).
fn merge(base: ConnectionConfig, over: ConnectionConfig) -> ConnectionConfig {
    ConnectionConfig {
        name: over.name,
        preset: over.preset.or(base.preset),
        kind: over.kind.or(base.kind),
        label: over.label.or(base.label),
        base_url: over.base_url.or(base.base_url),
        default_model: over.default_model.or(base.default_model),
        models: if over.models.is_empty() {
            base.models
        } else {
            over.models
        },
        key_env: over.key_env.or(base.key_env),
        key_required: over.key_required.or(base.key_required),
        env_fallback: over.env_fallback.or(base.env_fallback),
        demo_only: over.demo_only.or(base.demo_only),
        executable: over.executable.or(base.executable),
        max_parallel: over.max_parallel.or(base.max_parallel),
        free_tier: over.free_tier.or(base.free_tier),
        shared_budget: over.shared_budget.or(base.shared_budget),
        internal: over.internal.or(base.internal),
        sampling: over.sampling.or(base.sampling),
        prompt_cache: over.prompt_cache.or(base.prompt_cache),
        schema_mode: over.schema_mode.or(base.schema_mode),
        // Routing preferences belong to the base's provider; a different `kind` drops them.
        provider_routing: over.provider_routing.or(base
            .provider_routing
            .filter(|_| over.kind.is_none() || over.kind == base.kind)),
    }
}

/// A connection ready to call.
#[derive(Debug, Clone)]
pub struct Connection {
    pub config: ConnectionConfig,
    pub kind: ProviderKind,
    pub key_policy: KeyPolicy,
    pub provider: Arc<dyn Provider>,
}

impl Connection {
    pub fn build(config: ConnectionConfig) -> Result<Self> {
        let kind = config
            .kind
            .ok_or_else(|| LlmError::Config(format!("connection '{}' has no kind", config.name)))?;
        let key_policy = if kind.is_cli() {
            KeyPolicy::None
        } else if config.key_required.unwrap_or(kind != ProviderKind::OpenAiCompatible) {
            KeyPolicy::Required {
                env: config.key_env.clone(),
                env_fallback: config.env_fallback.unwrap_or(false),
            }
        } else if config.key_env.is_some() {
            KeyPolicy::Optional {
                env: config.key_env.clone(),
            }
        } else {
            KeyPolicy::None
        };
        let paid = matches!(
            kind,
            ProviderKind::OpenAi | ProviderKind::Anthropic | ProviderKind::OpenRouter | ProviderKind::Gemini
        );
        if paid && config.env_fallback == Some(true) && config.free_tier != Some(true) {
            return Err(LlmError::Config(format!(
                "connection '{}': a paid provider may use a server env key only as a free tier (free_tier = true, D23)",
                config.name
            )));
        }
        if config.shared_budget.is_some() && config.free_tier != Some(true) {
            return Err(LlmError::Config("shared_budget requires free_tier=true".into()));
        }
        if config.provider_routing.is_some() && kind != ProviderKind::OpenRouter {
            return Err(LlmError::Config(format!(
                "connection '{}': provider_routing is an OpenRouter option",
                config.name
            )));
        }
        if config.free_tier == Some(true) && kind.is_cli() {
            return Err(LlmError::Config(format!(
                "connection '{}': free_tier needs an HTTP provider",
                config.name
            )));
        }
        let par = config.max_parallel.unwrap_or(1);
        let provider: Arc<dyn Provider> = match kind {
            ProviderKind::Connector => {
                return Err(LlmError::Config(
                    "connector connections require an authenticated runtime transport".into(),
                ));
            }
            ProviderKind::ClaudeCode => Arc::new(ClaudeCode::new(config.executable.clone(), par)?),
            ProviderKind::CodexCli => Arc::new(CodexCli::new(config.executable.clone(), par)?),
            ProviderKind::GeminiCli => Arc::new(GeminiCli::new(config.executable.clone(), par)?),
            ProviderKind::OpenCodeCli => Arc::new(OpenCodeCli::new(config.executable.clone(), par)?),
            _ => Arc::new(
                HttpProvider::new(kind, config.base_url.as_deref(), key_policy.clone())?
                    .with_sampling(config.sampling.unwrap_or(true))
                    .with_prompt_cache(config.prompt_cache.unwrap_or(true))
                    .with_schema_mode(config.schema_mode.unwrap_or_default())
                    .with_provider_routing(config.provider_routing.clone()),
            ),
        };
        Ok(Self {
            config,
            kind,
            key_policy,
            provider,
        })
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Public description for `list_connections()`; `availability` only when probed.
    pub fn info(&self, availability: Option<Availability>) -> ConnectionInfo {
        let env = self.key_policy.env_var().map(str::to_owned);
        ConnectionInfo {
            name: self.config.name.clone(),
            label: self.config.label.clone().unwrap_or_else(|| self.config.name.clone()),
            kind: self.kind,
            runs_on: if self.kind.is_cli() || self.kind == ProviderKind::Connector {
                "user-machine"
            } else {
                "server"
            }
            .into(),
            base_url: self.provider.base_url().map(str::to_owned),
            default_model: self.config.default_model.clone(),
            model_label: self
                .config
                .default_model
                .as_deref()
                .map(crate::model_label::model_label),
            models: self.config.models.clone(),
            needs_key: self.key_policy.needs_key(),
            key_from_env_available: matches!(
                self.key_policy,
                KeyPolicy::Required { env_fallback: true, .. } | KeyPolicy::Optional { .. }
            ) && env
                .as_deref()
                .is_some_and(|v| std::env::var(v).is_ok_and(|s| !s.trim().is_empty())),
            key_env: env,
            demo_only: self.config.demo_only.unwrap_or(false),
            availability,
            free_tier: None,
        }
    }
}

/// What the UI / server sees about a connection. Never contains a key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub label: String,
    pub kind: ProviderKind,
    /// `server` (HTTP from atlas-server) or `user-machine` (CLI: local server or connector).
    pub runs_on: String,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    /// `default_model` as the UI names it (D48), catalog message key + params + fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_label: Option<crate::model_label::ModelLabel>,
    pub models: Vec<String>,
    /// A key is needed (per request, unless `key_from_env_available`).
    pub needs_key: bool,
    pub key_env: Option<String>,
    /// The server may use its env key for this connection.
    pub key_from_env_available: bool,
    pub demo_only: bool,
    pub availability: Option<Availability>,
    /// Free-tier limits and remaining quota (D23), for `hosted-free`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_tier: Option<FreeTierStatus>,
}

/// The set of named connections.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    connections: BTreeMap<String, Connection>,
    /// Free-tier guards by connection name (shared by clones).
    free_tiers: BTreeMap<String, Arc<FreeTier>>,
}

impl Registry {
    /// Server-owned query task. Keep the *same* quota/spend guard and personal connections.
    /// Only the production OpenRouter hosted connection changes; test/custom providers remain intact.
    pub fn for_query_planning(&self) -> Result<Self> {
        let mut registry = self.clone();
        if let Some(hosted) = self.connections.get(HOSTED_FREE)
            && hosted.kind == ProviderKind::OpenRouter
        {
            let mut config = hosted.config.clone();
            config.default_model = Some("openai/gpt-6.1-sol".into());
            config.models = vec!["openai/gpt-6.1-sol".into()];
            config.provider_routing = Some(serde_json::json!({
                "data_collection":"deny", "zdr":true, "require_parameters":true,
                "max_price":{"prompt":2.0,"completion":10.0}
            }));
            config.prompt_cache = Some(false);
            registry
                .connections
                .insert(HOSTED_FREE.into(), Connection::build(config)?);
        }
        Ok(registry)
    }

    /// All presets. `include_cli = false` for a hosted server (it cannot reach the user's CLIs).
    pub fn presets(include_cli: bool) -> Result<Self> {
        let mut r = Self::default();
        for name in PRESETS {
            let cfg = preset(name).expect("preset exists");
            if !include_cli && cfg.kind.is_some_and(ProviderKind::is_cli) {
                continue;
            }
            r.add(Connection::build(cfg)?)?;
        }
        Ok(r)
    }

    /// Presets plus/overridden by a TOML file.
    pub fn from_toml(text: &str, include_cli: bool) -> Result<Self> {
        let file: ConfigFile = toml::from_str(text).map_err(|e| LlmError::Config(e.to_string()))?;
        let mut r = if file.replace_defaults {
            Self::default()
        } else {
            Self::presets(include_cli)?
        };
        for over in file.connections {
            let base_name = over.preset.clone().unwrap_or_else(|| over.name.clone());
            let base = r
                .connections
                .get(&over.name)
                .map(|c| c.config.clone())
                .or_else(|| preset(&base_name))
                .unwrap_or_default();
            let cfg = merge(base, over);
            if !include_cli && cfg.kind.is_some_and(ProviderKind::is_cli) {
                continue;
            }
            r.add(Connection::build(cfg)?)?;
        }
        Ok(r)
    }

    pub fn from_file(path: &Path, include_cli: bool) -> Result<Self> {
        Self::from_toml(&std::fs::read_to_string(path)?, include_cli)
    }

    /// Add a connection; a `free_tier` connection gets its guard from `ATLAS_FREE_*`
    /// (disabled if configuration is invalid; use [`Registry::add`] to see the error).
    pub fn insert(&mut self, c: Connection) {
        if self.add(c.clone()).is_err() {
            if c.config.free_tier == Some(true) {
                self.free_tiers.insert(
                    c.config.name.clone(),
                    FreeTier::new(FreeTierConfig {
                        disabled: true,
                        spend_file: None,
                        ..FreeTierConfig::default()
                    }),
                );
            }
            self.connections.insert(c.config.name.clone(), c);
        }
    }

    /// Add a connection; for `free_tier` connections, build the guard from env (errors surface).
    pub fn add(&mut self, c: Connection) -> Result<()> {
        if c.config.free_tier == Some(true) {
            if let Some(parent) = &c.config.shared_budget {
                let budget = self.free_tiers.get(parent).ok_or_else(|| {
                    LlmError::Config(format!(
                        "shared budget '{parent}' must be configured before '{}'",
                        c.name()
                    ))
                })?;
                let model = c
                    .config
                    .default_model
                    .clone()
                    .ok_or_else(|| LlmError::Config("shared hosted model must be fixed".into()))?;
                // Never use global ATLAS_FREE_PRICE_* overrides here: a zero-cost
                // primary such as Apodex must not underprice paid model fallbacks.
                let pinned_academic = c.name() == HOSTED_KISSKI
                    && c.kind == ProviderKind::OpenAiCompatible
                    && model == KISSKI_MODEL
                    && c.config.base_url.as_deref().map(|url| url.trim_end_matches('/')) == Some(KISSKI_BASE_URL);
                let prices = (if pinned_academic {
                    Some(Prices {
                        input: 0.0,
                        output: 0.0,
                        cache_read: 0.0,
                        cache_write: 0.0,
                    })
                } else {
                    Prices::for_model(&model)
                })
                .ok_or_else(|| LlmError::Config("shared hosted fallback has no known model-specific price".into()))?;
                let unchanged_credentials = self.connections.get(c.name()).is_some_and(|previous| {
                    previous.kind == c.kind
                        && previous.config.base_url == c.config.base_url
                        && previous.config.key_env == c.config.key_env
                        && previous.config.default_model == c.config.default_model
                });
                let server_key = if unchanged_credentials {
                    self.free_tiers.get(c.name()).and_then(|t| t.server_key().cloned())
                } else {
                    None
                };
                self.free_tiers
                    .insert(c.name().into(), budget.with_model(model, prices, server_key)?);
                self.connections.insert(c.config.name.clone(), c);
                return Ok(());
            }
            if self.free_tiers.contains_key(c.name()) {
                self.connections.insert(c.config.name.clone(), c);
                return Ok(());
            }
            let mut cfg = FreeTierConfig::from_env()?;
            if let Some(m) = c.config.default_model.as_ref().filter(|m| **m != cfg.model) {
                // The TOML model wins over ATLAS_FREE_MODEL; its price must be known or explicit.
                let explicit = std::env::var_os("ATLAS_FREE_PRICE_INPUT").is_some();
                match crate::free_tier::Prices::for_model(m) {
                    Some(p) if !explicit => cfg.prices = p,
                    Some(_) => {}
                    None if explicit => {}
                    None => {
                        return Err(LlmError::Config(format!(
                            "no built-in price for free-tier model '{m}'; set ATLAS_FREE_PRICE_INPUT and ATLAS_FREE_PRICE_OUTPUT"
                        )));
                    }
                }
                cfg.model.clone_from(m);
            }
            if matches!(c.name(), "gemini-free" | "gemini-lite-free" | "openai-hosted") {
                // Each hosted provider owns a separate ledger and writer lock.
                cfg.spend_file = cfg
                    .spend_file
                    .map(|p| p.with_file_name(format!("{}-spend.json", c.name())));
            }
            self.free_tiers.insert(c.config.name.clone(), FreeTier::new(cfg));
        }
        self.connections.insert(c.config.name.clone(), c);
        Ok(())
    }

    /// Install or replace the free-tier guard of a connection (tests, admin tools).
    pub fn set_free_tier(&mut self, name: &str, config: FreeTierConfig) -> Arc<FreeTier> {
        let t = if let Some(parent) = self.connections.get(name).and_then(|c| c.config.shared_budget.as_ref()) {
            match self.free_tiers.get(parent).and_then(|budget| {
                budget
                    .with_model(config.model.clone(), config.prices, config.server_key.clone())
                    .ok()
            }) {
                Some(t) => t,
                None => FreeTier::new(FreeTierConfig {
                    disabled: true,
                    spend_file: None,
                    ..config
                }),
            }
        } else {
            FreeTier::new(config)
        };
        self.free_tiers.insert(name.to_owned(), t.clone());
        // Replacing a primary test/admin guard must also rebind its dependents,
        // rather than leave them attached to an old ledger or stale kill switch.
        let children: Vec<_> = self
            .connections
            .values()
            .filter(|c| c.config.shared_budget.as_deref() == Some(name))
            .map(|c| c.config.name.clone())
            .collect();
        for child in children {
            if let Some(previous) = self.free_tiers.get(&child) {
                let cfg = previous.config();
                if let Ok(view) = t.with_model(cfg.model.clone(), cfg.prices, cfg.server_key.clone()) {
                    self.free_tiers.insert(child, view);
                }
            }
        }
        t
    }

    /// The free-tier guard of a connection, if it has one.
    pub fn free_tier(&self, name: &str) -> Option<&Arc<FreeTier>> {
        self.free_tiers.get(name)
    }

    pub fn get(&self, name: &str) -> Result<&Connection> {
        self.connections
            .get(name)
            .ok_or_else(|| LlmError::UnknownConnection(name.into()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Connection> {
        self.connections.values()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_task_uses_sol_and_shares_hosted_budget_without_changing_personal_models() {
        let r = super::Registry::presets(false).unwrap();
        let query = r.for_query_planning().unwrap();
        assert_eq!(
            query.get(super::HOSTED_FREE).unwrap().config.default_model.as_deref(),
            Some("openai/gpt-6.1-sol")
        );
        assert!(std::sync::Arc::ptr_eq(
            r.free_tier(super::HOSTED_FREE).unwrap(),
            query.free_tier(super::HOSTED_FREE).unwrap()
        ));
        assert_eq!(
            r.get("openrouter").unwrap().config.default_model,
            query.get("openrouter").unwrap().config.default_model
        );
        let routing = query
            .get(super::HOSTED_FREE)
            .unwrap()
            .config
            .provider_routing
            .as_ref()
            .unwrap();
        assert_eq!(routing["data_collection"], "deny");
        assert_eq!(routing["zdr"], true);
        assert_eq!(routing["max_price"]["completion"], 10.0);
    }
    use super::*;

    #[test]
    fn presets_and_overrides() {
        let r = Registry::from_toml(
            r#"
            [[connections]]
            name = "kisski"
            default_model = "llama-3.3-70b-instruct"

            [[connections]]
            name = "gpu"
            preset = "vllm"
            base_url = "http://gpu:8000/v1"
            "#,
            false,
        )
        .unwrap();
        let k = r.get("kisski").unwrap();
        assert_eq!(k.config.default_model.as_deref(), Some("llama-3.3-70b-instruct"));
        assert_eq!(k.config.base_url.as_deref(), Some(KISSKI_BASE_URL));
        assert!(k.info(None).demo_only);
        assert_eq!(r.get("gpu").unwrap().provider.base_url(), Some("http://gpu:8000/v1/"));
        assert!(r.get("claude-code").is_err(), "hosted mode has no CLIs");
        // Paid providers never fall back to a server env key unless configured.
        assert_eq!(
            r.get("openai").unwrap().key_policy,
            KeyPolicy::Required {
                env: Some("OPENAI_API_KEY".into()),
                env_fallback: false
            }
        );
        assert_eq!(r.get("ollama").unwrap().key_policy, KeyPolicy::None);
    }

    #[test]
    fn insert_does_not_enable_an_unpriced_hosted_model() {
        let mut config = preset(HOSTED_FREE).unwrap();
        config.default_model = Some("unpriced-security-test-model".into());
        let mut registry = Registry::default();
        registry.insert(Connection::build(config).unwrap());
        assert!(registry.free_tier(HOSTED_FREE).unwrap().is_disabled());
    }

    #[test]
    fn hosted_free_is_gpt_oss_on_openrouter_with_private_routing() {
        let r = Registry::presets(false).unwrap();
        let c = r.get(HOSTED_FREE).unwrap();
        assert_eq!(c.kind, ProviderKind::OpenRouter);
        assert_eq!(c.key_policy.env_var(), Some("OPENROUTER_API_KEY"));
        let model = c.config.default_model.as_deref().unwrap();
        assert!(!model.contains("claude"), "D48: Claude is no default");
        if model == DEFAULT_FREE_MODEL {
            let routing = c.config.provider_routing.as_ref().unwrap();
            assert_eq!(routing["data_collection"], "deny");
            assert_eq!(routing["zdr"], true);
            assert_eq!(routing["max_price"]["completion"], GPT_OSS_120B_MAX_PRICE.1);
            assert_eq!(
                c.info(None).model_label.unwrap().fallback,
                "gpt-oss-120b (OpenAI open-weight)"
            );
        }
        assert!(
            r.get("ollama")
                .unwrap()
                .config
                .default_model
                .as_deref()
                .unwrap()
                .starts_with("gpt-oss")
        );
        // Routing preferences are an OpenRouter option only.
        assert!(
            Registry::from_toml(
                "[[connections]]\nname='k'\npreset='kisski'\nprovider_routing={zdr=true}\n",
                false
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(Registry::from_toml("[[connections]]\nname='x'\napi_key='nope'\n", false).is_err());
    }

    #[test]
    fn opencode_is_local_free_and_has_no_server_key() {
        let local = Registry::presets(true).unwrap();
        let c = local.get("opencode").unwrap();
        assert_eq!(c.kind, ProviderKind::OpenCodeCli);
        assert!(c.kind.is_cli());
        assert_eq!(
            c.config.default_model.as_deref(),
            Some(crate::cli::opencode::DEFAULT_MODEL)
        );
        assert_eq!(c.key_policy, KeyPolicy::None);
        assert_eq!(c.info(None).runs_on, "user-machine");
        assert!(Registry::presets(false).unwrap().get("opencode").is_err());
    }
}
