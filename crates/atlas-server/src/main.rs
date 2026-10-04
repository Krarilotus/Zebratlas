//! Rare Disease Atlas: HTTP API and CLI.
//!
//!   atlas-server [serve] [--addr 127.0.0.1:8000] [--data DIR]   load (or build) the snapshot, serve /api
//!   atlas-server build [--data DIR]                              rebuild the atlas and graph snapshots
//!   atlas-server trace --orcid X | --name N [--affiliation A] | --email E | --node ID [--show]
//!                                                                everything derived from one person (D43)

mod account_wiring;
mod bridges;
mod cards;
mod checks;
mod clinvar;
mod codes;
mod connections;
mod connector;
mod copy;
mod copy_d30;
mod copy_extra;
mod copy_jobs;
mod copy_related;
mod coverage;
mod drugs;
mod explore;
mod explore_candidates;
mod explore_document;
mod explore_overview;
mod explore_failover;
mod explore_projection;
mod explore_query;
mod explore_reasoning;
mod explore_sparql;
mod explore_stats;
mod export;
mod find;
mod funding_evidence;
mod initiatives;
#[cfg(test)]
mod initiatives_tests;
mod intake;
mod jobs;
mod journeys;
#[cfg(test)]
mod language_contract;
mod llm;
mod models;
mod nodes;
mod privacy;
mod prov;
mod query_execution;
mod query_graph;
mod questions;
mod resolve;
mod routes;
mod runtime_snapshots;
mod search;
#[cfg(test)]
mod test_support;
mod understand;
mod units;
mod views;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use atlas_core::{Atlas, Graph};

