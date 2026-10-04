//! Evidence records on disease edges: HPO annotations, gene links, prevalence.
//! Each keeps its source record ([`RecordRef`]).

use serde::{Deserialize, Serialize};

use crate::provenance::RecordRef;

/// HPO frequency subontology -> representative probability (midpoint of the defined range).
pub const FREQUENCY_TERMS: &[(&str, f64)] = &[
    ("HP:0040280", 1.0),   // Obligate (100%)
    ("HP:0040281", 0.895), // Very frequent (80-99%)
    ("HP:0040282", 0.545), // Frequent (30-79%)
    ("HP:0040283", 0.17),  // Occasional (5-29%)
    ("HP:0040284", 0.025), // Very rare (1-4%)
    ("HP:0040285", 0.0),   // Excluded (0%)
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frequency {
    pub raw: String,
    /// Probability in [0, 1].
    pub value: Option<f64>,
    /// (affected, examined) when given as n/m.
    pub cohort: Option<(u64, u64)>,
}

impl Frequency {
    /// HPO term, `n/m` or `x%`; anything else keeps only `raw`. Empty -> None.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let make = |value, cohort| {
            Some(Self {
                raw: raw.to_owned(),
                value,
                cohort,
            })
        };
        if let Some(&(_, p)) = FREQUENCY_TERMS.iter().find(|(id, _)| *id == raw) {
            return make(Some(p), None);
        }
        if let Some((n, d)) = raw.split_once('/')
            && let (Some(n), Some(d)) = (digits(n), digits(d))
        {
            let value = (d != 0).then(|| n as f64 / d as f64);
            return make(value, Some((n, d)));
        }
        if let Some(pct) = raw.strip_suffix('%')
            && is_decimal(pct)
            && let Ok(v) = pct.parse::<f64>()
        {
            return make(Some(v / 100.0), None);
        }
        make(None, None)
    }
}

fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `\d+(\.\d+)?`
fn is_decimal(s: &str) -> bool {
    match s.split_once('.') {
        Some((a, b)) => digits(a).is_some() && !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()),
        None => !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()),
    }
}

/// One phenotype.hpoa row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhenotypeAnnotation {
    /// OMIM:..., ORPHA:..., DECIPHER:...
    pub disease_id: String,
    pub disease_name: String,
    /// As written in the file (may be an alt id; the edge holds the canonical term).
    pub hpo_id: String,
    /// Qualifier NOT: the disease is documented to *lack* this feature.
    pub negated: bool,
    /// PMID:..., OMIM:..., ORPHA:...
    pub references: Vec<String>,
    /// PCS / TAS / IEA.
    pub evidence: String,
    pub onset: Option<String>,
    pub frequency: Option<Frequency>,
    pub sex: Option<String>,
    pub modifiers: Vec<String>,
    /// P phenotype, I inheritance, C clinical course, M modifier, H past history.
    pub aspect: String,
    pub biocuration: String,
    pub record: RecordRef,
}

impl PhenotypeAnnotation {
    /// Last curation date in `biocuration` (`HPO:probinson[2021-06-21]` -> `2021-06-21`).
    pub fn date(&self) -> Option<&str> {
        let end = self.biocuration.rfind(']')?;
        let start = self.biocuration[..end].rfind('[')?;
        Some(&self.biocuration[start + 1..end])
    }
}

/// Disease-gene association from one source record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneLink {
    pub symbol: String,
    pub association: String,
    /// Orphanet / OMIM/MedGen.
    pub source: String,
    pub source_disease: String,
    pub pmids: Vec<String>,
    pub assessed: Option<bool>,
    pub hgnc: Option<String>,
    pub ncbi_gene: Option<String>,
    pub record: RecordRef,
}

impl GeneLink {
    /// The gene causes the disease: OMIM Mendelian, Orphanet disease-causing germline (incl. LoF/GoF),
    /// or a supported Gene2Phenotype model. Not susceptibility, candidate, modifier, role in phenotype,
    /// or Orphanet `UNKNOWN` (e.g. genes inside a contiguous deletion).
    pub fn is_causal(&self) -> bool {
        self.association == "MENDELIAN"
            || self.association.starts_with("Disease-causing germline")
            || self.source == "G2P"
    }

    /// LoF / GoF / DN from the Orphanet association type, else `unknown`.
    pub fn variant_effect(&self) -> &'static str {
        let a = self.association.to_ascii_lowercase();
        if a.contains("loss of function") {
            "LoF"
        } else if a.contains("gain of function") {
            "GoF"
        } else if a.contains("dominant negative") {
            "DN"
        } else {
            "unknown"
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prevalence {
    /// Point prevalence, Birth prevalence, Cases/families, ...
    pub kind: String,
    /// Value and class / Class only / Case(s).
    pub qualification: String,
    /// e.g. "1-9 / 100 000".
    pub prevalence_class: Option<String>,
    pub mean_value: Option<f64>,
    pub geography: String,
    pub validated: bool,
    pub pmids: Vec<String>,
    pub record: RecordRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_forms() {
        assert_eq!(Frequency::parse("HP:0040281").unwrap().value, Some(0.895));
        let f = Frequency::parse("3/12").unwrap();
        assert_eq!((f.value, f.cohort), (Some(0.25), Some((3, 12))));
        assert_eq!(Frequency::parse("40%").unwrap().value, Some(0.4));
        assert_eq!(Frequency::parse("0/0").unwrap().value, None);
        assert_eq!(Frequency::parse("often").unwrap().value, None);
        assert!(Frequency::parse(" ").is_none());
    }
}
