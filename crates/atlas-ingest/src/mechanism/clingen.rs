//! ClinGen dosage sensitivity curation (gene list, GRCh38): haploinsufficiency (HI) and
//! triplosensitivity (TS) scores. HI 3 = sufficient evidence that loss of one copy causes disease,
//! i.e. independent support for a loss-of-function mechanism; 30 = autosomal recessive; 40 = unlikely.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use atlas_core::provenance::{EntityIdx, RecordRef};

use crate::error::IngestError;

pub use atlas_core::mechanism::clingen::{ClinGenDosage, DosageScore};

/// Records and the `#03 Oct,2026` date header.
pub fn read(path: &Path, entity: EntityIdx) -> Result<(Vec<ClinGenDosage>, Option<String>), IngestError> {
    let f = File::open(path).map_err(IngestError::io(path))?;
    let mut out = Vec::new();
    let mut date = None;
    let mut header: Option<Vec<String>> = None;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(IngestError::io(path))?;
        if let Some(h) = line.strip_prefix('#') {
            if h.starts_with("Gene Symbol") {
                header = Some(h.split('\t').map(str::to_owned).collect());
            } else if i == 1 {
                date = Some(h.trim().to_owned());
            }
            continue;
        }
        let Some(hdr) = header.as_ref() else { continue };
        let cols: Vec<&str> = line.split('\t').collect();
        let get = |name: &str| -> &str {
            hdr.iter()
                .position(|h| h == name)
                .and_then(|i| cols.get(i).copied())
                .unwrap_or("")
                .trim()
        };
        let score = |prefix: &str, disease: &str| {
            let raw = get(&format!("{prefix} Score")).to_owned();
            DosageScore {
                score: raw.parse().ok(),
                raw,
                description: get(&format!("{prefix} Description")).to_owned(),
                pmids: (1..=6)
                    .map(|k| get(&format!("{prefix} PMID{k}")))
                    .filter(|p| !p.is_empty())
                    .map(|p| format!("PMID:{p}"))
                    .collect(),
                disease: Some(get(disease).to_owned()).filter(|d| !d.is_empty()),
            }
        };
        out.push(ClinGenDosage {
            symbol: get("Gene Symbol").to_owned(),
            ncbi_gene: get("Gene ID").to_owned(),
            haploinsufficiency: score("Haploinsufficiency", "Haploinsufficiency Disease ID"),
            triplosensitivity: score("Triplosensitivity", "Triplosensitivity Disease ID"),
            evaluated: get("Date Last Evaluated").to_owned(),
            record: RecordRef::line(entity, i as u32 + 1),
        });
    }
    Ok((out, date))
}
