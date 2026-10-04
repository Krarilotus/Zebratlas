//! Things a persona can *get* (D39): research models, cell lines, biobanks, registries, datasets,
//! therapy programmes, regulatory designations, drugs, outcome measures and funding calls, each
//! with who holds it and how to request it; plus the licence gate (D35/D37 §3) and legal
//! free-to-read links for papers (D35).

use serde::{Deserialize, Serialize};

use super::RecIdx;
use crate::provenance::EntityIdx;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Animal or other organism model (strain, line, genotype).
    Model,
    CellLine,
    Biobank,
    Registry,
    Dataset,
    /// Company or academic therapy development programme.
    Programme,
    /// Regulatory orphan designation, opinion or approval.
    Designation,
    Drug,
    OutcomeMeasure,
    FundingCall,
    /// A model-organism gene orthologous to a human gene (KGX).
    OrthologGene,
    Other,
}

/// The persona jobs of D39 (API `job=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    ModelsSamples,
    TherapyProgrammes,
    FreePapers,
    Funding,
    OutcomeMeasures,
}

impl Job {
    pub const ALL: [Job; 5] = [
        Self::ModelsSamples,
        Self::TherapyProgrammes,
        Self::FreePapers,
        Self::Funding,
        Self::OutcomeMeasures,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelsSamples => "models_samples",
            Self::TherapyProgrammes => "therapy_programmes",
            Self::FreePapers => "free_papers",
            Self::Funding => "funding",
            Self::OutcomeMeasures => "outcome_measures",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.as_str() == s)
    }
}

impl AssetKind {
    pub const ALL: [AssetKind; 12] = [
        Self::Model,
        Self::CellLine,
        Self::Biobank,
        Self::Registry,
        Self::Dataset,
        Self::Programme,
        Self::Designation,
        Self::Drug,
        Self::OutcomeMeasure,
        Self::FundingCall,
        Self::OrthologGene,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::CellLine => "cell_line",
            Self::Biobank => "biobank",
            Self::Registry => "registry",
            Self::Dataset => "dataset",
            Self::Programme => "programme",
            Self::Designation => "designation",
            Self::Drug => "drug",
            Self::OutcomeMeasure => "outcome_measure",
            Self::FundingCall => "funding_call",
            Self::OrthologGene => "ortholog_gene",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The persona job this kind serves (None: context only, e.g. ortholog genes).
    pub fn job(self) -> Option<Job> {
        match self {
            Self::Model | Self::CellLine | Self::Biobank | Self::Registry | Self::Dataset => Some(Job::ModelsSamples),
            Self::Programme | Self::Designation | Self::Drug => Some(Job::TherapyProgrammes),
            Self::OutcomeMeasure => Some(Job::OutcomeMeasures),
            Self::FundingCall => Some(Job::Funding),
            Self::OrthologGene | Self::Other => None,
        }
    }

    /// One plain line per kind (English fallback of `jobs.kind.<kind>`).
    pub fn explain(self) -> &'static str {
        match self {
            Self::Model => "A research model: an animal or organism line that carries a change in this gene.",
            Self::CellLine => "A cell line researchers can order or request to study the condition in a dish.",
            Self::Biobank => "A biobank stores patient samples that researchers can apply to use.",
            Self::Registry => {
                "A registry collects information from many patients; researchers can ask for data access."
            }
            Self::Dataset => "A dataset or study collection that researchers can request.",
            Self::Programme => {
                "A therapy in development: who is working on it and at which stage. Not a treatment recommendation."
            }
            Self::Designation => {
                "A regulator's decision about a medicine for a rare condition (for example an orphan designation)."
            }
            Self::Drug => "A medicine or compound studied for this condition or gene. Not a treatment recommendation.",
            Self::OutcomeMeasure => {
                "A way to measure change in this condition that studies have used or regulators have seen."
            }
            Self::FundingCall => "A funding programme that accepts research proposals on this topic.",
            Self::OrthologGene => "The matching gene in a model organism.",
            Self::Other => "A research resource.",
        }
    }
}

