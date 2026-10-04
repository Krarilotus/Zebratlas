//! D48 validator check: the journey tasks (same prompts, schemas, validators, one regeneration and
//! template fallback as production) on the fixture of `docs/research/local-model/fixture.json`,
//! per connection, with a hard cap on live provider calls (cache off).
//!
//! ```text
//! ATLAS_LLM_CACHE=off cargo run -j 4 -p atlas-llm --example d48_eval -- \
//!     FIXTURE OUTPUT.jsonl MAX_CALLS CONNECTION...
//! ```
//! Connections: any registry name (`kisski`, `hosted-free`), or `old-claude` = the previous
//! free-tier default (Anthropic `claude-sonnet-5-5`, sampling off) with `ANTHROPIC_API_KEY` sent
//! as a per-request key. Keys come from the main checkout's `.env` (`ATLAS_ENV_FILE`); none is
//! printed or written. Per connection: the five tasks in German plus `translate_snippet` and
//! `reconcile` in Japanese (7 tasks, 7-14 calls).

use std::io::Write;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Instant, SystemTime};

use async_trait::async_trait;
use atlas_core::provenance::rfc3339;
use atlas_llm::cache::sha256_hex;
use atlas_llm::provider::{Availability, Provider, ProviderKind};
use atlas_llm::request::ProviderOutput;
use atlas_llm::{
    ApiKey, Cache, CacheMode, Call, Candidate, Completion, CompletionRequest, DraftKind, Fact, Llm, LlmError,
    MessageCard, Registry, UserRole,
};
use serde_json::{Value, json};

const SLOTS: [(&str, &str); 7] = [
    ("reconcile", "de"),
    ("plain_summary", "de"),
    ("why_sentence", "de"),
    ("draft_message", "de"),
    ("translate_snippet", "de"),
    ("translate_snippet", "ja"),
    ("reconcile", "ja"),
];

/// Count at the provider boundary, including failed HTTP calls (which have no Completion).
/// No probing or fallback provider calls are made by this evaluation.
#[derive(Debug)]
struct BudgetProvider {
    inner: Arc<dyn Provider>,
    used: Arc<AtomicUsize>,
    max_calls: usize,
}

#[async_trait]
impl Provider for BudgetProvider {
    fn kind(&self) -> ProviderKind {
        self.inner.kind()
    }

    fn base_url(&self) -> Option<&str> {
        self.inner.base_url()
    }

    async fn probe(&self) -> Availability {
        self.inner.probe().await
    }

