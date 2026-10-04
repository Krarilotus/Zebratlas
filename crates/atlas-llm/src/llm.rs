//! `Llm`: the façade analytics/server call. Registry + cache + provenance + guarded JSON.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{Cache, CacheEntry, CacheMode, DEFAULT_DIR, cache_key, prompt_hash};
use crate::error::{LlmError, Result, truncate};
use crate::free_tier::{Admission, HOSTED_FREE, QuotaReason};
use crate::provenance::{CallFacts, LlmCall, now_rfc3339};
use crate::provider::{Availability, KeyPolicy};
use crate::registry::{Connection, ConnectionInfo, Registry};
use crate::request::{CompletionRequest, CompletionResponse, Message, ProviderOutput};
use crate::secret::ApiKey;

/// Who/what a call is for: connection name, optional BYO key, provenance inputs.
#[derive(Clone, Debug, Default)]
pub struct Call {
    pub connection: String,
    /// Per-request key from the UI. Used for this call only; never stored, cached or logged.
    pub key: Option<ApiKey>,
    /// Ids of what the prompt was built from (`PMID:123`, `ORPHA:558`, ...): `prov:used`.
    pub inputs: Vec<String>,
    /// Pseudonymous visitor id for the free tier's per-visitor limits: a session id or
    /// [`crate::free_tier::visitor_key`] of the IP. Kept in memory only; never in provenance.
    pub visitor: Option<String>,
    /// User-authored prompts and replies must never enter the shared disk cache.
    pub private: bool,
    /// Optional total budget for a guarded text task, including its validation retry.
    pub deadline: Option<Duration>,
    /// D48: when this connection fails for an infrastructure reason (not configured, down,
    /// timeout, upstream 5xx/404/429, server key rejected), try the next connection of the
    /// default chain ([`Llm::default_chain`]). Hosted connections participate automatically;
    /// this flag also enables custom fallback connections. Personal keys and connectors stay fixed.
    pub fallback: bool,
}

