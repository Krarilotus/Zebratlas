//! Live smoke tests: one call per selected connection. Off unless `ATLAS_LIVE_LLM=1`.
//!
//! `ATLAS_LIVE_LLM_CONNECTIONS` (default `kisski`) selects connections, comma-separated. CLIs are
//! only called when `probe()` reports them logged in, and then exactly once. Keys come from the
//! git-ignored `.env` (never printed). Cache is off, so every run is a real call.

use std::time::Duration;

use atlas_llm::{Cache, CacheMode, Call, CompletionRequest, JsonSchema, Llm, Message, Registry};
use serde::Deserialize;
use serde_json::json;

fn enabled() -> Option<Vec<String>> {
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env"));
    if std::env::var("ATLAS_LIVE_LLM").as_deref() != Ok("1") {
        eprintln!("skipped: set ATLAS_LIVE_LLM=1");
        return None;
    }
    let list = std::env::var("ATLAS_LIVE_LLM_CONNECTIONS").unwrap_or_else(|_| "kisski".into());
    Some(
        list.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

fn llm() -> Llm {
    Llm::new(
        Registry::presets(true).unwrap(),
        Cache::new(std::env::temp_dir().join("atlas-llm-live"), CacheMode::Off),
    )
}

#[derive(Debug, Deserialize)]
struct Answer {
    gene: String,
}

#[tokio::test]
async fn live_smoke() {
    let Some(selected) = enabled() else { return };
    let llm = llm();
    for name in selected.iter().filter(|n| *n != "hosted-free") {
        let availability = llm.probe(name).await.unwrap();
        eprintln!("{name}: probe = {availability:?}");
        if !availability.available {
            eprintln!("{name}: skipped (not available)");
            continue;
        }
        let mut req = CompletionRequest::new(vec![
            Message::system("You answer in at most five words."),
            Message::user("Which gene is mutated in STXBP1 encephalopathy? Answer with the gene symbol only."),
        ])
        .with_deadline(Duration::from_secs(120));
        if name == "claude-code" {
            req = req.with_model("haiku"); // cheapest
        }
        if !name.ends_with("-code") && name != "codex" {
            req = req.with_temperature(0.0).with_max_tokens(400);
        }
        let done = llm
            .complete(&Call::new(name.clone()).with_inputs(["live-smoke"]), req)
            .await
            .unwrap();
        eprintln!(
            "{name}: text={:?} requested={} reported={:?} latency={}ms usage={:?} agent={} v{}",
            done.response.text,
            done.response.requested_model,
            done.response.reported_model,
            done.response.latency_ms,
            done.response.usage,
            done.provenance.activity.agent.name,
            done.provenance.activity.agent.version,
        );
        assert!(
            done.response.text.to_uppercase().contains("STXBP1"),
            "{name}: {}",
            done.response.text
        );
    }
}

/// Structured output through the guard (1 call, 2 on retry). KISSKI only.
#[tokio::test]
async fn live_json_kisski() {
    let Some(selected) = enabled() else { return };
    if !selected.iter().any(|s| s == "kisski") {
        return;
    }
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["gene"],
                        "properties": {"gene": {"type": "string", "enum": ["STXBP1", "SCN1A", "SCN2A"]}}});
    let req = CompletionRequest::new(vec![Message::user(
        "Which of these genes encodes Munc18-1: STXBP1, SCN1A or SCN2A? Reply as JSON.",
    )])
    .with_schema(JsonSchema::new("gene_pick", schema))
    .with_temperature(0.0)
    .with_max_tokens(400);
    let out = llm().complete_json::<Answer>(&Call::new("kisski"), req).await.unwrap();
    eprintln!("kisski json: {:?} after {} call(s)", out.value, out.calls.len());
    assert_eq!(out.value.gene, "STXBP1");
}

