//! Live smoke run through KISSKI (demo preset), STXBP1 in English and German, at most 5 model
//! calls. Ignored by default:
//!   cargo test -p atlas-ask --test live -- --ignored --nocapture
//! Loads KISSKI_API_KEY from the main checkout's .env; caches replies under data/cache/ask/llm.

mod common;

use std::sync::Arc;

use atlas_ask::{AskRequest, Asker};
use atlas_llm::{Cache, CacheMode, Llm, Registry};

#[tokio::test]
#[ignore = "live: calls KISSKI"]
async fn stxbp1_english_and_german() {
    let Some((atlas, graph)) = common::real() else { return };
    let data = common::data_dir();
    let _ = dotenvy::from_path(data.join("..").join(".env"));
    if std::env::var("KISSKI_API_KEY").is_err() {
        eprintln!("skipping: KISSKI_API_KEY not set");
        return;
    }
    let llm = Llm::new(
        Registry::presets(false).unwrap(),
        Cache::new(data.join("cache").join("ask").join("llm"), CacheMode::ReadWrite),
    );
    let asker = Asker::new(atlas, graph, Arc::new(llm));
    let mut used = 0;
    for (lang, q) in [
        (
            "en",
            "My child has an STXBP1 variant. Is there a patient group or a study we could join?",
        ),
        (
            "de",
            "Meine Tochter hat eine STXBP1-Mutation. Gibt es Studien, an denen wir teilnehmen können?",
        ),
    ] {
        if used + 3 > 5 {
            eprintln!("budget: skipping {lang} (5-call limit)");
            break;
        }
        let req = AskRequest {
            question: q.into(),
            lang: lang.into(),
            connection: Some("kisski".into()),
            ..AskRequest::default()
        };
        let out = asker.ask(&req, None).await;
        used += out.calls.iter().filter(|c| !c.cached).count();
        eprintln!("\n=== {lang}: {q}");
        for c in &out.chips {
            eprintln!(
                "chip {} {} {:?} → {:?} {:?}",
                c.id,
                c.intent.as_str(),
                c.slots,
                c.resolved.values().map(|r| &r.node.id).collect::<Vec<_>>(),
                c.status
            );
        }
        for s in &out.answer {
            eprintln!("  [{:?}] {} {:?}", s.kind, s.text, s.cites);
        }
        eprintln!(
            "origin {:?}, validator {:?}, calls {:?}",
            out.origin,
            out.validation,
            out.calls
                .iter()
                .map(|c| (c.purpose, c.cached, c.latency_ms))
                .collect::<Vec<_>>()
        );
        assert!(!out.answer.is_empty());
        for s in out
            .answer
            .iter()
            .filter(|s| s.kind == atlas_llm::tasks::SentenceKind::Fact)
        {
            assert!(s.cites.iter().all(|k| out.facts.iter().any(|f| &f.key == k)));
        }
    }
    eprintln!("uncached model calls: {used}");
    assert!(used <= 5);
}