use anyhow::Context;
use atlas_analytics::{Matcher, ScoringParams};
use atlas_ingest::Origin;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Rare Disease Atlas API")]
struct Cli {
    /// Data root with raw/ and cache/ (default: $RARE_ATLAS_DATA or the nearest ./data).
    #[arg(long, global = true, env = "RARE_ATLAS_DATA")]
    data: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the JSON API (default).
    Serve {
        #[arg(long, default_value = "127.0.0.1:8000", env = "ATLAS_ADDR")]
        addr: String,
        /// Rebuild the snapshot before serving.
        #[arg(long)]
        rebuild: bool,
        /// Load existing snapshots without rebuilding or writing shared source caches.
        #[arg(long, conflicts_with = "rebuild")]
        read_only_snapshots: bool,
    },
    /// Rebuild both snapshots from data/raw and data/cache; print stats and the integrity summary.
    Build,
    /// Write a private operational RDF adapter from explicit immutable snapshots.
    OperationalProjection {
        #[arg(long)]
        atlas_snapshot: PathBuf,
        #[arg(long)]
        graph_snapshot: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Provenance trace of one person (D43): nodes, edges, cache records, release files.
    /// Read-only. Prints counts; `--show` prints the details (personal data: reviewers only).
    Trace {
        #[arg(long)]
        orcid: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        affiliation: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        node: Option<String>,
        /// Graph snapshot to read (default: <data>/cache/graph.snapshot).
        #[arg(long)]
        graph_snapshot: Option<PathBuf>,
        #[arg(long)]
        show: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data = cli.data.unwrap_or_else(atlas_ingest::data_dir);
    match cli.command.unwrap_or(Command::Serve {
        addr: "127.0.0.1:8000".into(),
        rebuild: false,
        read_only_snapshots: false,
    }) {
        Command::OperationalProjection {
            atlas_snapshot,
            graph_snapshot,
            output,
        } => {
            use sha2::{Digest, Sha256};
            fn hash(path: &Path) -> anyhow::Result<String> {
                use std::io::Read;
                let mut file = std::fs::File::open(path)?;
                let mut hash = Sha256::new();
                let mut buffer = vec![0u8; 1024 * 1024];
                loop {
                    let n = file.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
                Ok(format!("{:x}", hash.finalize()))
            }
            let started = Instant::now();
            let mapping_directory = atlas_ingest::identity_projection::directory(&data);
            let gate_path = mapping_directory.join("identity-gate.manifest.json");
            atlas_ingest::identity_projection::load(&data)?;
            let gate_hash = gate_path.exists().then(|| hash(&gate_path)).transpose()?;
            let atlas_hash = hash(&atlas_snapshot)?;
            let graph_hash = hash(&graph_snapshot)?;
            let (atlas, _) = atlas_core::snapshot::load(&atlas_snapshot)?;
            let (mut graph, _) = atlas_core::snapshot::load_graph(&graph_snapshot)?;
            graph.set_withhold(atlas_ingest::withhold::load_or_closed(
                &data,
                atlas_ingest::withhold::salt_from_env(),
            ));
            let report = explore_projection::write(&atlas, &graph, &output)?;
            anyhow::ensure!(
                atlas_hash == hash(&atlas_snapshot)? && graph_hash == hash(&graph_snapshot)?,
                "Source snapshots changed during projection; do not activate this RDF"
            );
            let manifest = serde_json::json!({
                "format": "zebratlas-operational-rdf-v1", "visibility":"private-live-withholding", "public_release":false,
                "ontology":"https://w3id.org/rare-disease-atlas/vocab#", "ids":"https://w3id.org/rare-disease-atlas/id/",
                "atlas_snapshot":atlas_snapshot,"atlas_sha256":atlas_hash,"graph_snapshot":graph_snapshot,"graph_sha256":graph_hash,
                "rdf":output,"rdf_sha256":hash(&output)?,"runtime_reasoning":false,"report":report,
                "mapping_directory":mapping_directory,"identity_gate_sha256":gate_hash,
                "elapsed_ms":started.elapsed().as_millis() as u64
            });
            let manifest_path = output.with_extension("manifest.json");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&manifest_path)?;
            use std::io::Write;
            file.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        Command::Build => {
            let (atlas, graph) = load(&data, true)?;
            let report = atlas_core::integrity::check(&atlas, &graph);
            let contracts: Vec<_> = report.contracts.iter().map(|c| (c.id, c.violations)).collect();
            println!("{}", serde_json::to_string_pretty(&atlas.stats())?);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "integrity_passed": report.passed, "contracts": contracts, "nodes": report.nodes,
                    "edges": report.edges_by_relation,
                }))?
            );
            Ok(())
        }
        Command::Trace {
            orcid,
            name,
            affiliation,
            email,
            node,
            graph_snapshot,
            show,
        } => {
            let q = atlas_ingest::trace::TraceQuery {
                orcid,
                name,
                affiliation,
                email,
                node,
            };
            privacy::trace_cli(&data, graph_snapshot, q, show)
        }
        Command::Serve {
            addr,
            rebuild,
            read_only_snapshots,
        } => {
            let (atlas, graph) = if read_only_snapshots {
                let (atlas, _) = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))?;
                let (mut graph, _) = atlas_core::snapshot::load_graph(&atlas_ingest::graph::snapshot_path(&data))?;
                graph.set_withhold(atlas_ingest::withhold::load_or_closed(
                    &data,
                    atlas_ingest::withhold::salt_from_env(),
                ));
                (atlas, graph)
            } else {
                load(&data, rebuild)?
            };
            let atlas = Arc::new(atlas);
            let matcher = Matcher::new(atlas.clone(), ScoringParams::default());
            let related = Arc::new(OnceLock::new());
            let clusters = Arc::new(OnceLock::new());
            spawn_related(
                data.clone(),
                atlas.clone(),
                related.clone(),
                clusters.clone(),
                read_only_snapshots,
            );
            {
                // Warm the ICD code index (typed links from Orphanet + MONDO) off the request path.
                let (a, d) = (atlas.clone(), data.clone());
                std::thread::spawn(move || {
                    codes::load(&a, &d);
                });
            }
            let graph = Arc::new(graph);
            let search = Arc::new(atlas_core::search::domain::Index::build(&atlas, &graph));
            let query_suggestions = Arc::new(atlas_core::query_graph::SuggestionIndex::new(&atlas, &graph));
            let state = routes::AppState {
                atlas: atlas.clone(),
                matcher: Arc::new(matcher),
                withhold: privacy::WithholdState::new(data.clone(), graph.clone()),
                graph,
                search,
                query_suggestions,
                data: Arc::new(data),
                llm: llm::LlmState::from_env(),
                integrity: Arc::new(OnceLock::new()),
                related,
                questions: Arc::new(questions::QuestionCache::default()),
                clusters,
                units: Arc::new(OnceLock::new()),
                query_engine: Arc::new(OnceLock::new()),
                explore_index: Arc::new(OnceLock::new()),
            };
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
            runtime.block_on(routes::serve(&addr, state))
        }
    }
}

