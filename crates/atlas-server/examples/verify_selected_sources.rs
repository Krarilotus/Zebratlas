//! Read-only final-reader evidence for explicitly supplied private snapshots.
use std::path::Path;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() >= 5, "usage: verify_selected_sources DATA ATLAS GRAPH SOURCE [SOURCE...]");
    let data = Path::new(&args[1]);
    let atlas = atlas_core::snapshot::load(Path::new(&args[2]))?.0;
    let (graph, signature) = atlas_core::snapshot::load_graph(Path::new(&args[3]))?;
    let report = atlas_core::integrity::check(&atlas, &graph);
    anyhow::ensure!(report.passed, "private snapshot integrity failed: {:?}", report.violations);
    if args[4..].iter().any(|s| s.contains("carryover")) {
        let asset = |id: &str| graph.data().assets.iter().find(|a| a.id == id).expect("required carryover card");
        let sgc = asset("carryover:sgc-probe-catalogue");
        anyhow::ensure!(sgc.access_context().get("material_rights").is_some(), "missing SGC material rights");
        let site = asset("carryover:NCT06625112:site:0");
        anyhow::ensure!(site.access_context()["site_status"] == "RECRUITING", "site scope lost");
        anyhow::ensure!(site.facts.iter().any(|(k,v)| k == "intended_use" && v.contains("Study:") && v.contains("site:")), "study/site distinction lost");
        let comparison = asset("carryover:starr-esco-component-review");
        let context = comparison.access_context();
        anyhow::ensure!(context["access_routes"].as_array().is_some_and(|routes| routes.len() == 2), "comparison routes lost");
        for key in ["population_difference", "unresolved_question", "rights"] {
            anyhow::ensure!(context.get(key).is_some(), "comparison limit lost: {key}");
        }
        anyhow::ensure!(!comparison.release, "comparison rights do not allow bulk release");
    }
    for source in &args[4..] {
        let mut checked = 0;
        let mut failed = Vec::new();
        for (idx, record) in graph.data().records.iter().enumerate() {
            let entity = graph.provenance().entity(record.entity);
            if !entity.file.contains(source) { continue; }
            let result = atlas_ingest::graph::verify::record(data, &graph, idx as u32);
            checked += 1;
            if !result.matches { failed.push(result); }
        }
        println!("{}", serde_json::json!({"source":source,"signature":signature,"integrity":true,"checked":checked,"coverage":graph.coverage().iter().filter(|c| c.source.contains(source)).collect::<Vec<_>>(),"failed":failed}));
        anyhow::ensure!(checked > 0 && failed.is_empty(), "source verification failed: {source}");
    }
    Ok(())
}
