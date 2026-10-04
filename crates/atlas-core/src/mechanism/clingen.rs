//! ClinGen dosage sensitivity records (haploinsufficiency / triplosensitivity scores). Parsed in
//! `atlas_ingest::mechanism::clingen`.

use serde::{Deserialize, Serialize};

use crate::provenance::RecordRef;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DosageScore {
    /// As written: `0`..`3`, `30`, `40` or `Not yet evaluated`.
    pub raw: String,
    pub score: Option<u8>,
    pub description: String,
    pub pmids: Vec<String>,
    /// MONDO id of the curated disease, where given.
    pub disease: Option<String>,
}

impl DosageScore {
    /// Score 3: sufficient evidence for dosage pathogenicity.
    pub fn sufficient(&self) -> bool {
        self.score == Some(3)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinGenDosage {
    pub symbol: String,
    pub ncbi_gene: String,
    pub haploinsufficiency: DosageScore,
    pub triplosensitivity: DosageScore,
    pub evaluated: String,
    pub record: RecordRef,
}

impl ClinGenDosage {
    pub fn url(&self) -> String {
        format!(
            "https://search.clinicalgenome.org/kb/gene-dosage?search={}",
            self.symbol
        )
    }
}
