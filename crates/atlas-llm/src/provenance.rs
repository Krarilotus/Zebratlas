//! PROV-O record of one LLM call (D10/D11), in atlas-core's types.
//!
//! - `prov:Activity` `activity:llm-call:<key16>`: parameters = what was actually sent;
//! - `prov:wasAssociatedWith` agent = provider + model (requested and reported) + CLI/client version;
//! - `prov:used` = the prompt entity (sha256 of messages + schema) and the caller's input ids;
//! - `prov:generated` = the response entity (sha256 of the response text).

use std::collections::BTreeMap;

use atlas_core::provenance::{Activity, ActivityIdx, Agent, Provenance, SourceEntity};
use serde::{Deserialize, Serialize};

use crate::cache::sha256_hex;
use crate::request::Usage;

/// Activity id prefix of LLM calls.
pub const LLM_CALL: &str = "activity:llm-call";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmCall {
    pub activity: Activity,
    /// `prov:used`: the prompt.
    pub prompt: SourceEntity,
    /// `prov:used`: caller-supplied input ids (PMIDs, node ids, snapshot ids ...).
    pub inputs: Vec<String>,
    /// `prov:generated`: the response.
    pub response: SourceEntity,
}

/// Facts needed to build a record.
pub(crate) struct CallFacts<'a> {
    pub connection: &'a str,
    pub provider: &'a str,
    pub base_url: Option<&'a str>,
    pub requested_model: &'a str,
    pub reported_model: Option<&'a str>,
    pub agent_version: Option<&'a str>,
    pub cache_key: &'a str,
    pub cache_file: String,
    pub prompt_hash: String,
    pub prompt_bytes: u64,
    pub response_text: &'a str,
    pub sent: &'a BTreeMap<String, String>,
    pub usage: &'a Usage,
    pub latency_ms: u64,
    pub started_at: &'a str,
    pub ended_at: &'a str,
    pub cache_hit: bool,
    pub inputs: &'a [String],
}

impl LlmCall {
    pub(crate) fn new(f: CallFacts<'_>) -> Self {
        let response_hash = sha256_hex(f.response_text.as_bytes());
        let prompt = SourceEntity {
            id: format!("llm-prompt:{}", f.prompt_hash),
            url: String::new(),
            file: f.cache_file.clone(),
            version: None,
            retrieved_at: Some(f.started_at.to_owned()),
            sha256: Some(f.prompt_hash.clone()),
            bytes: f.prompt_bytes,
            licence: None,
        };
        let response = SourceEntity {
            id: format!("llm-response:{response_hash}"),
            url: String::new(),
            file: f.cache_file,
            version: f.reported_model.map(str::to_owned),
            retrieved_at: Some(f.ended_at.to_owned()),
            sha256: Some(response_hash),
            bytes: f.response_text.len() as u64,
            licence: None,
        };
        let mut parameters = BTreeMap::new();
        let mut p = |k: &str, v: &str| {
            parameters.insert(k.to_owned(), v.to_owned());
        };
        p("connection", f.connection);
        p("provider", f.provider);
        if let Some(u) = f.base_url {
            p("base_url", u);
        }
        p("model.requested", f.requested_model);
        p("model.reported", f.reported_model.unwrap_or("not-reported"));
        p(
            "model.label",
            &crate::model_label::model_label(f.requested_model).fallback,
        );
        if let Some(c) = f.usage.cost_usd {
            p("cost_usd.reported", &format!("{c:.6}"));
        }
        p("cache_key", f.cache_key);
        p("cache", if f.cache_hit { "hit" } else { "miss" });
        p("latency_ms", &f.latency_ms.to_string());
        p("prov:used", &prompt.id);
        p("prov:generated", &response.id);
        if !f.inputs.is_empty() {
            p("inputs", &f.inputs.join(" "));
        }
        for (k, v) in f.sent {
            parameters.insert(format!("sent.{k}"), v.clone());
        }
        let mut counts = BTreeMap::new();
        for (k, v) in [
            ("input_tokens", f.usage.input_tokens),
            ("output_tokens", f.usage.output_tokens),
            ("cached_input_tokens", f.usage.cached_input_tokens),
            ("cache_creation_input_tokens", f.usage.cache_creation_input_tokens),
        ] {
            if let Some(n) = v {
                counts.insert(k.to_owned(), n);
            }
        }
        let model = f.reported_model.unwrap_or(f.requested_model);
        let activity = Activity {
            id: format!("{LLM_CALL}:{}", &f.cache_key[..16.min(f.cache_key.len())]),
            label: format!("LLM call via {} ({model})", f.connection),
            started_at: Some(f.started_at.to_owned()),
            ended_at: Some(f.ended_at.to_owned()),
            used: vec![],
            parameters,
            agent: Agent {
                name: format!("llm:{}/{model}", f.provider),
                version: f.agent_version.unwrap_or("unknown").to_owned(),
                commit: None,
            },
            counts,
        };
        Self {
            activity,
            prompt,
            inputs: f.inputs.to_vec(),
            response,
        }
    }

    /// Add prompt + response entities and the activity to a snapshot's registry.
    /// The response entity is linked by the `prov:generated` parameter.
    pub fn register(&self, prov: &mut Provenance) -> ActivityIdx {
        let used = prov.add_entity(self.prompt.clone());
        prov.add_entity(self.response.clone());
        let mut a = self.activity.clone();
        a.used = vec![used];
        prov.add_activity(a)
    }
}

pub(crate) fn now_rfc3339() -> String {
    humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string()
}
