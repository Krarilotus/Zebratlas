//! Hosted free tier (D23, D48): a project server key behind per-visitor and global limits, a
//! daily spend cap and a kill switch. Provider-agnostic: the `hosted-free` connection decides the
//! provider (default: OpenRouter `openai/gpt-oss-120b`, zero-data-retention endpoints only).
//!
//! - Per visitor (session id or [`visitor_key`] of the IP): calls per hour and per day.
//! - Global: concurrent calls and calls per minute.
//! - Daily spend cap in USD (UTC day): cost = the provider's reported charge (OpenRouter
//!   `usage.cost`) when present, else reported usage × [`Prices`]. Before a call the
//!   worst case (input estimate + `max_tokens` output) is *reserved*, so parallel calls cannot
//!   overshoot; afterwards the reservation is replaced by the actual cost. A call whose outcome is
//!   unknown (timeout, connection drop) keeps the reserved amount. The day's spend is persisted.
//! - Kill switch: `ATLAS_FREE_DISABLED=1` (read at every call) or [`FreeTier::set_disabled`].
//! - Cache hits are free and do not count.
//!
//! Every limit is a typed [`LlmError::FreeQuota`] the UI can show ("free quota reached, bring
//! your own key or try later").

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::cache::sha256_hex;
use crate::error::{LlmError, Result};
use crate::request::Usage;
use crate::secret::ApiKey;

/// Name of the hosted free connection.
pub const HOSTED_FREE: &str = "hosted-free";
pub const HOSTED_ANTHROPIC: &str = "hosted-anthropic";
pub const HOSTED_GEMINI: &str = "hosted-gemini";
pub const HOSTED_KISSKI: &str = "hosted-kisski";
/// Default model (D48: OpenAI open-weight via OpenRouter); `ATLAS_FREE_MODEL` overrides it.
pub const DEFAULT_FREE_MODEL: &str = "openai/gpt-oss-120b";
/// Where the gpt-oss prices come from.
pub const OPENROUTER_PRICES_SOURCE: &str = "OpenRouter endpoint list for openai/gpt-oss-120b \
     (GET https://openrouter.ai/api/v1/models/openai/gpt-oss-120b/endpoints and /api/v1/endpoints/zdr, \
     read 2026-10-03 23:20 UTC): zero-data-retention endpoints charge $0.03-$0.35 input / $0.17-$0.95 \
     output per MTok (most $0.03-$0.15 / $0.17-$0.60); the table uses the most expensive ZDR endpoint as \
     the worst case and the request caps routing at that price (provider.max_price); the spend uses \
     OpenRouter's reported usage.cost per response when present";
/// Where the Anthropic prices come from (a `hosted-free` configured on Anthropic in TOML, D23).
pub const ANTHROPIC_PRICES_SOURCE: &str = "Anthropic model table (claude-api reference, cached 2026-09-25, checked 2026-10-03): \
     claude-sonnet-5-5 $2.00 input / $10.00 output / $0.20 cache read per MTok; cache write (5 min) = 1.25x input";
pub const GEMINI_PRICES_SOURCE: &str = "Google Gemini Developer API pricing \
    (https://ai.google.dev/gemini-api/docs/pricing, checked 2026-10-04): gemini-2.5-flash \
    $0.30 text input / $2.50 output including thinking per MTok; implicit cache reads \
    conservatively charged at the full input rate; no explicit cache storage requested";
/// Worst-case gpt-oss-120b price on OpenRouter's ZDR endpoints (USD per MTok: input, output), see
/// [`OPENROUTER_PRICES_SOURCE`]; also sent as `provider.max_price` so routing never exceeds it.
pub const GPT_OSS_120B_MAX_PRICE: (f64, f64) = (0.35, 0.95);

/// Source note for a model's built-in price.
pub fn prices_source(model: &str) -> &'static str {
    if model == "openai/gpt-6.1-sol" {
        "OpenRouter https://openrouter.ai/openai/gpt-6.1-sol (checked 2026-10-04): $2 input / $10 output per MTok; routing is capped at those prices"
    } else if model.contains("gpt-oss") {
        OPENROUTER_PRICES_SOURCE
    } else if model.starts_with("claude") {
        ANTHROPIC_PRICES_SOURCE
    } else if model == "gemini-2.5-flash" {
        GEMINI_PRICES_SOURCE
    } else if model.starts_with("gemini-") {
        "Google Gemini https://ai.google.dev/gemini-api/docs/pricing"
    } else if model == "gpt-5.4-mini" {
        "OpenAI https://developers.openai.com/api/docs/models/gpt-5.4-mini"
    } else {
        "set by ATLAS_FREE_PRICE_* (USD per MTok)"
    }
}

/// USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Prices {
    const fn new(input: f64, output: f64, cache_read: f64) -> Self {
        Self {
            input,
            output,
            cache_read,
            cache_write: input * 1.25,
        }
    }

    /// Built-in prices for the models the free tier may run (see [`prices_source`]): gpt-oss on
    /// OpenRouter (worst-case ZDR endpoint) and, for a TOML-configured Anthropic free tier,
    /// Anthropic's first-party table.
    pub fn for_model(model: &str) -> Option<Self> {
        Some(match model {
            // OpenRouter model/endpoint catalog checked 2026-10-04; routing caps match.
            "openai/gpt-6.1-sol" => Self {
                input: 2.0,
                output: 10.0,
                cache_read: 2.0,
                cache_write: 2.0,
            },
            "openai/gpt-oss-120b" => Self {
                input: GPT_OSS_120B_MAX_PRICE.0,
                output: GPT_OSS_120B_MAX_PRICE.1,
                // Cached prompt reads are cheaper on some endpoints; the worst case bills them in full.
                cache_read: GPT_OSS_120B_MAX_PRICE.0,
                cache_write: GPT_OSS_120B_MAX_PRICE.0,
            },
            "claude-sonnet-5-5" | "claude-sonnet-5" => Self::new(2.0, 10.0, 0.20),
            "claude-opus-5-5" => Self::new(4.0, 20.0, 0.20),
            "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => Self::new(5.0, 25.0, 0.50),
            "claude-sonnet-4-6" => Self::new(3.0, 15.0, 0.30),
            "claude-haiku-4-5" => Self::new(1.0, 5.0, 0.10),
            // Conservative paid-tier text prices even when Google's project is free.
            // Google Gemini pricing, checked 2026-10-04; no cache writes are sent.
            "gemini-2.5-flash" => Self {
                input: 0.30,
                output: 2.50,
                cache_read: 0.30,
                cache_write: 0.30,
            },
            // Use the published post-promotion prices as a conservative reservation.
            "gemini-3.8-flash" => Self {
                input: 1.50,
                output: 7.50,
                cache_read: 0.15,
                cache_write: 1.50,
            },
            "gemini-3.5-flash-lite" => Self {
                input: 0.30,
                output: 2.50,
                cache_read: 0.03,
                cache_write: 0.30,
            },
            // OpenAI model documentation, checked 2026-10-04.
            "gpt-5.4-mini" => Self {
                input: 0.75,
                output: 4.50,
                cache_read: 0.075,
                cache_write: 0.75,
            },
            _ => return None,
        })
    }

    /// Cost of reported usage: the provider's reported charge when present, else the table.
    /// `input_tokens` is the total prompt (uncached + cache read + write).
    pub fn cost(&self, u: &Usage) -> Option<f64> {
        if let Some(c) = u.cost_usd.filter(|c| c.is_finite() && *c >= 0.0) {
            return Some(c);
        }
        let input = u.input_tokens? as f64;
        let output = u.output_tokens? as f64;
        let read = u.cached_input_tokens.unwrap_or(0) as f64;
        let write = u.cache_creation_input_tokens.unwrap_or(0) as f64;
        let plain = (input - read - write).max(0.0);
        Some((plain * self.input + read * self.cache_read + write * self.cache_write + output * self.output) / 1e6)
    }
}

/// Limits and prices of the free tier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeTierConfig {
    pub model: String,
    pub daily_usd: f64,
    pub per_visitor_per_hour: u32,
    pub per_visitor_per_day: u32,
    pub max_concurrent: usize,
    pub per_minute: u32,
    /// Output cap per call (requests asking for more are clamped).
    pub max_tokens: u32,
    /// Prompt size cap per call (characters over all messages).
    pub max_input_chars: usize,
    pub prices: Prices,
    pub disabled: bool,
    /// Where the day's spend is kept across restarts (`None` = memory only).
    pub spend_file: Option<PathBuf>,
    /// Server key given in code (tests, secret stores); else the connection's env var is used.
    #[serde(skip)]
    pub server_key: Option<ApiKey>,
}

impl Default for FreeTierConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_FREE_MODEL.into(),
            daily_usd: 30.0,
            per_visitor_per_hour: 20,
            per_visitor_per_day: 60,
            max_concurrent: 4,
            per_minute: 30,
            max_tokens: 8000,
            max_input_chars: 60_000,
            prices: Prices::for_model(DEFAULT_FREE_MODEL).expect("known model"),
            disabled: false,
            spend_file: Some(PathBuf::from("data/cache/llm/free-tier-spend.json")),
            server_key: None,
        }
    }
}

