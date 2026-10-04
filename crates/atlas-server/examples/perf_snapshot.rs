//! Prepare an isolated, compatible snapshot without changing shared snapshots.
//! RARE_ATLAS_DATA=<main>/data cargo run -j 2 -p atlas-server --example perf_snapshot
fn main() -> anyhow::Result<()> {
    let data = atlas_ingest::data_dir();
    let (atlas, _) = atlas_core::snapshot::load(&atlas_ingest::snapshot_path(&data))?;
    let signature = atlas_ingest::graph::signature(&data)?;
    let graph = atlas_ingest::graph::build(&data, &atlas)?;
    let path = std::env::var_os("RARE_ATLAS_PERF_GRAPH_SNAPSHOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| data.join("cache/perf/graph.snapshot"));
    atlas_core::snapshot::save_graph(&path, &graph, &signature)?;
    println!(
        "{} nodes, {} edges: {}",
        graph.studies.len()
            + graph.grants.len()
            + graph.papers.len()
            + graph.people.len()
            + graph.orgs.len()
            + graph.assets.len(),
        graph.edges.len(),
        path.display()
    );
    Ok(())
}
