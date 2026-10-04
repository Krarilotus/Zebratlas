//! Disease node: identity, names, typed edges with their evidence, and its provenance.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::evidence::{GeneLink, PhenotypeAnnotation, Prevalence};
use crate::identity::IdLink;
use crate::ontology::TermIdx;
use crate::provenance::{ActivityIdx, RecordRef};

/// Classification property of conditions created from a G2P record.
pub const NEWLY_DESCRIBED: &str = "newly_described";

/// Dense index into [`crate::Atlas::diseases`].
pub type DiseaseIdx = u32;

/// Retired nodes stay in the graph but are left out of analytics, IC and search by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Active,
    Retired,
}

/// A decision about a node (`status=retired`, `rare=false`), with the activity and reason.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Classification {
    pub property: String,
    pub value: String,
    pub reason: String,
    pub activity: ActivityIdx,
    pub record: Option<RecordRef>,
}

/// A name or synonym with its type (`ABBREVIATION`, ...) where the source gives one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Name {
    pub text: String,
    pub kind: Option<String>,
}

/// Phenotype edge: canonical HPO term and every annotation row behind it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhenotypeEdge {
    pub term: TermIdx,
    pub annotations: Vec<PhenotypeAnnotation>,
}

/// Attribute value -> records asserting it (inheritance, onset, clinical course).
pub type Attributes = BTreeMap<String, Vec<RecordRef>>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Disease {
    pub id: String,
    pub name: String,
    pub definition: String,
    pub status: Status,
    /// Source ids merged into this node (as written in the source).
    pub source_ids: BTreeSet<String>,
    /// Distinct names and synonyms, first occurrence wins.
    pub synonyms: Vec<Name>,
    pub rare: bool,
    pub parents: Vec<String>,
    /// Present phenotypes, in first-seen order.
    pub phenotypes: Vec<PhenotypeEdge>,
    /// NOT annotations: documented as absent. Never mixed with "not recorded".
    pub excluded: Vec<PhenotypeEdge>,
    pub inheritance: Attributes,
    pub clinical_course: Attributes,
    pub onset: Attributes,
    pub genes: Vec<GeneLink>,
    pub prevalence: Vec<Prevalence>,
    pub related: Vec<IdLink>,
    /// `prov:wasGeneratedBy`: the activity that created the node.
    pub generated_by: ActivityIdx,
    /// `prov:wasDerivedFrom`: source records that contributed the node (deduplicated).
    pub derived_from: Vec<RecordRef>,
    pub classifications: Vec<Classification>,
}

impl Disease {
    pub fn new(id: impl Into<String>, generated_by: ActivityIdx) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            definition: String::new(),
            status: Status::Active,
            source_ids: BTreeSet::new(),
            synonyms: Vec::new(),
            rare: false,
            parents: Vec::new(),
            phenotypes: Vec::new(),
            excluded: Vec::new(),
            inheritance: Attributes::new(),
            clinical_course: Attributes::new(),
            onset: Attributes::new(),
            genes: Vec::new(),
            prevalence: Vec::new(),
            related: Vec::new(),
            generated_by,
            derived_from: Vec::new(),
            classifications: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }

    /// A gene-specific condition defined only by Gene2Phenotype (no OMIM/Orphanet/MONDO node yet).
    pub fn is_newly_described(&self) -> bool {
        self.classifications
            .iter()
            .any(|c| c.property == NEWLY_DESCRIBED && c.value == "true")
    }

    /// Classification value of `property` (`confidence` of a newly described condition, ...).
    pub fn classification(&self, property: &str) -> Option<&str> {
        self.classifications
            .iter()
            .find(|c| c.property == property)
            .map(|c| c.value.as_str())
    }

    /// Add a name unless the same text is already there.
    pub fn add_name(&mut self, text: &str, kind: Option<&str>) {
        if !self.synonyms.iter().any(|n| n.text == text) {
            self.synonyms.push(Name {
                text: text.to_owned(),
                kind: kind.map(str::to_owned),
            });
        }
    }

    pub fn derive_from(&mut self, record: RecordRef) {
        if !self.derived_from.contains(&record) {
            self.derived_from.push(record);
        }
    }

    /// File a phenotype row under its canonical term; NOT rows go to `excluded`.
    pub fn annotate(&mut self, term: TermIdx, annotation: PhenotypeAnnotation) {
        let edges = if annotation.negated {
            &mut self.excluded
        } else {
            &mut self.phenotypes
        };
        match edges.iter_mut().find(|e| e.term == term) {
            Some(e) => e.annotations.push(annotation),
            None => edges.push(PhenotypeEdge {
                term,
                annotations: vec![annotation],
            }),
        }
    }

    pub fn phenotype(&self, term: TermIdx) -> Option<&PhenotypeEdge> {
        self.phenotypes.iter().find(|e| e.term == term)
    }

    pub fn excluded_phenotype(&self, term: TermIdx) -> Option<&PhenotypeEdge> {
        self.excluded.iter().find(|e| e.term == term)
    }
}

/// Record `value` as asserted by `record`.
pub fn add_attribute(attrs: &mut Attributes, value: &str, record: RecordRef) {
    let records = attrs.entry(value.to_owned()).or_default();
    if !records.contains(&record) {
        records.push(record);
    }
}