impl Call {
    pub fn new(connection: impl Into<String>) -> Self {
        Self {
            connection: connection.into(),
            ..Self::default()
        }
    }
    pub fn with_private(mut self) -> Self {
        self.private = true;
        self
    }
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
    pub fn with_key(mut self, key: ApiKey) -> Self {
        self.key = Some(key);
        self
    }
    pub fn with_inputs<I: IntoIterator<Item = S>, S: Into<String>>(mut self, inputs: I) -> Self {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
    pub fn with_visitor(mut self, visitor: impl Into<String>) -> Self {
        self.visitor = Some(visitor.into());
        self
    }
    /// Follow the default chain on infrastructure failures (see [`Call::fallback`]).
    pub fn with_fallback(mut self, on: bool) -> Self {
        self.fallback = on;
        self
    }
}

/// A finished call with its PROV-O record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub response: CompletionResponse,
    pub provenance: LlmCall,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnectionCheck {
    pub connection: String,
    pub connected: bool,
    pub models: Vec<String>,
    pub availability: Availability,
}

/// A schema-validated, typed result; `calls` has one entry, or two after the retry.
#[derive(Clone, Debug)]
pub struct JsonCompletion<T> {
    pub value: T,
    pub json: Value,
    pub calls: Vec<Completion>,
}

#[derive(Debug, Clone)]
pub struct Llm {
    registry: Registry,
    cache: Cache,
    /// Connections tried after `hosted-free`, in order (D48). Built-in hosted alternatives for [`Llm::new`];
    /// [`Llm::from_env`] reads [`fallbacks_from_env`].
    fallbacks: Vec<String>,
}

/// D48 fallback order after the hosted free tier: `ATLAS_FREE_FALLBACKS` (comma list; empty =
/// none), else `kisski` (demo only), then `ollama` (local `gpt-oss:20b`) when `ATLAS_OLLAMA_URL`
/// is set. Connections without a usable key are skipped at call time.
pub fn fallbacks_from_env() -> Vec<String> {
    if let Ok(list) = std::env::var("ATLAS_FREE_FALLBACKS") {
        return list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    let mut v = vec!["kisski".to_owned()];
    if std::env::var("ATLAS_OLLAMA_URL").is_ok_and(|u| !u.trim().is_empty()) {
        v.push("ollama".into());
    }
    v
}

impl Llm {
    /// Replace only the request's registry, retaining cache policy and the D48 fallback chain.
    pub fn with_registry(&self, registry: Registry) -> Self {
        Self {
            registry,
            ..self.clone()
        }
    }

    pub fn new(registry: Registry, cache: Cache) -> Self {
        Self {
            registry,
            cache,
            fallbacks: crate::routing::DEFAULT_ORDER
                .iter()
                .skip(1)
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }

    /// Replace the fallback order after `hosted-free` (D48).
    pub fn with_fallbacks<I: IntoIterator<Item = S>, S: Into<String>>(mut self, names: I) -> Self {
        self.fallbacks = names.into_iter().map(Into::into).collect();
        self
    }

    /// From env:
    /// - `ATLAS_LLM_CONFIG`: TOML file (else built-in presets);
    /// - `ATLAS_LLM_CLI=0`: hosted mode, no subscription CLIs on this machine;
    /// - `ATLAS_LLM_CACHE_DIR` (default `data/cache/llm`), `ATLAS_LLM_CACHE` = `read-write` |
    ///   `replay-only` | `off`.
    pub fn from_env() -> Result<Self> {
        let include_cli = !matches!(std::env::var("ATLAS_LLM_CLI").as_deref(), Ok("0" | "off" | "false"));
        let registry = match std::env::var_os("ATLAS_LLM_CONFIG") {
            Some(p) => Registry::from_file(&PathBuf::from(p), include_cli)?,
            None => Registry::presets(include_cli)?,
        };
        let dir = std::env::var_os("ATLAS_LLM_CACHE_DIR").map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from);
        let mode = match std::env::var("ATLAS_LLM_CACHE") {
            Ok(s) => CacheMode::parse(&s).ok_or_else(|| LlmError::Config(format!("ATLAS_LLM_CACHE={s}")))?,
            Err(_) => CacheMode::ReadWrite,
        };
        let llm = Self::new(registry, Cache::new(dir, mode)).with_fallbacks(fallbacks_from_env());
        llm.log_free_tier_setup();
        Ok(llm)
    }

    /// One line at start-up when the hosted free tier has no server key (never the key itself).
    fn log_free_tier_setup(&self) {
        if let Ok(conn) = self.registry.get(HOSTED_FREE)
            && self.registry.free_tier(HOSTED_FREE).is_some()
            && self.free_key(conn).is_none()
        {
            let var = conn.key_policy.env_var().unwrap_or("its key variable");
            let next = self.default_chain().into_iter().next();
            eprintln!(
                "atlas-llm: {HOSTED_FREE} has no server key ({var} not set); default connection: {}",
                next.as_deref().unwrap_or("none (bring your own key)")
            );
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// All connections; with `probe`, each is probed concurrently (no model calls).
    pub async fn list_connections(&self, probe: bool) -> Vec<ConnectionInfo> {
        self.list_connections_for(probe, None).await
    }

    /// As [`Llm::list_connections`], plus the free tier's remaining calls for `visitor`.
    pub async fn list_connections_for(&self, probe: bool, visitor: Option<&str>) -> Vec<ConnectionInfo> {
        let mut found: Vec<Option<Availability>> = vec![None; self.registry.iter().count()];
        if probe {
            let mut set = tokio::task::JoinSet::new();
            for (i, c) in self.registry.iter().enumerate() {
                let p = c.provider.clone();
                set.spawn(async move { (i, p.probe().await) });
            }
            while let Some(Ok((i, a))) = set.join_next().await {
                found[i] = Some(a);
            }
        }
        self.registry
            .iter()
            .zip(found)
            .map(|(c, a)| {
                let mut info = c.info(a);
                if let Some(t) = self.registry.free_tier(c.name()) {
                    let status = t.status(visitor, self.free_key(c).is_some());
                    info.needs_key = false;
                    info.key_from_env_available = status.blocked != Some(QuotaReason::NotConfigured);
                    if let Some(a) = info.availability.as_mut()
                        && (!matches!(
                            c.kind,
                            crate::provider::ProviderKind::Gemini | crate::provider::ProviderKind::OpenAi
                        ) || status.blocked.is_some())
                    {
                        a.available = status.blocked.is_none();
                        a.reason = status.blocked.map_or_else(|| "ready".into(), reason_code);
                    }
                    info.free_tier = Some(status);
                }
                info
            })
            .collect()
    }

    pub async fn probe(&self, connection: &str) -> Result<Availability> {
        let conn = self.registry.get(connection)?;
        let mut a = conn.provider.probe().await;
        if let Some(t) = self.registry.free_tier(connection) {
            let blocked = t.status(None, self.free_key(conn).is_some()).blocked;
            if !matches!(
                conn.kind,
                crate::provider::ProviderKind::Gemini | crate::provider::ProviderKind::OpenAi
            ) || blocked.is_some()
            {
                a.available = blocked.is_none();
                a.reason = blocked.map_or_else(|| "ready".into(), reason_code);
            }
        }
        Ok(a)
    }

    /// Validate credentials without inference, then intersect with configured model IDs.
    pub async fn check_connection(&self, name: &str) -> Result<ConnectionCheck> {
        let conn = self.registry.get(name)?;
        let availability = self.probe(name).await?;
        let allowed = conn.config.default_model.iter().chain(conn.config.models.iter());
        let models = allowed
            .filter(|model| {
                availability.available && (availability.models.is_empty() || availability.models.contains(model))
            })
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(ConnectionCheck {
            connection: name.into(),
            connected: availability.available,
            models,
            availability,
        })
    }

    /// The server key of a free-tier connection (from code or its env var).
    fn free_key(&self, conn: &Connection) -> Option<ApiKey> {
        let tier = self.registry.free_tier(conn.name())?;
        tier.server_key().cloned().or_else(|| conn.key_policy.resolve(None))
    }

    /// Connection for a request that names none: `ATLAS_LLM_DEFAULT`; else, without an own key,
    /// the first usable connection of [`Llm::default_chain`] (D48: `hosted-free` = OpenRouter
    /// gpt-oss-120b, then KISSKI, then local Ollama); else `kisski`.
    pub fn default_connection(&self, has_own_key: bool) -> String {
        if let Ok(name) = std::env::var("ATLAS_LLM_DEFAULT")
            && !name.trim().is_empty()
        {
            return name.trim().to_owned();
        }
        if !has_own_key && let Some(first) = self.default_chain().into_iter().next() {
            return first;
        }
        "kisski".into()
    }

    /// The usable no-key connections in fallback order (D48): `hosted-free` when it has a
    /// server key and is switched on, then the configured fallbacks that have what they need
    /// (a server key where one is required). Never contains a connection that would spend a
    /// paid provider's server key outside the free-tier guard.
    pub fn default_chain(&self) -> Vec<String> {
        let mut chain = Vec::new();
        if let Ok(conn) = self.registry.get(HOSTED_FREE)
            && let Some(t) = self.registry.free_tier(HOSTED_FREE)
            && !t.is_disabled()
            && self.free_key(conn).is_some()
        {
            chain.push(HOSTED_FREE.to_owned());
        }
        for name in &self.fallbacks {
            let Ok(conn) = self.registry.get(name) else { continue };
            let usable = if let Some(tier) = self.registry.free_tier(name) {
                !tier.is_disabled() && self.free_key(conn).is_some()
            } else {
                match &conn.key_policy {
                    KeyPolicy::Required { env_fallback: true, .. } => conn.key_policy.resolve(None).is_some(),
                    KeyPolicy::Required { .. } => false,
                    KeyPolicy::Optional { .. } | KeyPolicy::None => !conn.kind.is_cli(),
                }
            };
            if usable && !chain.contains(name) {
                chain.push(name.clone());
            }
        }
        chain
    }

    /// D48 stable entry point: structured output on the default no-key connection (hosted free
    /// tier, falling back along [`Llm::default_chain`]), schema-validated with one guarded retry.
    /// Used by document intake and other server-side tasks that have no user connection.
    /// `visitor`: pseudonymous id for the free tier's per-visitor limits ([`crate::visitor_key`]);
    /// `inputs`: ids the prompt was built from (`prov:used`). Each returned call carries its
    /// connection, model and `model_label` (UI) plus a PROV-O record.
    pub async fn complete_default_json<T: DeserializeOwned>(
        &self,
        visitor: Option<&str>,
        inputs: &[String],
        req: CompletionRequest,
    ) -> Result<JsonCompletion<T>> {
        let mut call = Call::new(self.default_connection(false))
            .with_inputs(inputs.iter().cloned())
            .with_fallback(true);
        if let Some(v) = visitor {
            call = call.with_visitor(v);
        }
        self.complete_json(&call, req).await
    }

    /// One call: cache lookup → provider → cache store → PROV-O record.
    /// With a schema, `response.json` holds the parsed value if it validates (else `None`).
    /// With [`Call::fallback`] (and no own key), infrastructure failures move on along
    /// [`Llm::default_chain`]; the PROV-O record then has `fallback_from` and `fallback_reason`.
    pub async fn complete(&self, call: &Call, mut req: CompletionRequest) -> Result<Completion> {
        if let Some(deadline) = call.deadline.map(|budget| budget.min(req.deadline)) {
            req.deadline = deadline;
            tokio::time::timeout(deadline, self.complete_with_fallback(call, req))
                .await
                .unwrap_or(Err(LlmError::Timeout(deadline)))
        } else {
            self.complete_with_fallback(call, req).await
        }
    }

    /// Prefer the lighter guarded provider for small text tasks.
    pub fn for_easy_task(&self, call: &Call) -> Call {
        let mut routed = call.clone();
        if crate::routing::is_hosted(&call.connection)
            && (matches!(call.connection.as_str(), crate::routing::GEMINI | crate::routing::GEMINI_LITE)
                || self.fallbacks.iter().any(|name| name == crate::routing::GEMINI_LITE))
            && call.key.is_none()
            && self
                .registry
                .get(crate::routing::GEMINI_LITE)
                .is_ok_and(|c| self.free_key(c).is_some())
        {
            routed.connection = crate::routing::GEMINI_LITE.into();
        }
        routed
    }

    async fn complete_with_fallback(&self, call: &Call, req: CompletionRequest) -> Result<Completion> {
        let model_override = req.model.as_ref().is_some_and(|model| {
            !self
                .registry
                .get(&call.connection)
                .is_ok_and(|connection| connection.config.default_model.as_ref() == Some(model))
        });
        if call.key.is_some()
            || model_override
            || !(call.connection == HOSTED_FREE || call.fallback)
            || self
                .registry
                .get(&call.connection)
                .is_ok_and(|c| c.kind == crate::ProviderKind::Connector)
            || call.connection.starts_with("connector:")
        {
            return self.complete_once(call, req).await;
        }
        let mut candidates = vec![call.connection.clone()];
        for name in self.default_chain() {
            if !candidates.contains(&name) {
                candidates.push(name);
            }
        }
        let started = Instant::now();
        let mut failed = Vec::new();
        let mut last = LlmError::Unavailable("no usable hosted provider".into());
        for (index, name) in candidates.iter().enumerate() {
            let remaining = req.deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(LlmError::Timeout(req.deadline));
            }
            let mut attempt = req.clone();
            if index > 0 {
                attempt.model = None;
            }
            attempt.deadline = remaining / (candidates.len() - index) as u32;
            let next = Call {
                connection: name.clone(),
                key: None,
                fallback: false,
                ..call.clone()
            };
            match self.complete_once(&next, attempt).await {
                Ok(mut done) => {
                    if !failed.is_empty() {
                        let p = &mut done.provenance.activity.parameters;
                        p.insert("fallback.from".into(), call.connection.clone());
                        p.insert("fallback.failed_connections".into(), failed.join(","));
                        p.insert("fallback_from".into(), call.connection.clone());
                        p.insert("fallback_reason".into(), error_code(&last).into());
                    }
                    return Ok(done);
                }
                Err(error) if crate::routing::can_fallback(&error) => {
                    failed.push(name.clone());
                    last = error;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }

    async fn complete_once(&self, call: &Call, mut req: CompletionRequest) -> Result<Completion> {
        let conn = self.registry.get(&call.connection)?;
        // Never persist or share replies from a personal machine, even if a caller forgets
        // with_private(). A connector's transport is bound to its authenticated account.
        let mut connector_call;
        let call = if conn.kind == crate::ProviderKind::Connector {
            if call.key.is_some() {
                return Err(LlmError::InvalidRequest(
                    "connector keys stay on the user's machine".into(),
                ));
            }
            connector_call = call.clone();
            connector_call.private = true;
            &connector_call
        } else {
            call
        };
        let tier = self.registry.free_tier(&call.connection).cloned();
        let mut task_prices = None;
        if let Some(t) = &tier {
            let fixed_model = conn.config.default_model.as_deref().unwrap_or(&t.config().model);
            if req.model.as_deref().is_some_and(|m| m != fixed_model) {
                return Err(LlmError::InvalidRequest(
                    "the hosted model is fixed by the server".into(),
                ));
            }
            req.model = Some(fixed_model.to_owned());
            task_prices = Some(if fixed_model == t.config().model {
                t.config().prices
            } else {
                crate::free_tier::Prices::for_model(fixed_model)
                    .ok_or_else(|| LlmError::Config("server query model requires verified prices".into()))?
            });
            if call.key.is_some() {
                return Err(LlmError::InvalidRequest(format!(
                    "'{}' runs on the project's key; to use your own key, choose your provider's connection",
                    conn.name()
                )));
            }
            let cap = t.config().max_tokens;
            req.settings.max_tokens = Some(req.settings.max_tokens.map_or(cap, |n| n.min(cap)));
            if input_chars(&req) > t.config().max_input_chars {
                return Err(LlmError::FreeQuota {
                    reason: QuotaReason::TooLarge,
                    retry_after_secs: None,
                });
            }
        }
        let model = req
            .model
            .clone()
            .or_else(|| conn.config.default_model.clone())
            .ok_or_else(|| LlmError::InvalidRequest(format!("connection '{}' needs a model", conn.name())))?;
        if conn.kind.is_cli() && call.key.is_some() {
            return Err(LlmError::InvalidRequest(
                "subscription CLIs use their own login; do not send a key".into(),
            ));
        }
        let base_url = conn.provider.base_url().map(str::to_owned);
        let key = cache_key(conn.kind, base_url.as_deref(), &model, &req);

        let (output, latency_ms, started, ended, hit) =
            if let Some(e) = if call.private { None } else { self.cache.get(&key)? } {
                (e.output, e.latency_ms, e.started_at, e.ended_at, true)
            } else {
                if self.cache.mode() == CacheMode::ReplayOnly {
                    return Err(LlmError::CacheMiss(key));
                }
                let (api_key, admission) = match &tier {
                    Some(t) => {
                        let k = self.free_key(conn).ok_or(LlmError::FreeQuota {
                            reason: QuotaReason::NotConfigured,
                            retry_after_secs: None,
                        })?;
                        let input_bytes = serde_json::to_vec(&req)
                            .map_err(|_| LlmError::InvalidRequest("invalid request".into()))?
                            .len();
                        let p = task_prices.expect("hosted prices checked");
                        let estimate = ((input_bytes as f64 + 4096.0) * p.input.max(p.cache_read).max(p.cache_write)
                            + f64::from(req.settings.max_tokens.unwrap_or(t.config().max_tokens)) * p.output)
                            / 1e6;
                        let visitor = call.visitor.as_deref().unwrap_or("anonymous");
                        (Some(k), Some(t.admit(visitor, estimate)?))
                    }
                    None => (resolve_key(conn, call.key.as_ref())?, None),
                };
                let started = now_rfc3339();
                let t0 = Instant::now();
                let result = tokio::time::timeout(req.deadline, conn.provider.complete(&req, &model, api_key.as_ref()))
                    .await
                    .unwrap_or(Err(LlmError::Timeout(req.deadline)));
                let output = settle(admission, task_prices, result)?;
                let latency_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
                let ended = now_rfc3339();
                if !call.private {
                    self.cache.put(&CacheEntry {
                        format: 1,
                        key: key.clone(),
                        provider: conn.kind,
                        base_url: base_url.clone(),
                        model: model.clone(),
                        request: req.clone(),
                        output: output.clone(),
                        latency_ms,
                        started_at: started.clone(),
                        ended_at: ended.clone(),
                    })?;
                }
                (output, latency_ms, started, ended, false)
            };
        let cost = tier.as_ref().map(|_| {
            if hit {
                0.0
            } else {
                task_prices.and_then(|p| p.cost(&output.usage)).unwrap_or(0.0)
            }
        });
        let mut done = self.finish(
            conn, call, &req, model, key, output, latency_ms, &started, &ended, hit, cost,
        );
        if let Some(t) = &tier {
            // The day's running spend against the cap, so the PROV log doubles as a spend ledger.
            let p = &mut done.provenance.activity.parameters;
            p.insert("free_tier.day".into(), crate::free_tier::utc_date());
            p.insert("free_tier.spent_usd_today".into(), format!("{:.6}", t.spent_today()));
            p.insert("free_tier.daily_cap_usd".into(), format!("{:.2}", t.config().daily_usd));
        }
        Ok(done)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        conn: &Connection,
        call: &Call,
        req: &CompletionRequest,
        model: String,
        key: String,
        output: ProviderOutput,
        latency_ms: u64,
        started: &str,
        ended: &str,
        hit: bool,
        cost_usd: Option<f64>,
    ) -> Completion {
        let json = req
            .schema
            .as_ref()
            .and_then(|s| check_json(&s.schema, &output.text).ok());
        let prompt_bytes = req.messages.iter().map(|m| m.content.len() as u64).sum();
        let mut provenance = LlmCall::new(CallFacts {
            connection: conn.name(),
            provider: conn.kind.as_str(),
            base_url: conn.provider.base_url(),
            requested_model: &model,
            reported_model: output.reported_model.as_deref(),
            agent_version: output.agent_version.as_deref(),
            cache_key: &key,
            cache_file: self
                .cache
                .dir()
                .join(format!("{key}.json"))
                .display()
                .to_string()
                .replace('\\', "/"),
            prompt_hash: prompt_hash(req),
            prompt_bytes,
            response_text: &output.text,
            sent: &output.sent,
            usage: &output.usage,
            latency_ms,
            started_at: started,
            ended_at: ended,
            cache_hit: hit,
            inputs: &call.inputs,
        });
        if call.private {
            provenance.prompt.file.clear();
            provenance.response.file.clear();
            provenance
                .activity
                .parameters
                .insert("retention".into(), "private-no-cache".into());
        }
        if conn.kind == crate::ProviderKind::Connector {
            provenance.prompt.url = format!("urn:sha256:{}", provenance.prompt.sha256.as_deref().unwrap_or_default());
            provenance.prompt.version = Some("atlas-connector-v1".into());
            provenance.response.url = format!(
                "urn:sha256:{}",
                provenance.response.sha256.as_deref().unwrap_or_default()
            );
            provenance
                .response
                .version
                .get_or_insert_with(|| "model-not-reported".into());
            provenance.activity.parameters.insert(
                "record_locator.prompt".into(),
                "canonical-json:{messages,schema}".into(),
            );
            provenance
                .activity
                .parameters
                .insert("record_locator.response".into(), "$.response.text".into());
        }
        if let Some(c) = cost_usd {
            let p = &mut provenance.activity.parameters;
            p.insert("free_tier".into(), "true".into());
            p.insert("cost_usd".into(), format!("{c:.6}"));
        }
        let model_label = crate::model_label::model_label(&model);
        let response = CompletionResponse {
            connection: conn.name().to_owned(),
            requested_model: model,
            reported_model: output.reported_model,
            text: output.text,
            json,
            usage: output.usage,
            stop_reason: output.stop_reason,
            latency_ms,
            cached: hit,
            cache_key: key,
            cost_usd,
            model_label,
        };
        Completion { response, provenance }
    }

    /// Structured output with a guard: parse + validate against `req.schema`; on failure retry
    /// once with the validation error; then `SchemaValidation` (the caller falls back to a
    /// deterministic template).
    pub async fn complete_json<T: DeserializeOwned>(
        &self,
        call: &Call,
        req: CompletionRequest,
    ) -> Result<JsonCompletion<T>> {
        let schema = req
            .schema
            .as_ref()
            .ok_or_else(|| LlmError::InvalidRequest("complete_json needs a schema".into()))?
            .schema
            .clone();
        jsonschema::validator_for(&schema).map_err(|e| LlmError::InvalidSchema(e.to_string()))?;
        let first = self.complete(call, req.clone()).await?;
        let err = match typed::<T>(&schema, &first.response.text) {
            Ok((value, json)) => {
                return Ok(JsonCompletion {
                    value,
                    json,
                    calls: vec![first],
                });
            }
            Err(e) => e,
        };
        let mut retry = req;
        retry.messages.push(Message::assistant(first.response.text.clone()));
        retry.messages.push(Message::user(format!(
            "Your reply did not validate against the JSON schema: {err}\n\
             Reply again with only the corrected JSON value, nothing else."
        )));
        let second = self.complete(call, retry).await?;
        match typed::<T>(&schema, &second.response.text) {
            Ok((value, json)) => Ok(JsonCompletion {
                value,
                json,
                calls: vec![first, second],
            }),
            Err(e) => Err(LlmError::SchemaValidation(e)),
        }
    }
}

/// Short, non-secret code of an error (logs and PROV parameters).
fn error_code(e: &LlmError) -> &'static str {
    match e {
        LlmError::FreeQuota {
            reason: QuotaReason::NotConfigured,
            ..
        } => "not_configured",
        LlmError::FreeQuota { .. } => "free_quota",
        LlmError::Unavailable(_) => "unavailable",
        LlmError::Timeout(_) => "timeout",
        LlmError::RateLimited(_) => "rate_limited",
        LlmError::Auth(_) => "auth",
        LlmError::MissingKey { .. } => "missing_key",
        LlmError::UnknownConnection(_) => "unknown_connection",
        LlmError::Provider { .. } => "provider_error",
        _ => "error",
    }
}

/// `disabled`, `daily_budget`, ... for `Availability::reason`.
fn reason_code(reason: QuotaReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn input_chars(req: &CompletionRequest) -> usize {
    req.messages.iter().map(|m| m.content.chars().count()).sum::<usize>()
        + req.schema.as_ref().map_or(0, |s| s.schema.to_string().len())
}

/// Book the cost of a free-tier call: the reported usage; zero for errors the provider rejected
/// before inference (4xx, auth, rate limit); the reserved worst case when the outcome is unknown.
fn settle(
    admission: Option<Admission>,
    prices: Option<crate::free_tier::Prices>,
    result: Result<ProviderOutput>,
) -> Result<ProviderOutput> {
    let (Some(a), Some(p)) = (admission, prices) else {
        return result;
    };
    match &result {
        Ok(out) => a.settle(p.cost(&out.usage)),
        Err(
            LlmError::Auth(_) | LlmError::RateLimited(_) | LlmError::MissingKey { .. } | LlmError::InvalidRequest(_),
        ) => {
            a.settle(Some(0.0));
        }
        Err(LlmError::Provider { status: Some(s), .. }) if (400..500).contains(s) => a.settle(Some(0.0)),
        Err(_) => a.settle(None),
    }
    result
}

fn resolve_key(conn: &Connection, per_request: Option<&ApiKey>) -> Result<Option<ApiKey>> {
    let key = conn.key_policy.resolve(per_request);
    if key.is_none()
        && let KeyPolicy::Required { env, env_fallback } = &conn.key_policy
    {
        let hint = match (env, env_fallback) {
            (Some(var), true) => format!("send a key with the request or set {var}"),
            _ => "send a key with the request".into(),
        };
        return Err(LlmError::MissingKey {
            connection: conn.name().into(),
            hint,
        });
    }
    Ok(key)
}

/// Extract a JSON value from model text: plain, fenced (```json), or the outermost {...}/[...].
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Some(v);
    }
    let unfenced = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .and_then(|s| s.trim_end().strip_suffix("```"))
        .map(str::trim);
    if let Some(Ok(v)) = unfenced.map(serde_json::from_str) {
        return Some(v);
    }
    let start = t.find(['{', '['])?;
    let close = if t.as_bytes()[start] == b'{' { '}' } else { ']' };
    let end = t.rfind(close)?;
    serde_json::from_str(&t[start..=end]).ok()
}

/// Parse and validate; the error lists up to five schema violations.
pub fn check_json(schema: &Value, text: &str) -> std::result::Result<Value, String> {
    let v = extract_json(text).ok_or_else(|| format!("reply is not JSON: {}", truncate(text, 200)))?;
    let validator = jsonschema::validator_for(schema).map_err(|e| e.to_string())?;
    let errors: Vec<String> = validator
        .iter_errors(&v)
        .take(5)
        .map(|e| format!("at '{}': {}", e.instance_path(), e))
        .collect();
    if errors.is_empty() {
        Ok(v)
    } else {
        Err(errors.join("; "))
    }
}

fn typed<T: DeserializeOwned>(schema: &Value, text: &str) -> std::result::Result<(T, Value), String> {
    let v = check_json(schema, text)?;
    let t = serde_json::from_value(v.clone()).map_err(|e| format!("does not match the expected type: {e}"))?;
    Ok((t, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_fenced_and_embedded_json() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), Some(json!({"a":1})));
        assert_eq!(
            extract_json("Sure! {\"a\": [1,2]} hope that helps"),
            Some(json!({"a":[1,2]}))
        );
        assert_eq!(extract_json("no json"), None);
    }

    #[test]
    fn validates_against_schema() {
        let s = json!({"type":"object","required":["n"],"properties":{"n":{"type":"integer"}}});
        assert!(check_json(&s, "{\"n\": 3}").is_ok());
        let e = check_json(&s, "{\"n\": \"x\"}").unwrap_err();
        assert!(e.contains("/n"), "{e}");
    }
}
