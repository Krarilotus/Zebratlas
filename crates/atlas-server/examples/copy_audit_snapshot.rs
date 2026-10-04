//! Build only a private connected-layer snapshot for this checkout's contract tests.
//! Raw sources and all other agents' snapshots are read-only.
fn main() -> anyhow::Result<()> {
    let data = atlas_ingest::data_dir();
    let path = std::env::var_os("RARE_ATLAS_GRAPH_SNAPSHOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| data.join("cache/copy-audit/graph.snapshot"));
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let atlas = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))?.0;
    let graph = atlas_core::Graph::new(atlas_ingest::graph::build(&data, &atlas)?);
    let report = atlas_core::integrity::check(&atlas, &graph);
    anyhow::ensure!(report.passed, "private graph failed integrity: {:?}", report.violations);
    let signature = atlas_ingest::graph::signature(&data)?;
    atlas_core::snapshot::save_graph(&path, graph.data(), &signature)?;
    println!("Compatible graph snapshot: {}", path.display());
    Ok(())
}
