//! OBO term model (HPO, MONDO): only the tags the atlas uses.

use serde::{Deserialize, Serialize};

/// Synonym scope. Unknown scopes read as `Related` (only `Exact` is ever tested).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Exact,
    Related,
    Broad,
    Narrow,
}

impl Scope {
    /// From the OBO token (`EXACT`, `RELATED`, ...).
    pub fn parse(token: &str) -> Self {
        match token {
            "EXACT" => Self::Exact,
            "BROAD" => Self::Broad,
            "NARROW" => Self::Narrow,
            _ => Self::Related,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Synonym {
    pub text: String,
    pub scope: Scope,
    /// Synonym type, e.g. `layperson`, `ABBREVIATION`, `uk_spelling`.
    pub kind: Option<String>,
}

impl Synonym {
    /// Synonym type equals `kind`, ignoring case (HPO `abbreviation` vs MONDO `ABBREVIATION`).
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(kind))
    }
}

/// Cross-reference with the provenance sources of its axiom (`MONDO:equivalentTo`, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Xref {
    pub id: String,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Term {
    pub id: String,
    pub name: String,
    pub definition: String,
    pub synonyms: Vec<Synonym>,
    /// `is_a` only.
    pub parents: Vec<String>,
    /// One entry per xref id; a repeated id replaces the earlier sources.
    pub xrefs: Vec<Xref>,
    pub alt_ids: Vec<String>,
    pub subsets: Vec<String>,
    pub obsolete: bool,
    pub replaced_by: Option<String>,
}

impl Term {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    pub fn in_subset(&self, subset: &str) -> bool {
        self.subsets.iter().any(|s| s == subset)
    }
}