/// Build the mechanism similarity index (mechanism agent's layer) on a background thread.
fn spawn_related(
    data: PathBuf,
    atlas: Arc<Atlas>,
    slot: Arc<OnceLock<Result<atlas_analytics::SimilarityIndex, String>>>,
    clusters: Arc<OnceLock<atlas_analytics::ClusterReport>>,
    read_only_snapshots: bool,
) {
    std::thread::spawn(move || {
        let t = Instant::now();
        let built = (if std::env::var_os("ATLAS_OPERATIONAL_MANIFEST").is_some() {
            runtime_snapshots::load_mechanism(&data).map(|m| (m, Origin::Snapshot))
                .map_err(|e| e.to_string())
        } else if read_only_snapshots {
            atlas_ingest::mechanism::load(&atlas_ingest::mechanism::snapshot_path(&data))
                .map(|(m, _)| (m, Origin::Snapshot)).map_err(|e| e.to_string())
        } else {
            atlas_ingest::mechanism::load_or_build(&data, &atlas, false).map_err(|e| e.to_string())
        })
        .map(|(m, _)| {
            atlas_analytics::SimilarityIndex::new(atlas, Arc::new(m), atlas_analytics::SimilarityParams::default())
        });
        match &built {
            Ok(_) => eprintln!("related: similarity index ready ({:.1}s)", t.elapsed().as_secs_f64()),
            Err(e) => eprintln!("related: unavailable: {e}"),
        }
        let _ = slot.set(built);
        if let Some(Ok(index)) = slot.get() {
            let t = Instant::now();
            let slice = atlas_analytics::dee_slice(index);
            let report = atlas_analytics::clusters(index, &slice, &atlas_analytics::ClusterParams::default());
            eprintln!(
                "related: {} clusters of the DEE slice ({:.1}s)",
                report.clusters.len(),
                t.elapsed().as_secs_f64()
            );
            let _ = clusters.set(report);
        }
    });
}

fn origin(o: Origin) -> &'static str {
    match o {
        Origin::Snapshot => "loaded snapshot",
        Origin::Built => "built and saved snapshot",
    }
}