fn env_num<T: std::str::FromStr>(var: &str) -> Result<Option<T>> {
    match std::env::var(var) {
        Ok(s) if !s.trim().is_empty() => s
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| LlmError::Config(format!("{var} is not a valid number"))),
        _ => Ok(None),
    }
}

/// `1`, `true`, `yes`, `on` (any case) switch a flag on.
pub fn flag(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn env_flag(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| flag(&v))
}

impl FreeTierConfig {
    fn valid_limits(&self) -> bool {
        self.daily_usd.is_finite()
            && self.daily_usd >= 0.0
            && [
                self.prices.input,
                self.prices.output,
                self.prices.cache_read,
                self.prices.cache_write,
            ]
            .iter()
            .all(|p| p.is_finite() && *p >= 0.0)
            && self.max_concurrent <= Semaphore::MAX_PERMITS
    }

    /// Defaults overridden by `ATLAS_FREE_*` env vars. An unknown model without explicit prices
    /// is a configuration error (the cap must never run on guessed prices).
    pub fn from_env() -> Result<Self> {
        let mut c = Self::default();
        if let Ok(m) = std::env::var("ATLAS_FREE_MODEL")
            && !m.trim().is_empty()
        {
            c.model = m.trim().to_owned();
        }
        let explicit = [
            env_num::<f64>("ATLAS_FREE_PRICE_INPUT")?,
            env_num::<f64>("ATLAS_FREE_PRICE_OUTPUT")?,
            env_num::<f64>("ATLAS_FREE_PRICE_CACHE_READ")?,
            env_num::<f64>("ATLAS_FREE_PRICE_CACHE_WRITE")?,
        ];
        let base = Prices::for_model(&c.model);
        let prices = match (base, explicit) {
            (_, [Some(i), Some(o), r, w]) => Prices {
                input: i,
                output: o,
                cache_read: r.unwrap_or(i * 0.1),
                cache_write: w.unwrap_or(i * 1.25),
            },
            (Some(b), [i, o, r, w]) => Prices {
                input: i.unwrap_or(b.input),
                output: o.unwrap_or(b.output),
                cache_read: r.unwrap_or(b.cache_read),
                cache_write: w.unwrap_or(b.cache_write),
            },
            (None, _) => {
                return Err(LlmError::Config(format!(
                    "no built-in price for free-tier model '{}'; set ATLAS_FREE_PRICE_INPUT and ATLAS_FREE_PRICE_OUTPUT (USD per MTok)",
                    c.model
                )));
            }
        };
        c.prices = prices;
        if let Some(v) = env_num("ATLAS_FREE_DAILY_USD")? {
            c.daily_usd = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_PER_HOUR")? {
            c.per_visitor_per_hour = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_PER_DAY")? {
            c.per_visitor_per_day = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_CONCURRENCY")? {
            c.max_concurrent = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_PER_MINUTE")? {
            c.per_minute = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_MAX_TOKENS")? {
            c.max_tokens = v;
        }
        if let Some(v) = env_num("ATLAS_FREE_MAX_INPUT_CHARS")? {
            c.max_input_chars = v;
        }
        if let Some(p) = std::env::var_os("ATLAS_FREE_SPEND_FILE") {
            if p.is_empty() {
                return Err(LlmError::Config("ATLAS_FREE_SPEND_FILE must not be empty".into()));
            }
            c.spend_file = Some(PathBuf::from(p));
        } else if let Some(dir) = std::env::var_os("ATLAS_LLM_CACHE_DIR") {
            c.spend_file = Some(PathBuf::from(dir).join("free-tier-spend.json"));
        } else if let Some(dir) = std::env::var_os("RARE_ATLAS_DATA") {
            c.spend_file = Some(PathBuf::from(dir).join("cache/llm/free-tier-spend.json"));
        }
        c.disabled = env_flag("ATLAS_FREE_DISABLED");
        if !c.valid_limits() {
            return Err(LlmError::Config("invalid hosted budget, prices or concurrency".into()));
        }
        Ok(c)
    }
}

/// Why the free tier said no. Shown to users; never contains a key or a visitor id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaReason {
    /// Kill switch on.
    Disabled,
    /// The project's daily budget is used up.
    DailyBudget,
    /// Too many calls across all visitors this minute.
    GlobalRate,
    /// All free slots are busy right now.
    Busy,
    VisitorHourly,
    VisitorDaily,
    /// The server has no key for the free tier.
    NotConfigured,
    /// The prompt is larger than the free tier allows.
    TooLarge,
}