/// How a persona gets the asset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Access {
    /// `official_page` | `request_form` | `repository_order` | `access_committee` | `apply` |
    /// `contact_holder` | `public_download`.
    pub route: String,
    pub url: Option<String>,
    /// Verbatim access instruction or terms (MTA, eligibility), when the source gives one.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    /// Upstream CURIE (`CVCL_…`, `MGI:…`, `CHEMBL.COMPOUND:…`) or our id (`orgasset:…`, `funders:…`).
    pub id: String,
    pub label: String,
    pub kind: AssetKind,
    /// Biolink category or source record type, verbatim (`biolink:CellLine`, `orphan_designation`).
    pub category: String,
    /// Organisation node id of the holder, when it resolves to one.
    pub holder: Option<String>,
    /// Holder as the source names it (sponsor, repository, operator, funder).
    pub holder_name: Option<String>,
    pub access: Access,
    /// Verbatim source facts (`stage`, `modality`, `status`, `deadline`, `organism`, ...).
    pub facts: Vec<(String, String)>,
    /// Upstream page to check the record by hand.
    pub verify_url: Option<String>,
    /// May appear in the open bulk release (false for people-like or restricted records).
    pub release: bool,
    pub records: Vec<RecIdx>,
}

impl Asset {
    pub fn fact(&self, key: &str) -> Option<&str> {
        self.facts.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Typed source qualifiers retained in the existing snapshot facts field.
    pub fn access_context(&self) -> serde_json::Value {
        self.fact("access_context")
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}))
    }
}

/// D37 §3 licence classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenceClass {
    /// CC0, CC BY, public domain: shown and released.
    Open,
    ShareAlike,
    NonCommercial,
    #[default]
    Unknown,
}

impl LicenceClass {
    pub const ALL: [LicenceClass; 4] = [Self::Open, Self::ShareAlike, Self::NonCommercial, Self::Unknown];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ShareAlike => "share_alike",
            Self::NonCommercial => "non_commercial",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// Class of an SPDX id or licence text (conservative: anything unrecognised is `unknown`).
    pub fn classify(licence: &str) -> Self {
        let l = licence.to_ascii_lowercase();
        if l.contains("-nc")
            || l.contains("non-commercial")
            || l.contains("noncommercial")
            || l.contains("non commercial")
        {
            Self::NonCommercial
        } else if l.contains("-sa")
            || l.contains("sharealike")
            || l.contains("share-alike")
            || l.contains("share alike")
        {
            Self::ShareAlike
        } else if l.contains("cc0")
            || l.contains("cc-by")
            || l.contains("cc by")
            || l.contains("public domain")
            || l.contains("public-domain")
            || l.contains("pddl")
            || l.contains("odc-by")
        {
            Self::Open
        } else {
            Self::Unknown
        }
    }

    /// What the open release may carry (D35, D37 §3).
    pub fn release(self) -> &'static str {
        match self {
            Self::Open => "full",
            _ => "ids_and_links",
        }
    }
}

/// Licence of one source entity of the graph's provenance registry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityLicence {
    pub entity: EntityIdx,
    /// SPDX id or URL as the source states it.
    pub licence: String,
    pub class: LicenceClass,
}

/// A legal free-to-read link for a paper (Unpaywall / Europe PMC OA flags; never circumvention).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenAccess {
    /// Paper node id (`PMID:…`).
    pub paper: String,
    pub url: String,
    /// `gold` | `green` | `hybrid` | `bronze` | `pmc` | ...
    pub status: String,
    pub licence: Option<String>,
    pub record: RecIdx,
}

/// A source record held back from UI payloads, the open release and exports (fail-closed), e.g.
/// a page fetched around a site's bot protection. The record stays in the graph with provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quarantine {
    pub record: RecIdx,
    /// `fetched_after_block`, `block_status_unknown`, ...
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_classes() {
        assert_eq!(LicenceClass::classify("CC0-1.0"), LicenceClass::Open);
        assert_eq!(LicenceClass::classify("CC-BY-4.0"), LicenceClass::Open);
        assert_eq!(LicenceClass::classify("CC-BY-SA-3.0"), LicenceClass::ShareAlike);
        assert_eq!(LicenceClass::classify("CC-BY-NC-4.0"), LicenceClass::NonCommercial);
        assert_eq!(
            LicenceClass::classify("public domain (US government)"),
            LicenceClass::Open
        );
        assert_eq!(LicenceClass::classify("site terms"), LicenceClass::Unknown);
        assert_eq!(AssetKind::CellLine.job(), Some(Job::ModelsSamples));
        assert_eq!(Job::parse("free_papers"), Some(Job::FreePapers));
    }
}