/// Atlas and graph; a fresh graph snapshot loads on a second thread while the atlas loads.
fn load(data: &Path, rebuild: bool) -> anyhow::Result<(Atlas, Graph)> {
    if let Some(path) = std::env::var_os("ATLAS_OPERATIONAL_MANIFEST") {
        anyhow::ensure!(
            !rebuild,
            "Pinned operational serving cannot rebuild its source snapshots"
        );
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 256 * 1024, "Operational manifest exceeds limit");
        let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            manifest["public_release"] == false,
            "Expected private operational manifest"
        );
        if let Some(expected) = manifest["identity_gate_sha256"].as_str() {
            let directory = atlas_ingest::identity_projection::directory(data);
            let gate = std::fs::read(directory.join("identity-gate.manifest.json"))?;
            anyhow::ensure!(
                atlas_ingest::identity_projection::digest(&gate) == expected,
                "Pinned identity gate checksum mismatch"
            );
            atlas_ingest::identity_projection::load(data)?;
        }
        let checked_path = |key: &str, hash_key: &str, env_key: &str| -> anyhow::Result<PathBuf> {
            let path = std::env::var_os(env_key)
                .map(PathBuf::from)
                .or_else(|| manifest[key].as_str().map(PathBuf::from))
                .context("Pinned manifest is missing snapshot path")?;
            let expected = manifest[hash_key]
                .as_str()
                .context("Pinned manifest is missing snapshot hash")?;
            anyhow::ensure!(
                expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid pinned snapshot hash"
            );
            let mut file = std::fs::File::open(&path)?;
            let mut hash = Sha256::new();
            let mut block = vec![0u8; 1024 * 1024];
            loop {
                let n = file.read(&mut block)?;
                if n == 0 {
                    break;
                }
                hash.update(&block[..n]);
            }
            anyhow::ensure!(
                format!("{:x}", hash.finalize()) == expected,
                "Pinned snapshot checksum mismatch: {}",
                path.display()
            );
            Ok(path)
        };
        let atlas_path = checked_path("atlas_snapshot", "atlas_sha256", "RARE_ATLAS_ATLAS_SNAPSHOT")?;
        let graph_path = checked_path("graph_snapshot", "graph_sha256", "RARE_ATLAS_GRAPH_SNAPSHOT")?;
        // Fail boot closed when the operational mechanism artifact is missing or altered.
        checked_path("mechanism_snapshot", "mechanism_sha256", "RARE_ATLAS_MECHANISM_SNAPSHOT")?;
        let (atlas, _) = atlas_core::snapshot::load(&atlas_path)?;
        let (mut graph, _) = atlas_core::snapshot::load_graph(&graph_path)?;
        graph.set_withhold(atlas_ingest::withhold::load_or_closed(
            data,
            atlas_ingest::withhold::salt_from_env(),
        ));
        eprintln!("loaded checksum-pinned operational Atlas and Graph snapshots");
        return Ok((atlas, graph));
    }
    let t = Instant::now();
    let graph_path = atlas_ingest::graph::snapshot_path(data);
    let fresh = !rebuild
        && atlas_ingest::graph::signature(data)
            .ok()
            .is_some_and(|sig| atlas_core::snapshot::graph_signature(&graph_path).is_ok_and(|s| s == sig));
    let (atlas, graph) = std::thread::scope(|s| {
        let pre = fresh.then(|| s.spawn(|| atlas_core::snapshot::load_graph(&graph_path)));
        let atlas = atlas_ingest::load_or_build(data, rebuild)
            .with_context(|| format!("loading the atlas from {}", data.display()));
        let graph = pre.map(|h| h.join().expect("graph loader panicked"));
        (atlas, graph)
    });
    let (atlas, o) = atlas?;
    eprintln!(
        "{} {} ({:.1}s)",
        origin(o),
        atlas_ingest::snapshot_path(data).display(),
        t.elapsed().as_secs_f64()
    );
    let mut graph = match graph {
        Some(Ok((g, _))) => {
            eprintln!(
                "loaded snapshot {} ({:.1}s)",
                graph_path.display(),
                t.elapsed().as_secs_f64()
            );
            g
        }
        _ => {
            let (g, o) = atlas_ingest::graph::load_or_build(data, &atlas, rebuild)
                .with_context(|| format!("building the graph from {}", data.display()))?;
            eprintln!(
                "{} {} ({:.1}s)",
                origin(o),
                graph_path.display(),
                t.elapsed().as_secs_f64()
            );
            g
        }
    };
    graph.set_withhold(atlas_ingest::withhold::load_or_closed(
        data,
        atlas_ingest::withhold::salt_from_env(),
    ));
    Ok((atlas, graph))
}