impl QuotaReason {
    pub fn message(self) -> &'static str {
        match self {
            Self::Disabled => "the free assistant is switched off right now",
            Self::DailyBudget => "today's free budget is used up",
            Self::GlobalRate => "the free assistant is very busy",
            Self::Busy => "all free slots are busy",
            Self::VisitorHourly => "you reached the free limit for this hour",
            Self::VisitorDaily => "you reached the free limit for today",
            Self::NotConfigured => "the free assistant is not set up on this server",
            Self::TooLarge => "this request is too large for the free assistant",
        }
    }
}

impl std::fmt::Display for QuotaReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Free-tier state for `list_connections()`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeTierStatus {
    pub enabled: bool,
    /// Why it is not available right now, if it isn't.
    pub blocked: Option<QuotaReason>,
    pub model: String,
    pub daily_usd: f64,
    pub spent_usd_today: f64,
    pub remaining_usd_today: f64,
    pub per_visitor_per_hour: u32,
    pub per_visitor_per_day: u32,
    /// Remaining calls for the visitor asked about (if one was given).
    pub visitor_remaining_hour: Option<u32>,
    pub visitor_remaining_day: Option<u32>,
    pub max_tokens: u32,
    pub prices: Prices,
    pub prices_source: String,
    /// The model as the UI names it (D48), catalog message.
    #[serde(default)]
    pub model_label: crate::model_label::ModelLabel,
}

#[derive(Debug, Default)]
struct State {
    /// UTC day number the spend belongs to.
    day: u64,
    spent: f64,
    reserved: f64,
    global: VecDeque<Instant>,
    visitors: HashMap<String, VecDeque<Instant>>,
}

#[derive(Serialize, Deserialize)]
struct SpendFile {
    day: u64,
    usd: f64,
}

/// Today's UTC date, `YYYY-MM-DD` (the cap's accounting day).
pub fn utc_date() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()[..10].to_owned()
}

fn utc_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

fn secs_to_utc_midnight() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    86_400 - now % 86_400
}

const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);
const MINUTE: Duration = Duration::from_secs(60);

/// The guard. Shared by every clone of the registry.
#[derive(Debug)]
pub struct FreeTier {
    config: FreeTierConfig,
    disabled: Arc<AtomicBool>,
    gate: Arc<Semaphore>,
    _ledger_lock: Option<Arc<std::fs::File>>,
    state: Arc<Mutex<State>>,
}

/// An admitted call; settle it with the actual cost. Dropping it unsettled keeps the reservation
/// (outcome unknown → assume the worst case).
#[derive(Debug)]
pub struct Admission {
    tier: Arc<FreeTier>,
    reserved: f64,
    settled: bool,
    _permit: OwnedSemaphorePermit,
}

impl Admission {
    pub fn reserved(&self) -> f64 {
        self.reserved
    }

    /// Replace the reservation by `actual` (USD), or keep it when `None`.
    pub fn settle(mut self, actual: Option<f64>) {
        self.settled = true;
        self.tier.settle(self.reserved, actual.unwrap_or(self.reserved));
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if !self.settled {
            self.tier.settle(self.reserved, self.reserved);
        }
    }
}

