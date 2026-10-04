//! Exact-row Reactome membership evidence for the additive semantic-unit view.
//! Reads shared raw data; writes no snapshots, caches, or source files.
use crate::{error::IngestError, sources};
use atlas_core::Atlas;
use atlas_core::node::{NodeKind, NodeRef};
use atlas_core::provenance::{Activity, Agent};
use atlas_core::units::{ExcludedAnnotation, PathwayAnnotation, PathwayData, UnitEvidence};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

pub fn read(data: &Path, atlas: &Atlas) -> Result<PathwayData, IngestError> {
    let started_at = atlas_core::provenance::rfc3339(std::time::SystemTime::now());
    let path = data.join("raw/NCBI2Reactome.txt");
    if !path.exists() {
        return Ok(PathwayData::default());
    }
    // Hash and parse the same immutable byte buffer, preventing a time-of-check/read mismatch.
    let bytes = std::fs::read(&path).map_err(IngestError::io(&path))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| IngestError::Io {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
    })?;
    let spec = sources::SourceFile {
        file: "NCBI2Reactome.txt",
        url: "https://reactome.org/download/current/NCBI2Reactome.txt",
        licence: "CC0 (https://reactome.org/license, data clause 1c)",
    };
    let mut source = sources::entity(&data.join("raw"), &spec)?;
    source.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
    source.bytes = bytes.len() as u64;
    // The unversioned 'current' download carries no verified release ID.
    source.version = None;
    let mut out = PathwayData {
        available: true,
        activity: Activity {
            id: "activity:semantic-units-reactome-v1".into(),
            label: "Read direct human Reactome memberships with row provenance".into(),
            started_at: Some(started_at),
            agent: Agent {
                name: "atlas-ingest/semantic-units".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                commit: None,
            },
            ..Activity::default()
        },
        ..PathwayData::default()
    };
    let genes: HashMap<_, _> = atlas
        .genes()
        .iter()
        .filter_map(|g| {
            g.ncbi_gene
                .as_deref()
                .map(|id| (id.strip_prefix("NCBIGene:").unwrap_or(id), g.id()))
        })
        .collect();
    out.activity.parameters.insert(
        "rule".into(),
        "six-column NCBI2Reactome; human; atlas genes; direct annotations only".into(),
    );
    out.activity
        .parameters
        .insert("source_sha256".into(), source.sha256.clone().unwrap());
    for (i, line) in text.lines().enumerate() {
        out.activity.count("read", 1);
        let cols: Vec<_> = line.split('\t').collect();
        let reason = if cols.len() != 6 {
            Some("malformed_row")
        } else if cols[5] != "Homo sapiens" {
            Some("non_human")
        } else if !cols[1].starts_with("R-HSA-") {
            Some("invalid_human_pathway_id")
        } else if !genes.contains_key(cols[0]) {
            Some("gene_outside_atlas")
        } else {
            None
        };
        let locator = format!("L{}", i + 1);
        if let Some(reason) = reason {
            out.activity.count(&format!("skipped:{reason}"), 1);
            out.excluded.push(ExcludedAnnotation {
                locator,
                reason: reason.into(),
            });
            continue;
        }
        out.annotations.push(PathwayAnnotation {
            gene: genes[cols[0]].into(),
            pathway: NodeRef {
                id: cols[1].into(),
                kind: NodeKind::Pathway,
                label: cols[3].into(),
            },
            evidence: UnitEvidence {
                source: source.clone(),
                locator,
                record_url: Some(cols[2].into()),
                retrieved_at: source.retrieved_at.clone(),
                record_sha256: Some(format!("{:x}", Sha256::digest(line.as_bytes()))),
                upstream_activity: Some(out.activity.id.clone()),
                evidence_code: Some(cols[4].into()),
                status: Some(if cols[4] == "IEA" {
                    atlas_core::units::AssertionStatus::Inferred
                } else {
                    atlas_core::units::AssertionStatus::Asserted
                }),
            },
        });
        out.activity.count("kept", 1);
    }
    out.activity.ended_at = Some(atlas_core::provenance::rfc3339(std::time::SystemTime::now()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::evidence::GeneLink;
    use atlas_core::provenance::{ActivityIdx, EntityIdx, RecordRef};
    use atlas_core::{Disease, DiseaseIdentity, Provenance};

    #[test]
    fn row_hashes_locators_and_exclusion_ledger_are_exact() {
        let root = std::env::temp_dir().join(format!("atlas-units-fixture-{}", std::process::id()));
        let raw = root.join("raw");
        std::fs::create_dir_all(&raw).unwrap();
        let mut disease = Disease::new("MONDO:PLACEHOLDER", ActivityIdx(0));
        disease.genes.push(GeneLink {
            symbol: "PLACEHOLDER_GENE".into(),
            association: "MENDELIAN".into(),
            source: "fixture".into(),
            source_disease: disease.id.clone(),
            pmids: Vec::new(),
            assessed: None,
            hgnc: None,
            ncbi_gene: Some("NCBIGene:1".into()),
            record: RecordRef::line(EntityIdx(0), 1),
        });
        let atlas = Atlas::new(
            Vec::new(),
            DiseaseIdentity::default(),
            Provenance::default(),
            vec![disease],
        );
        assert!(!read(&root, &atlas).unwrap().available);
        let kept = "1\tR-HSA-PLACEHOLDER\thttps://example.org\tPlaceholder process\tIEA\tHomo sapiens";
        let file = format!(
            "{kept}\ninvalid\n1\tR-MUS-PLACEHOLDER\thttps://example.org\tPlaceholder mouse process\tTAS\tMus musculus\n99\tR-HSA-PLACEHOLDER\thttps://example.org\tPlaceholder process\tTAS\tHomo sapiens\n"
        );
        let path = raw.join("NCBI2Reactome.txt");
        std::fs::write(&path, &file).unwrap();
        let result = read(&root, &atlas).unwrap();
        assert_eq!(result.annotations.len(), 1);
        assert_eq!(result.excluded.len(), 3);
        let a = &result.annotations[0];
        assert_eq!(a.gene, "NCBIGene:1");
        assert_eq!(a.evidence.locator, "L1");
        assert_eq!(
            a.evidence.record_sha256,
            Some(format!("{:x}", Sha256::digest(kept.as_bytes())))
        );
        assert_eq!(
            a.evidence.source.sha256,
            Some(format!("{:x}", Sha256::digest(file.as_bytes())))
        );
        assert_eq!(a.evidence.evidence_code.as_deref(), Some("IEA"));
        assert!(a.evidence.source.version.is_none());
        assert_eq!(result.activity.counts["read"], 4);
        assert_eq!(result.excluded[0].locator, "L2");
        assert_eq!(result.excluded[0].reason, "malformed_row");
        assert_eq!(result.excluded[1].reason, "non_human");
        assert_eq!(result.excluded[2].reason, "gene_outside_atlas");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(raw).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