/// Journey tasks on KISSKI (≤ 3 calls): German summary (1, or 2 on regeneration) + reconcile (1).
/// Needs `ATLAS_LIVE_LLM_TASKS=1` as well.
#[tokio::test]
async fn live_tasks_kisski() {
    use atlas_llm::{Candidate, Fact, plain_summary, reconcile};
    let Some(selected) = enabled() else { return };
    if !selected.iter().any(|s| s == "kisski") || std::env::var("ATLAS_LIVE_LLM_TASKS").as_deref() != Ok("1") {
        return;
    }
    let llm = llm();
    let call = Call::new("kisski");
    let facts = [
        Fact::new(
            "E1",
            "STXBP1 encephalopathy (ORPHA:178469) is a rare brain condition caused by changes in the STXBP1 gene.",
        ),
        Fact::new(
            "E2",
            "Most children have seizures that start in the first year of life and have developmental delay.",
        ),
    ];
    let s = plain_summary(&llm, &call, "STXBP1 encephalopathy", &facts, "de").await;
    eprintln!(
        "summary: origin={:?} attempts={:?} reason={:?}\n  {}",
        s.origin,
        s.validation.attempts,
        s.validation.fallback_reason,
        s.text()
    );
    for c in &s.sentences {
        eprintln!("  - {:?} {}", c.cites, c.text);
    }
    let candidates = [
        Candidate {
            id: "ORPHA:178469".into(),
            label: "STXBP1 encephalopathy with epilepsy".into(),
            synonyms: vec!["STXBP1 encephalopathy".into()],
            kind: Some("disease".into()),
        },
        Candidate {
            id: "ORPHA:33069".into(),
            label: "Dravet syndrome".into(),
            synonyms: vec![],
            kind: None,
        },
        Candidate {
            id: "ORPHA:1934".into(),
            label: "Early infantile epileptic encephalopathy".into(),
            synonyms: vec![],
            kind: None,
        },
    ];
    let r = reconcile(
        &llm,
        &call,
        "Arztbrief: Nachweis einer de-novo Variante im STXBP1-Gen, Epilepsie seit dem 3. Lebensmonat",
        "de",
        &candidates,
    )
    .await;
    eprintln!(
        "reconcile: {:?} {:?} {:?} mention={:?} alts={:?} calls={}",
        r.origin,
        r.choice,
        r.confidence,
        r.mention,
        r.alternatives,
        r.calls.len()
    );
    assert_eq!(r.choice.as_deref(), Some("ORPHA:178469"));
}

/// `hosted-free` (D23, D48): exactly one structured call on the project's OpenRouter key
/// (gpt-oss-120b, ZDR endpoints only). Runs with `ATLAS_LIVE_LLM=1
/// ATLAS_LIVE_LLM_CONNECTIONS=hosted-free`; skipped without `OPENROUTER_API_KEY`. The spend file is
/// not touched (memory only).
#[tokio::test]
async fn live_hosted_free() {
    use atlas_llm::{FreeTierConfig, HOSTED_FREE};
    let Some(selected) = enabled() else { return };
    if !selected.iter().any(|s| s == HOSTED_FREE) {
        return;
    }
    if std::env::var("OPENROUTER_API_KEY").map_or(true, |k| k.trim().is_empty()) {
        eprintln!("hosted-free: skipped (no OPENROUTER_API_KEY)");
        return;
    }
    let mut reg = Registry::presets(false).unwrap();
    reg.set_free_tier(
        HOSTED_FREE,
        FreeTierConfig {
            spend_file: None,
            ..FreeTierConfig::from_env().unwrap()
        },
    );
    let llm = Llm::new(
        reg,
        Cache::new(std::env::temp_dir().join("atlas-llm-live"), CacheMode::Off),
    );
    eprintln!("hosted-free: probe = {:?}", llm.probe(HOSTED_FREE).await.unwrap());
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["gene"],
                        "properties": {"gene": {"type": "string", "enum": ["STXBP1", "SCN1A", "SCN2A"]}}});
    let req = CompletionRequest::new(vec![Message::user(
        "Which of these genes encodes Munc18-1: STXBP1, SCN1A or SCN2A? Reply as JSON.",
    )])
    .with_schema(JsonSchema::new("gene_pick", schema))
    .with_temperature(0.0)
    .with_max_tokens(2000);
    let done = llm
        .complete(&Call::new(HOSTED_FREE).with_visitor("live-test"), req)
        .await
        .unwrap();
    eprintln!(
        "hosted-free: text={:?} requested={} reported={:?} latency={}ms usage={:?} cost=${:?} sent={:?}",
        done.response.text,
        done.response.requested_model,
        done.response.reported_model,
        done.response.latency_ms,
        done.response.usage,
        done.response.cost_usd,
        done.provenance
            .activity
            .parameters
            .iter()
            .filter(|(k, _)| k.starts_with("sent."))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        done.response.json.as_ref().and_then(|j| j["gene"].as_str()),
        Some("STXBP1")
    );
    let status = llm.list_connections_for(false, Some("live-test")).await;
    let free = status
        .iter()
        .find(|c| c.name == HOSTED_FREE)
        .and_then(|c| c.free_tier.clone());
    eprintln!("hosted-free: status = {free:?}");
}