impl FreeTier {
    pub fn new(config: FreeTierConfig) -> Arc<Self> {
        let mut state = State {
            day: utc_day(),
            ..State::default()
        };
        // One writer per ledger, including across processes. Corruption or I/O errors fail closed.
        let mut ledger_lock = None;
        let mut failed = !config.valid_limits();
        if let Some(path) = &config.spend_file {
            let loaded = (|| -> std::io::Result<()> {
                if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)?;
                }
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(path.with_extension("lock"))?;
                lock.try_lock().map_err(std::io::Error::other)?;
                ledger_lock = Some(Arc::new(lock));
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let f: SpendFile = serde_json::from_slice(&bytes)?;
                        if !f.usd.is_finite() || f.usd < 0.0 || f.day > state.day {
                            return Err(std::io::Error::other("invalid budget ledger"));
                        }
                        if f.day == state.day {
                            state.spent = f.usd;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
                Ok(())
            })();
            failed |= loaded.is_err();
        }
        Arc::new(Self {
            disabled: Arc::new(AtomicBool::new(config.disabled || failed)),
            _ledger_lock: ledger_lock,
            gate: Arc::new(Semaphore::new(config.max_concurrent.clamp(1, Semaphore::MAX_PERMITS))),
            state: Arc::new(Mutex::new(state)),
            config,
        })
    }

    pub fn config(&self) -> &FreeTierConfig {
        &self.config
    }

    /// A model-specific view over the SAME admission/visitor/spend ledger. Only
    /// model, prices and its own key change; inherited zero prices never leak into
    /// a paid fallback. No second process lock, budget, rate gate or kill switch.
    pub(crate) fn with_model(
        self: &Arc<Self>,
        model: String,
        prices: Prices,
        server_key: Option<ApiKey>,
    ) -> Result<Arc<Self>> {
        let mut config = self.config.clone();
        config.model = model;
        config.prices = prices;
        config.server_key = server_key;
        if !config.valid_limits() {
            return Err(LlmError::Config("invalid shared hosted model prices".into()));
        }
        Ok(Arc::new(Self {
            config,
            disabled: self.disabled.clone(),
            gate: self.gate.clone(),
            _ledger_lock: self._ledger_lock.clone(),
            state: self.state.clone(),
        }))
    }

    pub fn server_key(&self) -> Option<&ApiKey> {
        self.config.server_key.as_ref()
    }

    /// Runtime kill switch (in addition to `ATLAS_FREE_DISABLED`).
    pub fn set_disabled(&self, disabled: bool) {
        self.disabled.store(disabled, Ordering::SeqCst);
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::SeqCst) || env_flag("ATLAS_FREE_DISABLED")
    }

    /// Conservative byte-token bound, plus framing overhead and the maximum cache-write price.
    /// Caller passes the entire serialized request, including tool definitions.
    pub fn estimate(&self, input_bytes: usize, max_tokens: u32) -> f64 {
        let p = &self.config.prices;
        ((input_bytes as f64 + 4096.0) * p.input.max(p.cache_write).max(p.cache_read)
            + f64::from(max_tokens) * p.output)
            / 1e6
    }

    pub fn cost(&self, usage: &Usage) -> Option<f64> {
        self.config.prices.cost(usage)
    }

    fn quota(reason: QuotaReason, retry_after_secs: Option<u64>) -> LlmError {
        LlmError::FreeQuota {
            reason,
            retry_after_secs,
        }
    }

    fn roll(state: &mut State) {
        let today = utc_day();
        if state.day != today {
            state.day = today;
            state.spent = 0.0;
        }
    }

    fn prune(q: &mut VecDeque<Instant>, window: Duration, now: Instant) {
        while q.front().is_some_and(|t| now.duration_since(*t) >= window) {
            q.pop_front();
        }
    }

    /// Check every limit; on success count the call and reserve `estimate` USD.
    pub fn admit(self: &Arc<Self>, visitor: &str, estimate: f64) -> Result<Admission> {
        if self.is_disabled() || !estimate.is_finite() || estimate < 0.0 {
            return Err(Self::quota(QuotaReason::Disabled, None));
        }
        let permit = self
            .gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| Self::quota(QuotaReason::Busy, Some(5)))?;
        let mut s = self.state.lock().expect("free-tier state");
        Self::roll(&mut s);
        // Monetary exhaustion does not block a configured zero-cost route;
        // shared concurrency/request/visitor gates below still apply.
        if estimate > 0.0 && s.spent + s.reserved + estimate > self.config.daily_usd {
            return Err(Self::quota(QuotaReason::DailyBudget, Some(secs_to_utc_midnight())));
        }
        let now = Instant::now();
        Self::prune(&mut s.global, MINUTE, now);
        if s.global.len() >= self.config.per_minute as usize {
            let wait = s
                .global
                .front()
                .map_or(60, |t| 60 - now.duration_since(*t).as_secs().min(59));
            return Err(Self::quota(QuotaReason::GlobalRate, Some(wait)));
        }
        // Drop idle visitors now and then so the map stays small.
        if s.visitors.len() > 10_000 {
            s.visitors
                .retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < DAY));
        }
        let q = s.visitors.entry(visitor.to_owned()).or_default();
        Self::prune(q, DAY, now);
        let last_hour = q.iter().filter(|t| now.duration_since(**t) < HOUR).count();
        if last_hour >= self.config.per_visitor_per_hour as usize {
            let oldest = q
                .iter()
                .find(|t| now.duration_since(**t) < HOUR)
                .copied()
                .unwrap_or(now);
            let wait = HOUR.saturating_sub(now.duration_since(oldest)).as_secs().max(1);
            return Err(Self::quota(QuotaReason::VisitorHourly, Some(wait)));
        }
        if q.len() >= self.config.per_visitor_per_day as usize {
            let wait = q
                .front()
                .map_or(86_400, |t| DAY.saturating_sub(now.duration_since(*t)).as_secs().max(1));
            return Err(Self::quota(QuotaReason::VisitorDaily, Some(wait)));
        }
        q.push_back(now);
        s.global.push_back(now);
        s.reserved += estimate;
        if self.persist(&s).is_err() {
            self.set_disabled(true);
            return Err(Self::quota(QuotaReason::Disabled, None));
        }
        drop(s);
        Ok(Admission {
            tier: self.clone(),
            reserved: estimate,
            settled: false,
            _permit: permit,
        })
    }

    fn settle(&self, reserved: f64, actual: f64) {
        let mut s = self.state.lock().expect("free-tier state");
        s.reserved = (s.reserved - reserved).max(0.0);
        Self::roll(&mut s);
        let actual = if actual.is_finite() && actual >= 0.0 {
            actual
        } else {
            reserved
        };
        s.spent += actual;
        // A pricing/token-bound mismatch must stop further spending until an operator reviews it.
        let persistence_failed = self.persist(&s).is_err();
        if actual > reserved || persistence_failed {
            self.set_disabled(true);
        }
    }

    /// Called with the state mutex held: reservations survive crashes and older settlements
    /// cannot overwrite newer ones. Atomic replacement, flushed before a provider call starts.
    fn persist(&self, s: &State) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(path) = &self.config.spend_file {
            let file = SpendFile {
                day: s.day,
                usd: s.spent + s.reserved,
            };
            let tmp = path.with_extension(format!("tmp{}", std::process::id()));
            let mut out = std::fs::File::create(&tmp)?;
            out.write_all(&serde_json::to_vec(&file)?)?;
            out.sync_all()?;
            drop(out);
            std::fs::rename(&tmp, path)?;
            #[cfg(unix)]
            std::fs::File::open(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new(".")),
            )?
            .sync_all()?;
        }
        Ok(())
    }

    pub fn spent_today(&self) -> f64 {
        let mut s = self.state.lock().expect("free-tier state");
        Self::roll(&mut s);
        s.spent
    }

    /// Public status; `visitor` adds that visitor's remaining calls.
    pub fn status(&self, visitor: Option<&str>, has_key: bool) -> FreeTierStatus {
        let mut s = self.state.lock().expect("free-tier state");
        Self::roll(&mut s);
        let now = Instant::now();
        let spent = s.spent + s.reserved;
        let (hour, day) = match visitor.and_then(|v| s.visitors.get(v)) {
            Some(q) => {
                let day_n = q.iter().filter(|t| now.duration_since(**t) < DAY).count() as u32;
                let hour_n = q.iter().filter(|t| now.duration_since(**t) < HOUR).count() as u32;
                (
                    Some(self.config.per_visitor_per_hour.saturating_sub(hour_n)),
                    Some(self.config.per_visitor_per_day.saturating_sub(day_n)),
                )
            }
            None if visitor.is_some() => (
                Some(self.config.per_visitor_per_hour),
                Some(self.config.per_visitor_per_day),
            ),
            None => (None, None),
        };
        drop(s);
        let remaining = (self.config.daily_usd - spent).max(0.0);
        let min_call = self.estimate(0, self.config.max_tokens.min(1000));
        let blocked = if !has_key {
            Some(QuotaReason::NotConfigured)
        } else if self.is_disabled() {
            Some(QuotaReason::Disabled)
        } else if remaining < min_call {
            Some(QuotaReason::DailyBudget)
        } else if hour == Some(0) {
            Some(QuotaReason::VisitorHourly)
        } else if day == Some(0) {
            Some(QuotaReason::VisitorDaily)
        } else {
            None
        };
        FreeTierStatus {
            enabled: blocked.is_none(),
            blocked,
            model: self.config.model.clone(),
            daily_usd: self.config.daily_usd,
            spent_usd_today: (spent * 10_000.0).round() / 10_000.0,
            remaining_usd_today: (remaining * 10_000.0).round() / 10_000.0,
            per_visitor_per_hour: self.config.per_visitor_per_hour,
            per_visitor_per_day: self.config.per_visitor_per_day,
            visitor_remaining_hour: hour,
            visitor_remaining_day: day,
            max_tokens: self.config.max_tokens,
            prices: self.config.prices,
            prices_source: prices_source(&self.config.model).into(),
            model_label: crate::model_label::model_label(&self.config.model),
        }
    }
}

