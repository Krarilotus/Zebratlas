//! Build private native/graph snapshots; output paths must be supplied explicitly.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for key in [
        "RARE_ATLAS_MAPPING_DIR",
        "RARE_ATLAS_ATLAS_SNAPSHOT",
        "RARE_ATLAS_GRAPH_SNAPSHOT",
    ] {
        if std::env::var_os(key).is_none() {
            return Err(format!("set {key}").into());
        }
    }
    let data = atlas_ingest::data_dir();
    let (atlas, _) = atlas_ingest::load_or_build(&data, true)?;
    let (graph, _) = atlas_ingest::graph::load_or_build(&data, &atlas, true)?;
    println!(
        "{} diseases; {} identity clusters; {} mapping receipts",
        atlas.diseases().len(),
        graph.data().identity_merges.len(),
        graph
            .data()
            .identity_merges
            .iter()
            .map(|m| m.mappings.len())
            .sum::<usize>()
    );
    Ok(())
}