/// Prompt caching on a `hosted-free` configured on Anthropic (D23 setup; D48 moved the default to
/// OpenRouter): two calls with the same stable prefix (shared preamble + task
/// instructions + schema) and different dynamic tails; the second must read the prefix from
/// Anthropic's cache. Exactly 2 calls. `ATLAS_LIVE_LLM=1 ATLAS_LIVE_LLM_CONNECTIONS=hosted-cache`.
#[tokio::test]
async fn live_prompt_cache() {
    use atlas_llm::tasks::PREAMBLE;
    use atlas_llm::{FreeTierConfig, HOSTED_FREE, Prices};
    let Some(selected) = enabled() else { return };
    if !selected.iter().any(|s| s == "hosted-cache") {
        return;
    }
    if std::env::var("ANTHROPIC_API_KEY").map_or(true, |k| k.trim().is_empty()) {
        eprintln!("prompt cache: skipped (no ANTHROPIC_API_KEY)");
        return;
    }
    let mut reg = Registry::from_toml(
        "[[connections]]\nname = \"hosted-free\"\nkind = \"anthropic\"\nkey_env = \"ANTHROPIC_API_KEY\"\n\
         default_model = \"claude-sonnet-5-5\"\nsampling = false\n",
        false,
    )
    .unwrap();
    let cfg = FreeTierConfig {
        spend_file: None,
        model: "claude-sonnet-5-5".into(),
        prices: Prices::for_model("claude-sonnet-5-5").unwrap(),
        ..FreeTierConfig::from_env().unwrap()
    };
    let prices: Prices = cfg.prices;
    reg.set_free_tier(HOSTED_FREE, cfg);
    let llm = Llm::new(
        reg,
        Cache::new(std::env::temp_dir().join("atlas-llm-live"), CacheMode::Off),
    );
    let schema = json!({"type": "object", "additionalProperties": false, "required": ["sentences"],
        "properties": {"sentences": {"type": "array", "items": {"type": "object", "additionalProperties": false,
            "required": ["text", "cites"], "properties": {"text": {"type": "string"},
            "cites": {"type": "array", "items": {"type": "string", "enum": ["E1", "E2"]}}}}}}});
    let task = "Task: plain summary. You explain a rare condition to a parent who has just heard the diagnosis. \
                Write exactly two sentences: what this condition is, and what it usually means in everyday life.";
    let mut usages = vec![];
    for (cond, gene) in [("STXBP1 encephalopathy", "STXBP1"), ("SCN2A-related epilepsy", "SCN2A")] {
        let req = CompletionRequest::new(vec![
            Message::system(PREAMBLE).cached(),
            Message::system(task).cached(),
            Message::user(format!(
                "Write in English.\nCondition: {cond}\nFacts:\n[E1] {cond} is a rare brain condition caused by changes in the {gene} gene.\n\
                 [E2] Most children have seizures that start in the first year of life.\n\nReply as JSON."
            )),
        ])
        .with_schema(JsonSchema::new("plain_summary", schema.clone()))
        .with_max_tokens(1500);
        let done = llm
            .complete(&Call::new(HOSTED_FREE).with_visitor("live-cache"), req)
            .await
            .unwrap();
        let u = done.response.usage.clone();
        let uncached = Prices::cost(
            &prices,
            &atlas_llm::Usage {
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(0),
                ..u.clone()
            },
        )
        .unwrap();
        eprintln!(
            "{cond}: total_in={:?} cache_write={:?} cache_read={:?} out={:?} cost=${:.6} (without caching ${uncached:.6}) breakpoints={}",
            u.input_tokens,
            u.cache_creation_input_tokens,
            u.cached_input_tokens,
            u.output_tokens,
            done.response.cost_usd.unwrap(),
            done.provenance
                .activity
                .parameters
                .get("sent.cache_breakpoints")
                .map_or("-", String::as_str),
        );
        usages.push((u, done.response.cost_usd.unwrap(), uncached));
    }
    let (second, cost2, uncached2) = &usages[1];
    assert!(
        second.cached_input_tokens.unwrap_or(0) > 0,
        "second call should read the cached prefix: {second:?}"
    );
    let in_price = |u: &atlas_llm::Usage, cached: bool| {
        let read = u.cached_input_tokens.unwrap_or(0) as f64;
        let total = u.input_tokens.unwrap_or(0) as f64;
        if cached {
            (total - read) * prices.input + read * prices.cache_read
        } else {
            total * prices.input
        }
    };
    eprintln!(
        "prompt cache: second call input cost {:.6} vs {:.6} uncached ({:.0}% less input cost); total call {:.6} vs {:.6}",
        in_price(second, true) / 1e6,
        in_price(second, false) / 1e6,
        100.0 * (1.0 - in_price(second, true) / in_price(second, false)),
        cost2,
        uncached2
    );
}
