//! Source files the build reads, as `prov:Entity` records: URL, licence, size, mtime, sha256.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use atlas_core::provenance::{Agent, SourceEntity};
use sha2::{Digest, Sha256};

use crate::error::IngestError;

pub struct SourceFile {
    pub file: &'static str,
    pub url: &'static str,
    pub licence: &'static str,
}

pub const HP_OBO: &str = "hp.obo";
pub const MONDO_OBO: &str = "mondo.obo";
pub const HPOA: &str = "phenotype.hpoa";
pub const GENES_TO_DISEASE: &str = "genes_to_disease.txt";
pub const PRODUCT1: &str = "en_product1.xml";
pub const PRODUCT6: &str = "en_product6.xml";
pub const PRODUCT9_PREV: &str = "en_product9_prev.xml";
pub const PRODUCT9_AGES: &str = "en_product9_ages.xml";
pub const G2P: &str = "allG2P_2026-09-28.csv.gz";

const HPO_LICENCE: &str = "HPO licence (attribution)";
const CC_BY: &str = "CC-BY-4.0";

/// Every file the graph build uses, with its download URL (same list as `download.py`).
pub const SOURCES: [SourceFile; 9] = [
    SourceFile {
        file: HP_OBO,
        url: "https://purl.obolibrary.org/obo/hp.obo",
        licence: HPO_LICENCE,
    },
    SourceFile {
        file: HPOA,
        url: "https://purl.obolibrary.org/obo/hp/hpoa/phenotype.hpoa",
        licence: HPO_LICENCE,
    },
    SourceFile {
        file: GENES_TO_DISEASE,
        url: "https://purl.obolibrary.org/obo/hp/hpoa/genes_to_disease.txt",
        licence: HPO_LICENCE,
    },
    SourceFile {
        file: MONDO_OBO,
        url: "https://purl.obolibrary.org/obo/mondo.obo",
        licence: CC_BY,
    },
    SourceFile {
        file: PRODUCT1,
        url: "https://www.orphadata.com/data/xml/en_product1.xml",
        licence: CC_BY,
    },
    SourceFile {
        file: PRODUCT6,
        url: "https://www.orphadata.com/data/xml/en_product6.xml",
        licence: CC_BY,
    },
    SourceFile {
        file: PRODUCT9_PREV,
        url: "https://www.orphadata.com/data/xml/en_product9_prev.xml",
        licence: CC_BY,
    },
    SourceFile {
        file: PRODUCT9_AGES,
        url: "https://www.orphadata.com/data/xml/en_product9_ages.xml",
        licence: CC_BY,
    },
    SourceFile {
        file: G2P,
        url: "https://www.ebi.ac.uk/gene2phenotype/download",
        licence: "Gene2Phenotype (EMBL-EBI) open data",
    },
];

/// Entity without checksum or version (filled in by the build).
pub fn entity(raw: &Path, src: &SourceFile) -> Result<SourceEntity, IngestError> {
    let path = raw.join(src.file);
    let meta = std::fs::metadata(&path).map_err(IngestError::io(&path))?;
    Ok(SourceEntity {
        id: format!("source:{}", src.file),
        url: src.url.to_owned(),
        file: src.file.to_owned(),
        version: None,
        retrieved_at: meta.modified().ok().map(rfc3339),
        sha256: None,
        bytes: meta.len(),
        licence: Some(src.licence.to_owned()),
    })
}

pub fn sha256(path: &Path) -> Result<String, IngestError> {
    let mut file = File::open(path).map_err(IngestError::io(path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(IngestError::io(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `file:size:mtime_ns;...` over all sources: cheap freshness key for the snapshot.
pub fn signature(raw: &Path) -> Result<String, IngestError> {
    let mut out = String::new();
    for src in &SOURCES {
        let path = raw.join(src.file);
        let meta = std::fs::metadata(&path).map_err(IngestError::io(&path))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        out.push_str(&format!("{}:{}:{};", src.file, meta.len(), mtime));
    }
    Ok(out)
}

pub use atlas_core::provenance::rfc3339;

/// This software, with the git state of the working tree next to the data directory.
pub fn agent(repo: &Path) -> Agent {
    let commit = std::process::Command::new("git")
        .args(["describe", "--always", "--dirty", "--abbrev=12"])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty());
    Agent {
        name: "atlas-ingest".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        commit,
    }
}
