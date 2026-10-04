//! Gene2Phenotype gene-disease models as the mechanism layer uses them: variant effect as a closed
//! set and the atlas conditions each model resolves to. Parsed and resolved in
//! `atlas_ingest::mechanism::g2p`.

use serde::{Deserialize, Serialize};

use crate::provenance::RecordRef;

/// Molecular mechanism (variant effect) as a closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Effect {
    #[serde(rename = "LoF")]
    LoF,
    #[serde(rename = "GoF")]
    GoF,
    #[serde(rename = "DN")]
    DN,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoF => "LoF",
            Self::GoF => "GoF",
            Self::DN => "DN",
        }
    }

    /// G2P `molecular mechanism` column; `undetermined*` -> None.
    pub fn from_g2p(value: &str) -> Option<Self> {
        match value {
            "loss of function" => Some(Self::LoF),
            "gain of function" => Some(Self::GoF),
            "dominant negative" => Some(Self::DN),
            _ => None,
        }
    }

    /// Orphanet association type (as `variant_effect` in the prototype: LoF / GoF only).
    pub fn from_orphanet(association: &str) -> Option<Self> {
        if association.contains("loss of function") {
            Some(Self::LoF)
        } else if association.contains("gain of function") {
            Some(Self::GoF)
        } else {
            None
        }
    }

    /// Loss vs gain of function (DN counts as loss): opposite directions of protein activity.
    /// LoF vs DN is "different" but not opposite.
    pub fn opposite(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::LoF | Self::DN, Self::GoF) | (Self::GoF, Self::LoF | Self::DN)
        )
    }
}

/// G2P confidence levels that count as an established gene-disease model.
pub const ESTABLISHED: [&str; 3] = ["definitive", "strong", "moderate"];

/// One G2P gene-disease model with its resolution to atlas conditions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct G2pRecord {
    pub g2p_id: String,
    pub symbol: String,
    pub hgnc_id: Option<String>,
    pub disease_name: String,
    /// MONDO / OMIM ids G2P gives as "same or highly similar disease", as written.
    pub disease_ids: Vec<String>,
    /// Active canonical atlas diseases these ids resolve to (identity layer; the prototype's rule).
    pub diseases: Vec<String>,
    /// The atlas node built for exactly this model (`G2P:<id>`, a newly described condition that no
    /// OMIM/Orphanet/MONDO node covers), if any. Takes precedence over `diseases`.
    pub node: Option<String>,
    pub allelic_requirement: String,
    /// definitive / strong / moderate / limited / disputed / refuted.
    pub confidence: String,
    pub mechanism_raw: String,
    pub mechanism: Option<Effect>,
    /// evidence / inferred.
    pub mechanism_support: String,
    pub variant_consequence: String,
    pub publications: Vec<String>,
    pub panels: Vec<String>,
    pub reviewed: String,
    pub record: RecordRef,
}

impl G2pRecord {
    pub fn established(&self) -> bool {
        ESTABLISHED.contains(&self.confidence.as_str())
    }

    /// Public G2P page of the record.
    pub fn url(&self) -> String {
        format!("https://www.ebi.ac.uk/gene2phenotype/lgd/{}", self.g2p_id)
    }

    /// Atlas conditions the model is filed under: its own node, else the resolved MONDO/OMIM ids.
    pub fn conditions(&self) -> &[String] {
        match &self.node {
            Some(n) => std::slice::from_ref(n),
            None => &self.diseases,
        }
    }
}
