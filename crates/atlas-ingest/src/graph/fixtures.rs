//! Test fixtures for the graph readers: a tiny atlas (one rare condition, its gene, two symptoms),
//! a throw-away data dir, and envelope/JSON-lines writers with correct header checksums.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use atlas_core::disease::Disease;
use atlas_core::evidence::GeneLink;
use atlas_core::provenance::{ActivityIdx, Agent, EntityIdx, Provenance, RecordRef};
use atlas_core::{Atlas, DiseaseIdentity, Term};
use serde_json::{Value, json};

use super::builder::Builder;
use super::cache;

pub const CONDITION: &str = "MONDO:0012812";
pub const GENE: &str = "HGNC:11444";

fn term(id: &str, name: &str, parent: Option<&str>) -> Term {
    Term {
        id: id.into(),
        name: name.into(),
        parents: parent.map(|p| vec![p.to_owned()]).unwrap_or_default(),
        ..Term::default()
    }
}

/// DEE4 (rare) caused by STXBP1, plus a non-rare condition; HPO with Seizure.
pub fn atlas() -> Atlas {
    let hpo = vec![
        term("HP:0000001", "All", None),
        term("HP:0000118", "Phenotypic abnormality", Some("HP:0000001")),
        term("HP:0001250", "Seizure", Some("HP:0000118")),
    ];
    let rec = RecordRef::line(EntityIdx(0), 1);
    let mut d = Disease::new(CONDITION, ActivityIdx(0));
    d.name = "developmental and epileptic encephalopathy 4".into();
    d.rare = true;
    d.source_ids.insert("ORPHA:178469".into());
    d.genes.push(GeneLink {
        symbol: "STXBP1".into(),
        association: "Disease-causing germline mutation(s) in".into(),
        source: "Orphanet".into(),
        source_disease: "ORPHA:178469".into(),
        pmids: vec![],
        assessed: Some(true),
        hgnc: Some(GENE.into()),
        ncbi_gene: None,
        record: rec,
    });
    let mut common = Disease::new("MONDO:0005027", ActivityIdx(0));
    common.name = "epilepsy".into();
    Atlas::new(hpo, DiseaseIdentity::default(), Provenance::default(), vec![d, common])
}

pub fn builder(atlas: &Atlas) -> Builder<'_> {
    Builder::new(
        atlas,
        Agent {
            name: "test".into(),
            version: "0".into(),
            commit: None,
        },
    )
}

/// A data dir under the system temp dir, removed on drop.
pub struct TempData(pub PathBuf);

impl TempData {
    pub fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "atlas-ingest-fx-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `data/<file>` as a schema envelope whose header sha256 matches its records.
    pub fn envelope(&self, file: &str, schema: &str, records: Value) {
        let sha = cache::hex(&cache::canonical_sha256(&records));
        let n = records.as_array().map_or(0, Vec::len);
        let v = json!({
            "schema": schema, "version": 1,
            "header": {"source": "fixture", "retrieved_at": "2026-10-04T00:00:00+00:00", "record_count": n, "sha256": sha},
            "records": records,
        });
        self.write(file, &serde_json::to_vec_pretty(&v).unwrap());
    }

    /// `data/<file>` as JSON lines.
    pub fn jsonl(&self, file: &str, lines: &[Value]) {
        let text: Vec<String> = lines.iter().map(|l| serde_json::to_string(l).unwrap()).collect();
        self.write(file, (text.join("\n") + "\n").as_bytes());
    }

    pub fn write(&self, file: &str, bytes: &[u8]) {
        let p = self.0.join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

impl Drop for TempData {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Fixtures pass through the same global producer gate as production inputs.
pub fn gate(d: &TempData) {
    let input = d.path().join("cache/intake");
    let output = d.path().join("cache/mappings");
    std::fs::rename(&output, &input).unwrap();
    atlas_discovery::align::safety::directory(&input, &output, None).unwrap();
}
