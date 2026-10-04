//! HPO annotation files: phenotype.hpoa and genes_to_disease.txt.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use atlas_core::evidence::{Frequency, PhenotypeAnnotation};
use atlas_core::provenance::{EntityIdx, RecordRef};

use crate::error::IngestError;

/// `#version:` from the comment header.
pub fn hpoa_version(path: &Path) -> Result<Option<String>, IngestError> {
    let file = File::open(path).map_err(IngestError::io(path))?;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(IngestError::io(path))?;
        let Some(comment) = line.strip_prefix('#') else {
            break;
        };
        if let Some(v) = comment.strip_prefix("version:") {
            return Ok(Some(v.trim().to_owned()));
        }
    }
    Ok(None)
}

/// Tab-separated rows with a header; `#` lines skipped. Yields (1-based line, `columns` values).
fn read_tsv(path: &Path, columns: &[&'static str], mut row: impl FnMut(u32, &[&str])) -> Result<(), IngestError> {
    let csv_err = |source| IngestError::Csv {
        path: path.to_owned(),
        source,
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .comment(Some(b'#'))
        .flexible(true)
        .from_path(path)
        .map_err(csv_err)?;
    let headers = reader.headers().map_err(csv_err)?.clone();
    let index: Vec<usize> = columns
        .iter()
        .map(|c| {
            headers.iter().position(|h| h == *c).ok_or(IngestError::MissingColumn {
                path: path.to_owned(),
                column: c,
            })
        })
        .collect::<Result<_, _>>()?;
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record).map_err(csv_err)? {
        let line = record.position().map_or(0, |p| p.line()) as u32;
        let fields: Vec<&str> = index.iter().map(|&i| record.get(i).unwrap_or("")).collect();
        row(line, &fields);
    }
    Ok(())
}

fn split_list(value: &str) -> Vec<String> {
    value.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

const HPOA_COLUMNS: [&str; 12] = [
    "database_id",
    "disease_name",
    "qualifier",
    "hpo_id",
    "reference",
    "evidence",
    "onset",
    "frequency",
    "sex",
    "modifier",
    "aspect",
    "biocuration",
];

pub fn read_hpoa(path: &Path, entity: EntityIdx) -> Result<Vec<PhenotypeAnnotation>, IngestError> {
    let mut out = Vec::new();
    read_tsv(path, &HPOA_COLUMNS, |line, f| {
        out.push(PhenotypeAnnotation {
            disease_id: f[0].to_owned(),
            disease_name: f[1].to_owned(),
            negated: f[2] == "NOT",
            hpo_id: f[3].to_owned(),
            references: split_list(f[4]),
            evidence: f[5].to_owned(),
            onset: non_empty(f[6]),
            frequency: Frequency::parse(f[7]),
            sex: non_empty(f[8]),
            modifiers: split_list(f[9]),
            aspect: f[10].to_owned(),
            biocuration: f[11].to_owned(),
            record: RecordRef::line(entity, line),
        });
    })?;
    Ok(out)
}

/// One genes_to_disease.txt row.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneDisease {
    pub ncbi_gene: String,
    pub symbol: String,
    /// MENDELIAN, POLYGENIC, UNKNOWN.
    pub association: String,
    pub disease_id: String,
    pub source: String,
    pub record: RecordRef,
}

pub fn read_genes_to_disease(path: &Path, entity: EntityIdx) -> Result<Vec<GeneDisease>, IngestError> {
    let mut out = Vec::new();
    let columns = [
        "ncbi_gene_id",
        "gene_symbol",
        "association_type",
        "disease_id",
        "source",
    ];
    read_tsv(path, &columns, |line, f| {
        out.push(GeneDisease {
            ncbi_gene: f[0].to_owned(),
            symbol: f[1].to_owned(),
            association: f[2].to_owned(),
            disease_id: f[3].to_owned(),
            source: f[4].to_owned(),
            record: RecordRef::line(entity, line),
        });
    })?;
    Ok(out)
}
