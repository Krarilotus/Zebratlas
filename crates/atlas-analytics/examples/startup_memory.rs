//! Read-only phase attribution for existing checksum-pinned operational snapshots.
use atlas_analytics::{Matcher, ScoringParams, SimilarityIndex, SimilarityParams};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command, sync::Arc, time::Instant};

fn memory(phase: &str, began: Instant) {
    let memory = if cfg!(target_os = "windows") {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {}).PrivateMemorySize64", std::process::id()),
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    } else {
        std::fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("VmRSS:") || l.starts_with("VmHWM:") || l.starts_with("RssAnon:"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    println!("{phase}: elapsed={:.1}s memory={memory}", began.elapsed().as_secs_f64());
}

fn pinned(
    manifest: &serde_json::Value,
    root: &Path,
    file: &str,
    hash: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use std::io::Read;
    let path = root.join(Path::new(manifest[file].as_str().ok_or("missing path")?).strip_prefix("/data/")?);
    let mut input = std::fs::File::open(&path)?;
    let mut digest = Sha256::new();
    let mut block = [0u8; 65536];
    loop {
        let n = input.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
    }
    if format!("{:x}", digest.finalize()) != manifest[hash].as_str().ok_or("missing hash")? {
        return Err("snapshot checksum mismatch".into());
    }
    Ok(path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("usage: startup_memory PRIVATE_DATA_DIR")?;
    let root = Path::new(&root);
    let manifest: serde_json::Value = serde_json::from_reader(std::fs::File::open(
        root.join("cache/runtime/operational.manifest.json"),
    )?)?;
    if manifest["public_release"] != false {
        return Err("expected private operational manifest".into());
    }
    let began = Instant::now();
    let identity = atlas_ingest::identity_projection::load(root)?;
    if manifest["identity_gate_sha256"].as_str() != Some(identity.accepted.manifest_sha256.as_str()) {
        return Err("operational identity gate mismatch".into());
    }
    memory("identity-guard", began);
    drop(identity);
    let atlas = Arc::new(atlas_core::snapshot::load(&pinned(&manifest, root, "atlas_snapshot", "atlas_sha256")?)?.0);
    memory("atlas", began);
    let graph = atlas_core::snapshot::load_graph(&pinned(&manifest, root, "graph_snapshot", "graph_sha256")?)?.0;
    memory("graph", began);
    let matcher = Matcher::new(atlas.clone(), ScoringParams::default());
    memory("matcher", began);
    let search = atlas_core::search::domain::Index::build(&atlas, &graph);
    memory("search", began);
    let suggestions = atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph);
    memory("suggestions", began);
    println!("suggestion-sha256={}", suggestions.sha256);
    let mechanism =
        Arc::new(atlas_ingest::mechanism::load(&pinned(&manifest, root, "mechanism_snapshot", "mechanism_sha256")?)?.0);
    memory("mechanism", began);
    let similarity = SimilarityIndex::new(atlas, mechanism, SimilarityParams::default());
    memory("similarity", began);
    let slice = atlas_analytics::dee_slice(&similarity);
    let clusters = atlas_analytics::clusters(&similarity, &slice, &atlas_analytics::ClusterParams::default());
    memory("clusters", began);
    std::hint::black_box((&matcher, &search, &suggestions, &similarity, &clusters));
    drop(suggestions);
    memory("drop-suggestions", began);
    drop(search);
    memory("drop-search", began);
    Ok(())
}
