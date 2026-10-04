//! Opt-in hosted smoke over the public, source-verified STXBP1 gene in a supplied release.
//! No private input or credentials are printed. Run only against an owned read-only endpoint.
use atlas_ask::query::{LinkedEntity, QueryEngine, SchemaCard, conversation::QueryRequest};
use atlas_llm::{Cache, CacheMode, Llm, Registry};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("query_smoke CARD ENDPOINT OUTPUT".into());
    }
    let card: SchemaCard = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let engine = QueryEngine::new(card, &args[2], None)?;
    let llm = Llm::new(
        Registry::presets(false)?.for_query_planning()?,
        Cache::new(std::env::temp_dir(), CacheMode::Off),
    );
    let req = QueryRequest {
        question: "Show the source-backed gene record for STXBP1.".into(),
        linked: vec![LinkedEntity {
            id: "HGNC:11444".into(),
            label: "STXBP1".into(),
        }],
        lang: Some("en".into()),
        ..Default::default()
    };
    match engine.understand(&llm, &req, None).await {
        Ok(answer) => {
            std::fs::write(&args[3], serde_json::to_vec_pretty(&answer)?)?;
            println!(
                "{}",
                json!({"status":"executed", "results":answer.results.len(), "latency_ms":answer.latency_ms})
            );
        }
        Err(error) => {
            // This smoke has fixed public input; never use this diagnostic output for private text.
            std::fs::write(
                &args[3],
                serde_json::to_vec_pretty(&json!({"status":"failed","diagnostic":error}))?,
            )?;
            return Err(error.into());
        }
    }
    Ok(())
}
