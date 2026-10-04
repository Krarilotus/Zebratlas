//! Gene2Phenotype (`allG2P_<date>.csv.gz`): one record per gene–disease model with confidence,
//! allelic requirement and mechanism. The one G2P parser of the workspace: the build uses it for
//! newly described conditions, the mechanism layer for variant effect and mechanism.

use std::io::BufReader;
use std::path::Path;

use atlas_core::provenance::{EntityIdx, RecordRef};

use crate::error::IngestError;

/// One G2P row (CSV line numbers are 1-based physical lines; the header is line 1).
#[derive(Clone, Debug, PartialEq)]
pub struct G2pRecord {
    /// `G2P03465`.
    pub id: String,
    pub symbol: String,
    /// `HGNC:11433`.
    pub hgnc: Option<String>,
    pub previous_symbols: Vec<String>,
    pub disease_name: String,
    /// `OMIM:…` when G2P names an OMIM phenotype.
    pub disease_omim: Option<String>,
    /// `MONDO:…` as given (often a generic term for new conditions).
    pub disease_mondo: Option<String>,
    pub allelic_requirement: String,
    /// definitive / strong / moderate / limited / disputed / refuted.
    pub confidence: String,
    pub variant_consequence: String,
    pub molecular_mechanism: String,
    pub molecular_mechanism_support: String,
    /// `PMID:…`.
    pub publications: Vec<String>,
    pub panels: Vec<String>,
    pub last_review: String,
    pub record: RecordRef,
}

impl G2pRecord {
    /// Confidence a link may rest on (not disputed or refuted).
    pub fn is_supported(&self) -> bool {
        matches!(
            self.confidence.as_str(),
            "definitive" | "strong" | "moderate" | "limited"
        )
    }
}

fn list(s: &str) -> Vec<String> {
    s.split(';')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

fn opt(s: &str, prefix: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else if s.contains(':') {
        Some(s.to_owned())
    } else {
        Some(format!("{prefix}:{s}"))
    }
}

/// Every G2P record of the file.
pub fn read_g2p(path: &Path, entity: EntityIdx) -> Result<Vec<G2pRecord>, IngestError> {
    let file = std::fs::File::open(path).map_err(IngestError::io(path))?;
    let gz = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(gz);
    let headers = rdr.headers().map_err(|e| csv_err(path, e))?.clone();
    let col = |name: &'static str| {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or(IngestError::MissingColumn {
                path: path.to_owned(),
                column: name,
            })
    };
    let c = [
        col("g2p id")?,
        col("gene symbol")?,
        col("hgnc id")?,
        col("previous gene symbols")?,
        col("disease name")?,
        col("disease mim")?,
        col("disease MONDO")?,
        col("allelic requirement")?,
        col("confidence")?,
        col("variant consequence")?,
        col("molecular mechanism")?,
        col("molecular mechanism support")?,
        col("publications")?,
        col("panel")?,
        col("date of last review")?,
    ];
    let mut out = Vec::new();
    for row in rdr.records() {
        let row = row.map_err(|e| csv_err(path, e))?;
        let line = row.position().map_or(0, |p| p.line()) as u32;
        let f = |i: usize| row.get(c[i]).unwrap_or("").trim();
        out.push(G2pRecord {
            id: f(0).to_owned(),
            symbol: f(1).to_owned(),
            hgnc: opt(f(2), "HGNC"),
            previous_symbols: list(f(3)),
            disease_name: f(4).to_owned(),
            disease_omim: opt(f(5), "OMIM"),
            disease_mondo: opt(f(6), "MONDO"),
            allelic_requirement: f(7).to_owned(),
            confidence: f(8).to_lowercase(),
            variant_consequence: f(9).to_owned(),
            molecular_mechanism: f(10).to_owned(),
            molecular_mechanism_support: f(11).to_owned(),
            publications: list(f(12)).into_iter().map(|p| format!("PMID:{p}")).collect(),
            panels: list(f(13)),
            last_review: f(14).to_owned(),
            record: RecordRef::line(entity, line),
        });
    }
    Ok(out)
}

fn csv_err(path: &Path, source: csv::Error) -> IngestError {
    IngestError::Csv {
        path: path.to_owned(),
        source,
    }
}
