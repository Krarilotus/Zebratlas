//! Build and reverify an explicitly isolated private mechanism snapshot.
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "usage: create_mechanism_snapshot DATA ATLAS OUTPUT");
    let data = Path::new(&args[1]);
    let output = Path::new(&args[3]);
    anyhow::ensure!(!output.exists(), "Output must be a new immutable artifact path");
    anyhow::ensure!(
        atlas_ingest::mechanism::snapshot_path(data) == output,
        "Explicit mechanism snapshot environment must match output"
    );
    let projection = atlas_ingest::identity_projection::load(data)?;
    anyhow::ensure!(
        !projection.accepted.manifest_sha256.is_empty(),
        "Accepted identity gate is required"
    );
    let atlas = atlas_core::snapshot::load(Path::new(&args[2]))?.0;
    let (mechanism, _) = atlas_ingest::mechanism::load_or_build(data, &atlas, true)?;
    anyhow::ensure!(
        !mechanism.hgnc.genes.is_empty()
            && !mechanism.reactome.ids.is_empty()
            && !mechanism.go_bp.ids.is_empty()
            && !mechanism.g2p.is_empty()
            && !mechanism.clingen.is_empty(),
        "Mechanism source collections must be populated"
    );
    let raw = atlas_ingest::raw_dir(data);
    for source in atlas_ingest::mechanism::SOURCES {
        let entity = mechanism
            .provenance
            .entities
            .iter()
            .find(|e| e.file == source.file)
            .ok_or_else(|| anyhow::anyhow!("Missing source entity: {}", source.file))?;
        anyhow::ensure!(
            entity.sha256.as_deref() == Some(atlas_ingest::sources::sha256(&raw.join(source.file))?.as_str()),
            "Mechanism source checksum mismatch: {}",
            source.file
        );
    }
    let signature = atlas_ingest::mechanism::signature(&raw)?;
    let (loaded, saved_signature) = atlas_ingest::mechanism::load(output)?;
    anyhow::ensure!(
        saved_signature == signature
            && loaded.provenance == mechanism.provenance
            && loaded.hgnc.genes.len() == mechanism.hgnc.genes.len()
            && loaded.g2p.len() == mechanism.g2p.len(),
        "Mechanism reload mismatch"
    );
    println!(
        "{}",
        serde_json::json!({"format":atlas_ingest::mechanism::FORMAT,"signature":signature,"identity_gate_sha256":projection.accepted.manifest_sha256,
        "sources":loaded.provenance.entities,"hgnc":loaded.hgnc.genes.len(),"reactome":loaded.reactome.ids.len(),"go_bp":loaded.go_bp.ids.len(),"g2p":loaded.g2p.len(),"clingen":loaded.clingen.len(),"reverified":true})
    );
    Ok(())
}
