//! JSONL bridge used by the preregistered evaluation. No files or network access.
use atlas_analytics::ranker_v3::{FusionParams, fuse};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
struct Request {
    atlas: BTreeMap<String, f64>,
    resnik: BTreeMap<String, f64>,
    params: Vec<FusionParams>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let req: Request = serde_json::from_str(&line?)?;
        let mut values = Vec::new();
        for p in req.params {
            let scores = fuse(&req.atlas, &req.resnik, p)?;
            let mut hash = Sha256::new();
            for score in scores {
                hash.update(score.score.to_le_bytes());
            }
            values.push(format!("{:x}", hash.finalize()));
        }
        serde_json::to_writer(&mut out, &values)?;
        writeln!(out)?;
        out.flush()?;
    }
    Ok(())
}
