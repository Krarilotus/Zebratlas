//! Offline replay against a private Python-generated checksum manifest.
//! Reports filenames/counts only; never prints records or changes source bytes.
use std::path::PathBuf;

use atlas_core::graph::RecordHash;
use atlas_core::provenance::Locator;
use atlas_ingest::graph::cache;
use serde::Deserialize;

#[derive(Deserialize)]
struct Expected {
    file: String,
    file_sha256: String,
    header_sha256: String,
    records_sha256: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("model-cache directory required")?);
    let manifest = PathBuf::from(args.next().ok_or("private expected-checksum manifest required")?);
    if args.next().is_some() {
        return Err("expected exactly two arguments".into());
    }
    let expected: Vec<Expected> = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let mut checked = 0;
    for item in &expected {
        if item.file.contains(['/', '\\', ':']) || matches!(item.file.as_str(), "" | "." | "..") {
            return Err("expected manifest filename must be a basename".into());
        }
        let path = root.join(&item.file);
        if cache::hex(&cache::sha256(&std::fs::read(&path)?)) != item.file_sha256 {
            return Err("source bytes changed after the Python replay".into());
        }
        let envelope = cache::read_envelope(&path, "", &[1])?;
        if !envelope.header_verified
            || envelope.header_str("sha256") != Some(item.header_sha256.as_str())
            || envelope.records.len() != item.records_sha256.len()
        {
            return Err("source envelope header/count mismatch".into());
        }
        for (i, hash) in item.records_sha256.iter().enumerate() {
            let check = cache::rehash(
                &path,
                &Locator::Record(format!("records[{i}]")),
                RecordHash::CanonicalJson,
            );
            if check.error.is_some() || check.computed_sha256.as_ref() != Some(hash) {
                return Err("current reader record rehash mismatch".into());
            }
        }
        checked += item.records_sha256.len();
        println!(
            "{}: header=true; records={}/{}",
            item.file,
            item.records_sha256.len(),
            item.records_sha256.len()
        );
    }
    println!("verified_headers={}; verified_records={checked}", expected.len());
    Ok(())
}
