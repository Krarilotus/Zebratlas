//! A pinned lexical artifact offers choices, never an interpreted scientific answer.
use crate::{explore_stats::DurableScanCache, routes::AppState};
use atlas_core::graph::RecordWithhold;
use atlas_core::{
    fuzzy_search::CompactFuzzyIndex,
    node::{NodeKey, NodeKind, NodeRef},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

struct Loaded {
    index: CompactFuzzyIndex,
    hash: String,
    atlas_identity: usize,
    graph_identity: usize,
}

fn reference(state: &AppState, key: NodeKey) -> Option<NodeRef> {
    let bound = match key.kind {
        NodeKind::Disease => state.atlas.diseases().len(),
        NodeKind::Gene => state.atlas.genes().len(),
        NodeKind::Phenotype => state.atlas.parts().0.len(),
        other => state.graph.node_count(other),
    };
    if key.idx as usize >= bound {
        return None;
    }
    Some(match key.kind {
        NodeKind::Disease | NodeKind::Gene | NodeKind::Phenotype => state.atlas.node_ref(key),
        _ => state.graph.node_ref(key),
    })
}

fn file_hash(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|_| "Candidate source unavailable")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "Candidate source unreadable")?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn load(state: &AppState, manifest_path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(manifest_path).map_err(|_| "Candidate manifest unavailable")?;
    if bytes.len() > 64 * 1024 {
        return Err("Candidate manifest exceeds limit".into());
    }
    let manifest: Value = serde_json::from_slice(&bytes).map_err(|_| "Candidate manifest invalid")?;
    if manifest["format"] != "zebratlas-lexical-candidates-v1"
        || manifest["scope"] != "name_candidates_only"
        || manifest["runtime_visibility_required"] != true
        || manifest["public_release"] != false
    {
        return Err("Candidate manifest is not admitted".into());
    }
    for (env, field) in [
        ("RARE_ATLAS_ATLAS_SNAPSHOT", "atlas_sha256"),
        ("RARE_ATLAS_GRAPH_SNAPSHOT", "graph_sha256"),
    ] {
        let path = std::env::var_os(env)
            .map(PathBuf::from)
            .ok_or("Candidate snapshots are not pinned")?;
        if manifest[field].as_str() != Some(file_hash(&path)?.as_str()) {
            return Err("Candidate source identity mismatch".into());
        }
    }
    let policy = atlas_ingest::withhold::signature(&state.data, &atlas_ingest::withhold::salt_from_env())
        .map_err(|_| "Candidate policy unavailable")?;
    let policy_hash = format!("{:x}", Sha256::digest(policy.as_bytes()));
    if manifest["withhold_sha256"].as_str() != Some(&policy_hash) {
        return Err("Candidate policy identity mismatch".into());
    }
    let index_path = manifest_path
        .parent()
        .ok_or("Candidate directory missing")?
        .join("index.bin");
    if std::fs::metadata(&index_path)
        .map_err(|_| "Candidate artifact unavailable")?
        .len()
        > atlas_core::fuzzy_search::MAX_INDEX_BYTES as u64
    {
        return Err("Candidate artifact exceeds limit".into());
    }
    let bytes = std::fs::read(index_path).map_err(|_| "Candidate artifact unreadable")?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if manifest["index_sha256"].as_str() != Some(&hash) {
        return Err("Candidate artifact identity mismatch".into());
    }
    Ok(Loaded {
        index: CompactFuzzyIndex::decode(&bytes)?,
        hash,
        atlas_identity: Arc::as_ptr(&state.atlas) as usize,
        graph_identity: Arc::as_ptr(&state.graph) as usize,
    })
}

pub async fn search(
    state: &AppState,
    query: &str,
    limit: usize,
) -> (Vec<Value>, Option<String>, bool, Option<&'static str>) {
    let Some(path) = std::env::var_os("ATLAS_FUZZY_MANIFEST").map(PathBuf::from) else {
        return (vec![], None, false, Some("not_configured"));
    };
    static CACHE: OnceLock<DurableScanCache<Loaded>> = OnceLock::new();
    let owned = state.clone();
    let loaded = match CACHE
        .get_or_init(|| DurableScanCache::new(Duration::from_secs(60)))
        .get_or_try_init(move || load(&owned, &path))
        .await
    {
        Ok(loaded) => loaded,
        Err(_) => return (vec![], None, false, Some("index_unavailable")),
    };
    if loaded.atlas_identity != Arc::as_ptr(&state.atlas) as usize
        || loaded.graph_identity != Arc::as_ptr(&state.graph) as usize
    {
        return (vec![], None, false, Some("source_identity_changed"));
    }
    let state = state.clone();
    let query = query.to_owned();
    tokio::task::spawn_blocking(move || {
        state.withhold.with_query_visibility(|allowed| {
            let result = loaded.index.search(&query, limit.min(10), |key| reference(&state, key).is_some_and(|node|
                allowed(&node.id) && allowed(&node.label) && state.graph.node(&node.id).is_none_or(|key| state.graph.node_withheld(key).is_none())));
            let matches = result.candidates.into_iter().filter_map(|candidate| {
                let node = reference(&state, candidate.node)?;
                Some(json!({"id":node.id,"label":node.label,"kind":node.kind,"match":"fuzzy","method":"lexical","score":candidate.score}))
            }).collect();
            (matches, Some(loaded.hash.clone()), result.truncated, None)
        })
    }).await.unwrap_or((vec![], None, false, Some("index_unavailable")))
}
