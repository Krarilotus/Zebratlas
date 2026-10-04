//! Read-only shared-data acceptance. Output is confined to cache/semantic-units/.
use atlas_core::{snapshot, units};
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = atlas_ingest::data_dir();
    let (atlas, _) = snapshot::load(&atlas_ingest::snapshot_path(&data))?;
    let withhold = atlas_ingest::withhold::load_or_closed(&data, atlas_ingest::withhold::salt_from_env());
    let mut graph = match snapshot::load_graph(&atlas_ingest::graph::snapshot_path(&data)) {
        Ok((graph, _)) => graph,
        Err(error) => {
            eprintln!("shared graph snapshot unavailable ({error}); build in memory without writing it");
            atlas_core::Graph::new(atlas_ingest::graph::build_with(&data, &atlas, &withhold)?)
        }
    };
    graph.set_withhold(withhold);
    let pathways = atlas_ingest::semantic_units::read(&data, &atlas)?;
    assert!(pathways.available, "NCBI2Reactome is required for this acceptance run");
    let output = data.join("cache/semantic-units");
    std::fs::create_dir_all(&output)?;
    let mut samples = Vec::new();
    for focus in ["STXBP1", "SNAP25"] {
        let started = std::time::Instant::now();
        let c = units::build(&atlas, &graph, &pathways, focus).ok_or("focus is not in the real atlas")?;
        let build_seconds = started.elapsed().as_secs_f64();
        let ids: std::collections::BTreeSet<_> = c.units.iter().map(|u| &u.id).collect();
        assert!(c.units.iter().all(|u| u.children.iter().all(|id| ids.contains(id))));
        assert!(
            c.units.iter().all(|u| !u.evidence.is_empty()),
            "every unit needs evidence"
        );
        let connections: Vec<_> = c
            .units
            .iter()
            .filter(|u| u.relation.as_deref() == Some("shares_pathway_with"))
            .collect();
        assert!(!connections.is_empty(), "need an actual pathway connection");
        assert!(
            connections
                .iter()
                .all(|u| u.status == units::AssertionStatus::Inferred && u.support.len() >= 2)
        );
        let ttl = units::rdf::turtle(&c);
        let elapsed = started.elapsed().as_secs_f64();
        let artifact = output.join(format!("{focus}.ttl"));
        std::fs::write(&artifact, &ttl)?;
        let sample = json!({"focus":c.focus, "overview":c.overview, "units":c.units.len(),
            "roots":c.roots.len(), "inferred_connections":connections.len(), "sample_connection":connections.first(),
            "elapsed_seconds":elapsed, "build_seconds":build_seconds, "rdf_seconds":elapsed-build_seconds,
            "root_summaries":c.root_summaries, "rdf_bytes":ttl.len(), "rdf_sha256":format!("{:x}",Sha256::digest(ttl.as_bytes())),
            "rdf_file":artifact.to_string_lossy(), "all_children_resolve":true, "all_units_have_evidence":true});
        println!(
            "{}",
            serde_json::to_string(&json!({"focus":focus, "overview":c.overview,
            "units":c.units.len(), "connections":connections.len(), "seconds":elapsed, "rdf_bytes":ttl.len()}))?
        );
        samples.push(sample);
    }
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut code_files = Vec::new();
    for relative in [
        "crates/atlas-core/src/units.rs",
        "crates/atlas-core/src/units/build.rs",
        "crates/atlas-core/src/units/rdf.rs",
        "crates/atlas-ingest/src/semantic_units.rs",
        "crates/atlas-ingest/examples/semantic_units_acceptance.rs",
        "crates/atlas-ingest/examples/verify_semantic_units.py",
    ] {
        let bytes = std::fs::read(repo.join(relative))?;
        code_files.push(json!({"file":relative, "sha256":format!("{:x}", Sha256::digest(bytes))}));
    }
    let result = json!({"schema":"atlas.semantic-units.acceptance.v1", "samples":samples,
        "code_files":code_files,
        "generated_at":atlas_core::provenance::rfc3339(std::time::SystemTime::now()),
        "pathway_activity":pathways.activity, "excluded_rows":pathways.excluded.len()});
    std::fs::write(output.join("acceptance.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
