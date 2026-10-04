//! Read-only domain retrieval benchmark; inputs and outputs are kept explicit.
use atlas_core::search::{SearchOptions, domain::Index};
use serde_json::json;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let data = atlas_ingest::data_dir();
    let atlas_path = atlas_ingest::snapshot_path(&data);
    let graph_path = PathBuf::from(std::env::var_os("RARE_ATLAS_GRAPH_SNAPSHOT").expect("private graph snapshot path"));
    let (atlas, _) = atlas_core::snapshot::load(&atlas_path)?;
    let (mut graph, _) = atlas_core::snapshot::load_graph(&graph_path)?;
    graph.set_withhold(atlas_ingest::withhold::load(
        &data,
        atlas_ingest::withhold::salt_from_env(),
    )?);
    let index = Index::build(&atlas, &graph);
    let samples: usize = std::env::var("PERF_SAMPLES").unwrap_or_else(|_| "30".into()).parse()?;
    anyhow::ensure!(samples > 0, "PERF_SAMPLES must be positive");
    let mut runs = Vec::new();
    for q in [
        "STXBP1",
        "STX",
        "My daughter has STXBP1",
        "epileptic encephalopathy",
        "patient groups with a recruiting study in Germany for conditions sharing a pathway with STXBP1",
    ] {
        let mut times = Vec::new();
        let mut result = None;
        for _ in 0..samples {
            let start = Instant::now();
            let matches = black_box(index.lexical_context(
                &atlas,
                &graph,
                q,
                SearchOptions {
                    limit: 100,
                    include_retired: false,
                },
            ));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            result = Some(matches);
        }
        times.sort_by(f64::total_cmp);
        let result = result.unwrap();
        runs.push(json!({"query":q,"samples_ms":times,"median_ms":(times[(samples-1)/2]+times[samples/2])/2.0,
            "p95_ms":times[(samples*95).div_ceil(100)-1],"hits":result.hits,"present":result.present,"excluded":result.excluded}));
    }
    println!(
        "{}",
        json!({"schema":"atlas.perf.retrieval","version":1,"prov:used":[atlas_path,graph_path],"runs":runs})
    );
    Ok(())
}
