//! Read-only benchmark against existing snapshots. No builds or writes to shared data.
use atlas_core::query_graph::{Direction, SuggestRequest, SuggestionIndex};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(std::env::var("RARE_ATLAS_DATA").expect("set RARE_ATLAS_DATA"));
    let snapshots = std::env::var("RARE_ATLAS_SNAPSHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data.join("cache"));
    let (atlas, _) = atlas_core::snapshot::load(&snapshots.join("atlas.snapshot"))?;
    let (graph, _) = atlas_core::snapshot::load_graph(&snapshots.join("graph.snapshot"))?;
    println!("Read-only snapshots: {}", snapshots.display());
    let start = Instant::now();
    let index = SuggestionIndex::new(&atlas, &graph);
    println!(
        "index build: {:?}; connected edges: {}; unresolved omitted: {}; fingerprint: {}",
        start.elapsed(),
        graph.edges().len(),
        index.excluded_unresolved,
        index.sha256
    );
    for request in [
        SuggestRequest {
            node: Some("STXBP1".into()),
            ..Default::default()
        },
        SuggestRequest {
            class: Some("gene".into()),
            ..Default::default()
        },
        SuggestRequest {
            class: Some("gene".into()),
            relation: Some("about_gene".into()),
            direction: Some(Direction::Incoming),
            q: Some("synaptic".into()),
            ..Default::default()
        },
    ] {
        let mut samples = vec![];
        let mut total = 0;
        for _ in 0..30 {
            let start = Instant::now();
            let result = index.suggest(&request, &atlas, &graph, &|_| true).unwrap();
            total = result.total;
            samples.push(start.elapsed());
        }
        samples.sort();
        println!(
            "{request:?}: total={total}; p50={:?}; p95={:?}",
            samples[15], samples[28]
        );
    }
    Ok(())
}