/// Pseudonymous visitor id from a raw identifier (IP address, session id): sha256 with a salt
/// (`ATLAS_FREE_SALT`, else random per process), first 16 hex chars. Only this id is kept, in
/// memory; it never enters provenance or logs.
pub fn visitor_key(raw: &str) -> String {
    static SALT: OnceLock<String> = OnceLock::new();
    let salt = SALT.get_or_init(|| {
        std::env::var("ATLAS_FREE_SALT").unwrap_or_else(|_| {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(std::process::id().into());
            format!("{:016x}", h.finish())
        })
    });
    sha256_hex(format!("{salt}\u{0}{raw}").as_bytes())[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(cfg: FreeTierConfig) -> Arc<FreeTier> {
        FreeTier::new(FreeTierConfig {
            spend_file: None,
            ..cfg
        })
    }

    #[test]
    fn prices_and_cost() {
        let g = Prices::for_model(DEFAULT_FREE_MODEL).unwrap();
        assert_eq!((g.input, g.output), GPT_OSS_120B_MAX_PRICE);
        let reported = Usage {
            input_tokens: Some(1000),
            output_tokens: Some(1000),
            cost_usd: Some(0.0002),
            ..Usage::default()
        };
        assert_eq!(
            g.cost(&reported),
            Some(0.0002),
            "the provider's charge wins over the table"
        );
        assert!(prices_source(DEFAULT_FREE_MODEL).contains("2026-10-03"));
        let p = Prices::for_model("claude-sonnet-5-5").unwrap();
        assert_eq!((p.input, p.output, p.cache_read, p.cache_write), (2.0, 10.0, 0.2, 2.5));
        let u = Usage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(100_000),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            cost_usd: None,
        };
        assert!((p.cost(&u).unwrap() - 3.0).abs() < 1e-9);
        let u = Usage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(0),
            cached_input_tokens: Some(500_000),
            cache_creation_input_tokens: Some(500_000),
            cost_usd: None,
        };
        assert!((p.cost(&u).unwrap() - (0.1 + 1.25)).abs() < 1e-9);
        assert!(Prices::for_model("unknown-model").is_none());
    }

    #[test]
    fn visitor_limits() {
        let t = tier(FreeTierConfig {
            per_visitor_per_hour: 2,
            per_visitor_per_day: 3,
            ..FreeTierConfig::default()
        });
        t.admit("a", 0.0).unwrap().settle(Some(0.0));
        t.admit("a", 0.0).unwrap().settle(Some(0.0));
        let e = t.admit("a", 0.0).unwrap_err();
        assert!(
            matches!(
                e,
                LlmError::FreeQuota {
                    reason: QuotaReason::VisitorHourly,
                    retry_after_secs: Some(_)
                }
            ),
            "{e}"
        );
        t.admit("b", 0.0).unwrap().settle(Some(0.0));
        assert_eq!(t.status(Some("a"), true).visitor_remaining_hour, Some(0));
        assert_eq!(t.status(Some("b"), true).visitor_remaining_hour, Some(1));
        assert_eq!(t.status(Some("new"), true).visitor_remaining_day, Some(3));
    }

    #[test]
    fn global_rate_and_concurrency() {
        let t = tier(FreeTierConfig {
            per_minute: 2,
            max_concurrent: 1,
            ..FreeTierConfig::default()
        });
        let held = t.admit("a", 0.0).unwrap();
        assert!(matches!(
            t.admit("b", 0.0),
            Err(LlmError::FreeQuota {
                reason: QuotaReason::Busy,
                ..
            })
        ));
        held.settle(Some(0.0));
        t.admit("c", 0.0).unwrap().settle(Some(0.0));
        assert!(matches!(
            t.admit("d", 0.0),
            Err(LlmError::FreeQuota {
                reason: QuotaReason::GlobalRate,
                ..
            })
        ));
    }

    #[test]
    fn spend_cap_reservation_and_kill_switch() {
        let t = tier(FreeTierConfig {
            daily_usd: 1.0,
            ..FreeTierConfig::default()
        });
        let a = t.admit("a", 0.6).unwrap();
        // The reservation blocks a parallel call that would overshoot.
        assert!(matches!(
            t.admit("b", 0.6),
            Err(LlmError::FreeQuota {
                reason: QuotaReason::DailyBudget,
                ..
            })
        ));
        a.settle(Some(0.1));
        t.admit("b", 0.85).unwrap().settle(Some(0.85));
        assert!((t.spent_today() - 0.95).abs() < 1e-9);
        assert!(matches!(
            t.admit("c", 0.1),
            Err(LlmError::FreeQuota {
                reason: QuotaReason::DailyBudget,
                ..
            })
        ));
        // Unsettled (outcome unknown) keeps the reservation.
        let t2 = tier(FreeTierConfig::default());
        drop(t2.admit("a", 0.25).unwrap());
        assert!((t2.spent_today() - 0.25).abs() < 1e-9);
        t2.set_disabled(true);
        assert!(matches!(
            t2.admit("a", 0.0),
            Err(LlmError::FreeQuota {
                reason: QuotaReason::Disabled,
                ..
            })
        ));
        assert_eq!(t2.status(None, true).blocked, Some(QuotaReason::Disabled));
        assert!(flag("1") && flag("TRUE") && !flag("0") && !flag(""));
    }

    #[test]
    fn spend_persists_for_the_day() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spend.json");
        let cfg = FreeTierConfig {
            spend_file: Some(file.clone()),
            ..FreeTierConfig::default()
        };
        FreeTier::new(cfg.clone()).admit("a", 0.5).unwrap().settle(Some(0.42));
        assert!((FreeTier::new(cfg).spent_today() - 0.42).abs() < 1e-9);
    }

    #[test]
    fn reservations_are_durable_before_the_provider_runs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spend.json");
        let cfg = FreeTierConfig {
            spend_file: Some(file.clone()),
            ..FreeTierConfig::default()
        };
        let tier = FreeTier::new(cfg.clone());
        let a = tier.admit("a", 0.5).unwrap();
        let b = tier.admit("b", 0.4).unwrap();
        let read = || {
            serde_json::from_slice::<SpendFile>(&std::fs::read(&file).unwrap())
                .unwrap()
                .usd
        };
        assert!((read() - 0.9).abs() < 1e-9);
        a.settle(Some(0.2));
        assert!((read() - 0.6).abs() < 1e-9); // includes other outstanding call
        assert!(FreeTier::new(cfg.clone()).is_disabled()); // second process cannot spend
        drop(b);
        drop(tier);
        let restarted = FreeTier::new(cfg);
        assert!(!restarted.is_disabled());
        assert!((restarted.spent_today() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn corrupt_or_unwritable_ledgers_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spend.json");
        std::fs::write(&file, b"broken").unwrap();
        let cfg = FreeTierConfig {
            spend_file: Some(file.clone()),
            ..FreeTierConfig::default()
        };
        assert!(FreeTier::new(cfg.clone()).is_disabled());
        std::fs::remove_file(&file).unwrap();
        let t = FreeTier::new(cfg);
        std::fs::create_dir(&file).unwrap(); // replacement can no longer succeed
        assert!(t.admit("a", 0.1).is_err());
        assert!(t.is_disabled());
    }

    #[test]
    fn invalid_reservations_and_underestimates_fail_closed() {
        let t = tier(FreeTierConfig::default());
        for invalid in [f64::NAN, f64::INFINITY, -0.1] {
            assert!(t.admit("a", invalid).is_err());
        }
        t.admit("a", 0.1).unwrap().settle(Some(0.2));
        assert!(t.is_disabled());
    }

    #[test]
    fn invalid_budget_configuration_fails_closed() {
        for invalid in [f64::NAN, f64::INFINITY, -0.1] {
            let cfg = FreeTierConfig {
                daily_usd: invalid,
                ..FreeTierConfig::default()
            };
            assert!(tier(cfg).admit("a", 0.1).is_err());
            let mut cfg = FreeTierConfig::default();
            cfg.prices.output = invalid;
            assert!(tier(cfg).admit("a", 0.1).is_err());
        }
        let cfg = FreeTierConfig {
            max_concurrent: usize::MAX,
            ..FreeTierConfig::default()
        };
        assert!(tier(cfg).is_disabled());
    }

    #[test]
    fn underestimated_usage_is_still_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spend.json");
        let cfg = FreeTierConfig {
            spend_file: Some(file.clone()),
            ..FreeTierConfig::default()
        };
        let t = FreeTier::new(cfg);
        t.admit("a", 0.1).unwrap().settle(Some(0.2));
        assert!(t.is_disabled());
        let saved: SpendFile = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(saved.usd, 0.2);
    }

    #[test]
    fn visitor_keys_are_pseudonymous() {
        let k = visitor_key("203.0.113.7");
        assert_eq!(k.len(), 16);
        assert_eq!(k, visitor_key("203.0.113.7"));
        assert_ne!(k, visitor_key("203.0.113.8"));
        assert!(!k.contains("203"));
    }
}
