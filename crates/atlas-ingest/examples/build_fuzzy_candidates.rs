//! Offline private cache, never a public release or graph mutation. Prints counts only.
use atlas_core::{
    fuzzy_search::{CompactFuzzyIndex, FuzzyBuilder},
    graph::RecordWithhold,
    node::{NodeKey, NodeKind},
    Atlas, Graph,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

fn hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn reference(atlas: &Atlas, graph: &Graph, key: NodeKey) -> atlas_core::node::NodeRef {
    match key.kind {
        NodeKind::Disease | NodeKind::Gene | NodeKind::Phenotype => atlas.node_ref(key),
        _ => graph.node_ref(key),
    }
}
fn hidden(value: &str, blocked: &HashSet<String>) -> bool {
    blocked.contains(value) || (value.contains('|') && value.split('|').any(|part| blocked.contains(part)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("Usage: build_fuzzy_candidates DATA ATLAS_SNAPSHOT GRAPH_SNAPSHOT NEW_OUTPUT_DIR".into());
    }
    let data = PathBuf::from(&args[0]);
    let atlas_path = PathBuf::from(&args[1]);
    let graph_path = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);
    if output.exists() {
        return Err("Output must be a new isolated directory; existing artifacts are never overwritten".into());
    }
    let started = Instant::now();
    let atlas_sha = hash(&atlas_path)?;
    let graph_sha = hash(&graph_path)?;
    let salt = atlas_ingest::withhold::salt_from_env();
    let policy_signature = atlas_ingest::withhold::signature(&data, &salt)?;
    let policy = atlas_ingest::withhold::load(&data, salt)?;
    let (atlas, _) = atlas_core::snapshot::load(&atlas_path)?;
    let (mut graph, _) = atlas_core::snapshot::load_graph(&graph_path)?;
    let mut blocked = HashSet::new();
    let mut excluded_nodes = 0;
    for kind in [
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Study,
        NodeKind::Paper,
        NodeKind::Grant,
        NodeKind::Asset,
    ] {
        for idx in 0..graph.node_count(kind) as u32 {
            let key = NodeKey { kind, idx };
            if policy.node(&graph, key).is_some() || graph.node_withheld(key).is_some() {
                excluded_nodes += 1;
                let node = graph.node_ref(key);
                blocked.insert(node.id);
                if kind == NodeKind::Person {
                    let person = graph.person(idx);
                    blocked.insert(person.name.clone());
                    blocked.extend(person.name_variants.iter().cloned());
                    blocked.extend(person.orcids.iter().cloned());
                }
            }
        }
    }
    for contacts in &graph.data().contacts {
        for contact in &contacts.central {
            if policy.contact(&contact.name, "", contact.email.as_deref()).is_some() {
                blocked.insert(contact.name.clone());
                blocked.extend(contact.email.clone());
            }
        }
        for contact in &contacts.officials {
            if policy.contact(&contact.name, &contact.affiliation, None).is_some() {
                blocked.insert(contact.name.clone());
            }
        }
    }
    graph.set_withhold(policy);
    let visible = |key: NodeKey| {
        let node = reference(&atlas, &graph, key);
        !hidden(&node.id, &blocked)
            && !hidden(&node.label, &blocked)
            && graph
                .node(&node.id)
                .is_none_or(|graph_key| graph.node_withheld(graph_key).is_none())
    };
    let mut builder = FuzzyBuilder::default();
    let mut labels = 0;
    let mut add = |key, text: &str| -> Result<(), String> {
        if visible(key) && !hidden(text, &blocked) {
            builder.add(key, text)?;
            labels += 1;
        }
        Ok(())
    };
    for (idx, disease) in atlas
        .diseases()
        .iter()
        .enumerate()
        .filter(|(_, disease)| disease.is_active())
    {
        let key = NodeKey {
            kind: NodeKind::Disease,
            idx: idx as u32,
        };
        add(key, &disease.name)?;
        for name in &disease.synonyms {
            add(key, &name.text)?;
        }
    }
    for (idx, gene) in atlas.genes().iter().enumerate() {
        let key = NodeKey {
            kind: NodeKind::Gene,
            idx: idx as u32,
        };
        add(key, &gene.symbol)?;
        if let Some(alias) = graph
            .gene_alias(&gene.symbol)
            .filter(|alias| graph.records_withheld(&[alias.record]).is_none())
        {
            add(key, &alias.name)?;
            for name in alias.aliases.iter().chain(&alias.previous) {
                add(key, name)?;
            }
        }
    }
    for (idx, term) in atlas.hpo.terms().iter().enumerate().filter(|(_, term)| !term.obsolete) {
        let key = NodeKey {
            kind: NodeKind::Phenotype,
            idx: idx as u32,
        };
        add(key, &term.name)?;
        for synonym in &term.synonyms {
            add(key, &synonym.text)?;
        }
    }
    for kind in [
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Study,
        NodeKind::Paper,
        NodeKind::Grant,
        NodeKind::Asset,
    ] {
        for idx in 0..graph.node_count(kind) as u32 {
            let key = NodeKey { kind, idx };
            let node = graph.node_ref(key);
            add(key, &node.label)?;
            if kind == NodeKind::Person {
                for name in &graph.person(idx).name_variants {
                    add(key, name)?;
                }
            }
            if kind == NodeKind::Asset {
                for (field, value) in &graph.asset(idx).facts {
                    if matches!(
                        field.as_str(),
                        "synonyms" | "aliases" | "alternative_names" | "drug_name" | "brand_name"
                    ) && value.len() <= 512
                    {
                        add(key, value)?;
                    }
                }
            }
        }
    }
    let index = builder.finish()?;
    let build_ms = started.elapsed().as_millis();
    let stats = index.stats();
    let bytes = index.encode()?;
    if bytes.len() > atlas_core::fuzzy_search::MAX_INDEX_BYTES {
        return Err("Serialized cache exceeds40MiB".into());
    }
    let decoded = CompactFuzzyIndex::decode(&bytes)?;
    let mut checks = Vec::new();
    for (query, expected) in [
        ("stxbpi", Some("HGNC:11444")),
        ("MUNC18-1", Some("HGNC:11444")),
        ("seizures", Some("HP:0001250")),
        ("encefalopathy", None),
        ("delayed development", None),
        ("Niemann Pick C", None),
        ("protein silencing", None),
        ("zqxvplkj astronaut dishwasher pineapple", None),
    ] {
        let time = Instant::now();
        let hits = decoded.search(query, 10, &visible);
        let elapsed_us = time.elapsed().as_micros();
        let expected_found = expected.map(|id| {
            hits.candidates
                .iter()
                .any(|hit| reference(&atlas, &graph, hit.node).id == id)
        });
        checks.push(json!({"query":query,"count":hits.candidates.len(),"expected_found":expected_found,"elapsed_us":elapsed_us,"visited":hits.visited,"truncated":hits.truncated}));
    }
    if checks.iter().any(|check| check["expected_found"] == false)
        || checks.last().is_some_and(|check| check["count"] != 0)
    {
        return Err(format!("Candidate smoke checks failed: {}", serde_json::to_string(&checks)?).into());
    }
    if atlas_sha != hash(&atlas_path)?
        || graph_sha != hash(&graph_path)?
        || policy_signature != atlas_ingest::withhold::signature(&data, &atlas_ingest::withhold::salt_from_env())?
    {
        return Err("Snapshot or withholding policy changed during build; cache not admitted".into());
    }
    std::fs::create_dir_all(&output)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("index.bin"))?;
    file.write_all(&bytes)?;
    file.flush()?;
    let manifest = json!({"format":"zebratlas-lexical-candidates-v1","method":"bm25_trigram","scope":"name_candidates_only","public_release":false,"visibility":"private-live-withholding","atlas_sha256":atlas_sha,"graph_sha256":graph_sha,"withhold_sha256":format!("{:x}",Sha256::digest(policy_signature.as_bytes())),"index_sha256":format!("{:x}",Sha256::digest(&bytes)),"labels_seen":labels,"excluded_nodes":excluded_nodes,"stats":stats,"serialized_bytes":bytes.len(),"build_ms":build_ms,"checks":checks,"cutoff":{"minimum_query_vocabulary_coverage":0.4,"minimum_document_word_coverage":0.45,"minimum_bm25":0.6},"runtime_visibility_required":true});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("manifest.json"))?;
    file.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
