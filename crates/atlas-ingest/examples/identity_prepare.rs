//! Verify the shared gate and bind a bounded inference input to its accepted closure.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::Path,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: identity_prepare GATE_DIR ASSERTIONS.jsonl NEW_RECEIPT.json".into());
    }
    let gate = atlas_ingest::identity_projection::load_directory(Path::new(&args[1]))?;
    if gate.accepted.manifest_sha256.is_empty() {
        return Err("complete gate required".into());
    }
    let input = Path::new(&args[2]);
    let mut representatives = BTreeMap::new();
    for line in BufReader::new(std::fs::File::open(input)?).lines() {
        let row: Value = serde_json::from_str(&line?)?;
        for key in ["subject", "object"] {
            if let Some(id) = row[key].as_str() {
                representatives.insert(id.to_owned(), gate.accepted.representative(id));
            }
        }
    }
    let bounded: std::collections::HashSet<_> = representatives.values().cloned().collect();
    let mut component_decisions = BTreeMap::<String, Vec<String>>::new();
    for (decision, (subject, _)) in &gate.pairs {
        let root = gate.accepted.representative(subject);
        if bounded.contains(&root) {
            component_decisions.entry(root).or_default().push(decision.clone());
        }
    }
    for decisions in component_decisions.values_mut() {
        decisions.sort();
    }
    let receipt = json!({"schema":"atlas.inference.identity-input", "version":1,
        "gate_directory":std::fs::canonicalize(&args[1])?,
        "gate_manifest_sha256":gate.accepted.manifest_sha256,
        "identity_policy":atlas_core::identity_policy::RULE,
        "identity_policy_sha256":atlas_core::identity_policy::code_sha256(),
        "assertions_sha256":atlas_ingest::sources::sha256(input)?,
        "representatives":representatives, "component_decisions":component_decisions,
        "restriction":"private build input; no public release authorization; no owl:sameAs entailment"});
    use std::io::Write;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    out.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "verified {} bounded identifiers; {} accepted decisions",
        representatives.len(),
        gate.decisions.len()
    );
    Ok(())
}
