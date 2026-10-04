//! Bring-your-own LLM access for the atlas (D10, D11, D18).
//!
//! - HTTP providers through `genai`: OpenAI, Anthropic, OpenRouter, Gemini and any
//!   OpenAI-compatible base URL (Ollama, LM Studio, vLLM, KISSKI demo preset).
//! - CLIs on the user's machine (Claude Code, Codex, Gemini, OpenCode), tool-free,
//!   bounded, without a shell, never touching the user's credentials.
//! - Content-addressed disk cache (`data/cache/llm/<sha256>.json`): deterministic replays.
//! - Every call yields a PROV-O record in atlas-core's types.
//! - `complete_json`: schema-validated structured output with one guarded retry.
//!
//! ```no_run
//! # async fn demo() -> atlas_llm::Result<()> {
//! use atlas_llm::{Call, CompletionRequest, Llm, Message};
//! let llm = Llm::from_env()?;
//! let req = CompletionRequest::new(vec![Message::user("Say hi")]).with_temperature(0.0);
//! let done = llm.complete(&Call::new("kisski").with_inputs(["PMID:1"]), req).await?;
//! println!("{} ({})", done.response.text, done.provenance.activity.id);
//! # Ok(()) }
//! ```

pub mod cache;
pub mod cli;
pub mod connector;
pub mod error;
pub mod free_tier;
pub mod http;
pub mod llm;
pub mod model_label;
pub mod provenance;
pub mod provider;
pub mod registry;
pub mod request;
pub mod routing;
pub mod secret;
pub mod tasks;

pub use cache::{Cache, CacheMode};
pub use error::{LlmError, Result};
pub use free_tier::{FreeTierConfig, FreeTierStatus, HOSTED_FREE, Prices, QuotaReason, visitor_key};
pub use llm::{Call, Completion, JsonCompletion, Llm, check_json, extract_json, fallbacks_from_env};
pub use model_label::{ModelLabel, model_label, model_labels};
pub use provenance::LlmCall;
pub use provider::{Availability, KeyPolicy, Provider, ProviderKind};
pub use registry::{ConnectionConfig, ConnectionInfo, Registry};
pub use request::{CompletionRequest, CompletionResponse, JsonSchema, Message, Role, Settings, Usage};
pub use secret::ApiKey;
pub use tasks::{
    Candidate, Draft, DraftKind, Fact, Generated, MessageCard, Origin, Reconciled, Translation, UserRole,
    draft_message, plain_summary, reconcile, translate_snippet, why_sentence,
};
