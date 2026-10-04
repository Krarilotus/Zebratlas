use anyhow::{Context, Result, bail};
use atlas_discovery::{Bundle, ReviewDecision, build, load, validate_review, write_bundle};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("candidates") if args.len() == 4 => {
            // Verify and stage accepted source proposals from atlas-contrib; never imports anything.
            let bytes = std::fs::read(&args[2])?;
            let q = atlas_discovery::candidates::stage(&bytes, &args[2], &atlas_discovery::align::now_utc())?;
            std::fs::write(&args[3], serde_json::to_vec_pretty(&q)?)?;
            for p in &q.staged {
                println!("{} {} {} {:?}", p.id, p.stage, p.url, p.blocked_reasons);
            }
        }
        Some("align") if args.get(2).map(String::as_str) == Some("annotate") && args.len() == 5 => {
            let report = atlas_discovery::align::annotate::directory(Path::new(&args[3]), Path::new(&args[4]))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("align") if args.get(2).map(String::as_str) == Some("gate") && (args.len() == 5 || args.len() == 6) => {
            let report = atlas_discovery::align::safety::directory(
                Path::new(&args[3]),
                Path::new(&args[4]),
                args.get(5).map(Path::new),
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("align") => {
            let data = std::env::var("RARE_ATLAS_DATA").context("set RARE_ATLAS_DATA to the data directory")?;
            let summaries = atlas_discovery::align::run(Path::new(&data), &args[2..])?;
            println!("{}", serde_json::to_string_pretty(&summaries)?);
        }
        Some("evaluate") if args.len() == 5 => {
            let b: Bundle = serde_json::from_slice(&std::fs::read(&args[2])?)?;
            let raw = std::fs::read(&args[3])?;
            let a = serde_json::from_slice(&raw)?;
            let result = atlas_discovery::evaluation::evaluate(&b, &a, &raw)?;
            let text = serde_json::to_string_pretty(&result)?;
            std::fs::write(&args[4], &text)?;
            println!("{text}");
        }
        Some("run") if args.len() == 4 => {
            let start = std::time::Instant::now();
            let (manifest, data, hash) = load(Path::new(&args[2]))?;
            let bundle = build(manifest, data, hash)?;
            write_bundle(&bundle, Path::new(&args[3]))?;
            println!("{}", serde_json::to_string_pretty(&bundle.metrics)?);
            println!(
                "review items: {}; elapsed_ms: {}",
                bundle.review_queue.len(),
                start.elapsed().as_millis()
            );
        }
        Some("review") if args.len() == 5 => {
            let bytes = std::fs::read(&args[2])?;
            let b: Bundle = serde_json::from_slice(&bytes)?;
            let d: ReviewDecision = serde_json::from_slice(&std::fs::read(&args[3])?)?;
            validate_review(&b, &d, &bytes)?;
            // create_new prevents replacing a prior decision; separate file per decision.
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[4])
                .context("review audit file must not already exist")?;
            file.write_all(&serde_json::to_vec_pretty(&d)?)?;
            println!("Recorded review; original bundle and identities unchanged.");
        }
        _ => bail!(
            "usage: atlas-discovery candidates QUEUE.json STAGED.json | align gate INPUT_DIR NEW_OUTPUT_DIR [REVOCATIONS.json] | align annotate INPUT_DIR OUTPUT_DIR | align [disease|gene|trial|org|work|drug|researchers|gard|rxnorm|affiliation|all]... | run MANIFEST OUTPUT_DIR | evaluate BUNDLE AUDIT OUTPUT | review BUNDLE DECISION NEW_AUDIT_FILE"
        ),
    }
    Ok(())
}
