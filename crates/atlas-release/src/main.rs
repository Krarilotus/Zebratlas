//! Offline, licence-aware release application. Shared source files are read-only.
use anyhow::{Context, ensure};
use atlas_core::{Graph, snapshot};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Create a local, citable atlas release (D35)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    WithholdIndex {
        #[arg(long, env = "RARE_ATLAS_DATA")]
        data: PathBuf,
    },
    ExportRelease {
        #[arg(long, env = "RARE_ATLAS_DATA")]
        data: PathBuf,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "release")]
        output: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let command = Cli::parse().command;
    if let Command::WithholdIndex { data } = &command {
        println!("{}", atlas_release::withhold_index::index(data)?);
        return Ok(());
    }
    let Command::ExportRelease { data, version, output } = command else {
        unreachable!()
    };
    ensure!(
        !version.is_empty()
            && version
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
            && version != "."
            && version != "..",
        "version must be a safe directory component"
    );
    atlas_ingest::withhold::load(&data, atlas_ingest::withhold::salt_from_env())?;
    atlas_ingest::identity_projection::load(&data)?;
    let raw = atlas_ingest::raw_dir(&data);
    // Raw-only snapshot signatures predate accepted identity decisions. An offline
    // release rebuilds through the current native policy without saving shared caches.
    eprintln!("Rebuilding native atlas with current identity policy; shared caches remain read-only");
    let atlas = atlas_ingest::build(&raw)?;
    let path = atlas_ingest::graph::snapshot_path(&data);
    let sig = atlas_ingest::graph::signature(&data)?;
    let graph = if snapshot::graph_signature(&path).is_ok_and(|s| s == sig) {
        snapshot::load_graph(&path)?.0
    } else {
        eprintln!("Graph snapshot stale: rebuilding in memory; shared caches remain read-only");
        Graph::new(atlas_ingest::graph::build(&data, &atlas)?)
    };
    // Strict: an unreadable suppression or quarantine list stops the release (D43, fail-closed).
    let withhold = atlas_ingest::withhold::load(&data, atlas_ingest::withhold::salt_from_env())?;
    let quarantine = atlas_release::quarantine::Quarantine::load(&data)?;
    let suppression = atlas_release::suppression::Summary::load(&data)?;
    let target = output.join(&version);
    let report =
        atlas_release::export_with_summary(&atlas, &graph, &withhold, &target, &version, quarantine, suppression)
            .with_context(|| format!("exporting {}", target.display()))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
