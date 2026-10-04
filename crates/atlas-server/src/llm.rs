//! atlas-llm in the journeys (D11): per-request connection and BYO key from headers, PROV-O
//! registration of every call (U1.1), and the fact lists the text tasks cite.
//!
//! The key (`X-LLM-Key`) is wrapped in `ApiKey` at once: never stored, logged or echoed.

use std::sync::{Arc, Mutex};

use atlas_core::Atlas;
use atlas_core::provenance::Provenance;
use atlas_llm::{ApiKey, Call, Completion, Fact, Llm};
use axum::http::HeaderMap;

use crate::connections::Found;
use crate::views;

/// Runtime registry of LLM calls made by this server process (bounded).
#[derive(Default)]
pub struct Runtime {
    prov: Mutex<Provenance>,
}

/// Activities kept before the registry starts over (u16 indexes).
const MAX_ACTIVITIES: usize = 20_000;

impl Runtime {
    /// Register the calls; returns their activity ids.
    pub fn register(&self, calls: &[Completion]) -> Vec<String> {
        let mut prov = self.prov.lock().expect("runtime provenance lock");
        if prov.activities.len() >= MAX_ACTIVITIES {
            *prov = Provenance::default();
        }
        calls
            .iter()
            .map(|c| {
                let i = c.provenance.register(&mut prov);
                prov.activity(i).id.clone()
            })
            .collect()
    }

    /// PROV-O record of a registered call: activity plus its prompt/response entities.
    pub fn lookup(&self, id: &str) -> Option<serde_json::Value> {
        let prov = self.prov.lock().expect("runtime provenance lock");
        let a = prov.activities.iter().rev().find(|a| a.id == id)?;
        let used: Vec<serde_json::Value> = a.used.iter().map(|&e| crate::prov::entity(prov.entity(e))).collect();
        Some(serde_json::json!({
            "@id": a.id,
            "@type": "prov:Activity",
            "kind": "llm_call",
            "rdfs:label": a.label,
            "prov:startedAtTime": a.started_at,
            "prov:endedAtTime": a.ended_at,
            "prov:used": used,
            "prov:wasAssociatedWith": { "@type": "prov:SoftwareAgent", "name": a.agent.name, "version": a.agent.version },
            "parameters": a.parameters,
            "counts": a.counts,
        }))
    }
}

#[derive(Clone)]
pub struct LlmState {
    pub llm: Option<Arc<Llm>>,
    pub runtime: Arc<Runtime>,
}

impl LlmState {
    pub fn from_env() -> Self {
        let llm = match Llm::from_env() {
            Ok(l) => Some(Arc::new(l)),
            Err(e) => {
                eprintln!("atlas-llm unavailable ({e}); journeys use templates");
                None
            }
        };
        Self {
            llm,
            runtime: Arc::new(Runtime::default()),
        }
    }
}

/// Connection from `X-LLM-Connection` (default `ATLAS_LLM_DEFAULT`), key from `X-LLM-Key`.
/// Without `X-LLM-Connection`, `Llm::default_connection` picks the hosted free tier (no own key;
/// D48: OpenRouter gpt-oss-120b) or the configured default, and the call may fall back along
/// `Llm::default_chain` on outages (never with an own key or an explicitly named connection). The visitor (for per-visitor free-tier limits) is a salted hash of
/// the client address the proxy forwards (`X-Forwarded-For` / `X-Real-IP`), else `local`.
pub fn call(llm: &Llm, headers: &HeaderMap, inputs: impl IntoIterator<Item = String>) -> Call {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let key = header("x-llm-key");
    let named = header("x-llm-connection").map(str::to_owned);
    let fallback = named.is_none() && key.is_none();
    let connection = named.unwrap_or_else(|| llm.default_connection(key.is_some()));
    let trust_proxy = atlas_accounts::AccountsConfig::from_env().trust_forwarded_for;
    let client = trust_proxy
        .then(|| header("x-forwarded-for"))
        .flatten()
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .or_else(|| trust_proxy.then(|| header("x-real-ip")).flatten())
        .unwrap_or("local");
    let mut call = Call::new(connection)
        .with_private()
        .with_fallback(fallback)
        .with_inputs(inputs)
        .with_visitor(atlas_llm::visitor_key(client));
    if let Some(k) = key {
        call = call.with_key(ApiKey::new(k));
    }
    call
}

/// Facts of a card for the text tasks (keys are edge ids or node ids).
pub fn card_facts(f: &Found) -> Vec<Fact> {
    let mut seen = std::collections::HashSet::new();
    f.facts
        .iter()
        .filter(|x| seen.insert(x.key.clone()))
        .map(|x| Fact::message(x.key.clone(), x.msg.clone()))
        .collect()
}

/// Facts about a condition for the plain summary: definition, causal genes, top symptoms,
/// prevalence, onset. Keys are the condition id or atlas edge ids.
pub fn condition_facts(atlas: &Atlas, d: atlas_core::DiseaseIdx) -> Vec<Fact> {
    let x = atlas.disease_at(d);
    let mut facts = Vec::new();
    if !x.definition.is_empty() {
        facts.push(Fact::message(
            x.id.clone(),
            crate::copy_extra::msg(
                "facts.condition_definition",
                serde_json::json!({"arg0": x.name, "arg1": x.definition}),
            ),
        ));
    }
    for (_, symbol, edge) in crate::nodes::causal_genes(atlas, d) {
        facts.push(Fact::message(
            edge,
            crate::copy_extra::msg(
                "facts.condition_causal_gene",
                serde_json::json!({"arg0": x.name, "symbol": symbol}),
            ),
        ));
    }
    let mut ph: Vec<_> = x.phenotypes.iter().collect();
    ph.sort_by(|a, b| atlas.hpo.ic(b.term).total_cmp(&atlas.hpo.ic(a.term)));
    for e in ph.iter().take(5) {
        let edge = views::phenotype_edge(atlas, &x.id, e, false);
        facts.push(Fact::message(
            edge.id,
            crate::copy_extra::msg(
                "facts.condition_symptom",
                serde_json::json!({"arg0": atlas.hpo.term(e.term).name, "arg1": x.name}),
            ),
        ));
    }
    if let Some(p) = x.prevalence.iter().find(|p| p.prevalence_class.is_some()) {
        facts.push(Fact::message(format!("{}#prevalence", x.id), crate::copy_extra::msg("facts.condition_prevalence", serde_json::json!({"arg0": x.name, "arg1": p.kind.to_lowercase(), "arg2": p.prevalence_class.as_deref().unwrap_or(""), "arg3": p.geography}))));
    }
    let onset: Vec<&str> = x.onset.keys().map(String::as_str).collect();
    if !onset.is_empty() {
        facts.push(Fact::message(
            format!("{}#onset", x.id),
            crate::copy_extra::msg(
                "facts.condition_onset",
                serde_json::json!({"arg0": x.name, "arg1": onset.join(", ")}),
            ),
        ));
    }
    facts
}