    async fn complete(
        &self,
        req: &CompletionRequest,
        model: &str,
        key: Option<&ApiKey>,
    ) -> atlas_llm::Result<ProviderOutput> {
        self.used
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.max_calls).then_some(n + 1)
            })
            .map_err(|_| LlmError::InvalidRequest("evaluation provider call cap reached".into()))?;
        self.inner.complete(req, model, key).await
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() >= 5,
        "usage: d48_eval FIXTURE OUTPUT MAX_CALLS CONNECTION..."
    );
    if let Some(p) = std::env::var_os("ATLAS_ENV_FILE") {
        dotenvy::from_path(p)?;
    }
    anyhow::ensure!(
        std::env::var("ATLAS_LLM_CACHE").as_deref() == Ok("off"),
        "set ATLAS_LLM_CACHE=off"
    );
    let max_calls: usize = args[3].parse()?;
    let fixture_bytes = std::fs::read(&args[1])?;
    let fixture: Value = serde_json::from_slice(&fixture_bytes)?;
    let facts: Vec<Fact> = serde_json::from_value(fixture["facts"].clone())?;
    let candidates: Vec<Candidate> = serde_json::from_value(fixture["candidates"].clone())?;
    let card: MessageCard = serde_json::from_value(fixture["card"].clone())?;
    let inputs: Vec<String> = fixture["sources"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let mut registry = Registry::from_toml(
        "[[connections]]\nname = \"old-claude\"\npreset = \"anthropic\"\ndefault_model = \"claude-sonnet-5-5\"\nsampling = false\n",
        false,
    )?;
    let used = Arc::new(AtomicUsize::new(0));
    for name in &args[4..] {
        let mut connection = registry.get(name)?.clone();
        connection.provider = Arc::new(BudgetProvider {
            inner: connection.provider,
            used: used.clone(),
            max_calls,
        });
        registry.add(connection)?;
    }
    let llm = Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off));
    // Preserve previous runs; each evaluation needs its own output filename.
    let mut out = std::io::BufWriter::new(std::fs::File::create_new(&args[2])?);
    let manifest_path = format!("{}.manifest.json", args[2]);
    let mut manifest_file = std::fs::File::create_new(&manifest_path)?;
    let started_at = rfc3339(SystemTime::now());
    let manifest = json!({
        "@type": "prov:Activity", "status": "running", "started_at": started_at,
        "fixture": { "path": args[1], "sha256": sha256_hex(&fixture_bytes),
            "version": fixture["format"], "record_locator": "/facts,/candidates,/card,/queries,/snippet",
            "sources": fixture["sources"] },
        "software": { "version": env!("CARGO_PKG_VERSION"),
            "commit": std::env::var("ATLAS_EVAL_COMMIT").ok(),
            "source_sha256": sha256_hex(include_bytes!("d48_eval.rs")) },
        "parameters": { "max_provider_calls": max_calls, "connections": &args[4..],
            "cache": "off", "slots": SLOTS, "max_regenerations": 1 },
        "output": args[2],
    });
    serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
    manifest_file.flush()?;
    for conn in &args[4..] {
        let mut call = Call::new(conn.clone())
            .with_inputs(inputs.clone())
            .with_visitor("d48-eval");
        if conn == "old-claude" {
            let Some(k) = ApiKey::from_env("ANTHROPIC_API_KEY") else {
                eprintln!("{conn}: skipped (no ANTHROPIC_API_KEY)");
                continue;
            };
            call = call.with_key(k);
        }
        for (task, lang) in SLOTS {
            // Each task makes at most two provider calls (one regeneration).
            if used.load(Ordering::SeqCst).saturating_add(2) > max_calls {
                eprintln!("call cap {max_calls} reached; stopping");
                break;
            }
            let before = used.load(Ordering::SeqCst);
            let t0 = Instant::now();
            let result = match task {
                "reconcile" => serde_json::to_value(
                    atlas_llm::reconcile(
                        &llm,
                        &call,
                        fixture["queries"][lang].as_str().unwrap_or(""),
                        lang,
                        &candidates,
                    )
                    .await,
                )?,
                "plain_summary" => serde_json::to_value(
                    atlas_llm::plain_summary(&llm, &call, fixture["condition"].as_str().unwrap_or(""), &facts, lang)
                        .await,
                )?,
                "why_sentence" => serde_json::to_value(
                    atlas_llm::why_sentence(&llm, &call, &card.recipient, &card.facts, lang).await,
                )?,
                "draft_message" => serde_json::to_value(
                    atlas_llm::draft_message(&llm, &call, &card, UserRole::Parent, DraftKind::Outreach, lang, None)
                        .await,
                )?,
                _ => serde_json::to_value(
                    atlas_llm::translate_snippet(&llm, &call, fixture["snippet"].as_str().unwrap_or(""), lang).await,
                )?,
            };
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            let core = if task == "draft_message" {
                &result["body"]
            } else {
                &result
            };
            let calls: Vec<Completion> = serde_json::from_value(core["calls"].clone()).unwrap_or_default();
            let provider_calls = used.load(Ordering::SeqCst) - before;
            let first_pass = core["validation"]["attempts"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
            // Missing upstream prices are unknown, rather than a measured zero-dollar charge.
            let cost = calls.first().and_then(|_| {
                calls
                    .iter()
                    .try_fold(0.0, |total, c| c.response.usage.cost_usd.map(|cost| total + cost))
            });
            let rec = json!({
                "run_manifest": manifest_path,
                "connection": conn, "task": task, "lang": lang, "elapsed_ms": elapsed_ms,
                "first_pass": first_pass, "fallback": core["origin"] == "template", "origin": core["origin"],
                "calls": calls.len(),
                "provider_calls": provider_calls,
                "provenance": calls.iter().map(|c| &c.provenance).collect::<Vec<_>>(),
                "input_tokens": calls.iter().filter_map(|c| c.response.usage.input_tokens).sum::<u64>(),
                "output_tokens": calls.iter().filter_map(|c| c.response.usage.output_tokens).sum::<u64>(),
                "reported_cost_usd": cost,
                "models": atlas_llm::model_labels(&calls),
                "routed_provider": calls.first().and_then(|c| c.provenance.activity.parameters.get("sent.routed_provider")),
                "reconcile_correct": (task == "reconcile").then(|| result["choice"] == fixture["expected_choice"]),
                "fallback_reason": core["validation"]["fallback_reason"],
                "attempts": core["validation"]["attempts"],
                "result": result,
            });
            writeln!(out, "{rec}")?;
            out.flush()?;
            eprintln!(
                "{conn} {task} {lang}: first_pass={first_pass} origin={} calls={} {elapsed_ms} ms",
                core["origin"],
                calls.len()
            );
        }
    }
    out.flush()?;
    let mut manifest = manifest;
    manifest["status"] = json!("complete");
    manifest["ended_at"] = json!(rfc3339(SystemTime::now()));
    manifest["provider_calls"] = json!(used.load(Ordering::SeqCst));
    manifest["output_sha256"] = json!(sha256_hex(&std::fs::read(&args[2])?));
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    eprintln!("live calls used: {} (cap {max_calls})", used.load(Ordering::SeqCst));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FailingProvider(AtomicUsize);

    #[async_trait]
    impl Provider for FailingProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenAiCompatible
        }

        async fn probe(&self) -> Availability {
            Availability::ready("test")
        }

        async fn complete(
            &self,
            _: &CompletionRequest,
            _: &str,
            _: Option<&ApiKey>,
        ) -> atlas_llm::Result<ProviderOutput> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(LlmError::Unavailable("test failure".into()))
        }
    }

    #[tokio::test]
    async fn failed_calls_consume_the_shared_budget_and_cap_blocks_dispatch() {
        let inner = Arc::new(FailingProvider(AtomicUsize::new(0)));
        let used = Arc::new(AtomicUsize::new(0));
        let provider = BudgetProvider {
            inner: inner.clone(),
            used: used.clone(),
            max_calls: 1,
        };
        let other = BudgetProvider {
            inner: inner.clone(),
            used: used.clone(),
            max_calls: 1,
        };
        let request = CompletionRequest::new(vec![]);
        assert!(matches!(
            provider.complete(&request, "test", None).await,
            Err(LlmError::Unavailable(_))
        ));
        assert!(matches!(
            other.complete(&request, "test", None).await,
            Err(LlmError::InvalidRequest(_))
        ));
        assert_eq!(used.load(Ordering::SeqCst), 1);
        assert_eq!(
            inner.0.load(Ordering::SeqCst),
            1,
            "the second connection never reached its provider"
        );
    }
}
