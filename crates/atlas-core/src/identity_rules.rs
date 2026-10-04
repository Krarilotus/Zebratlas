//! Versioned rules shared by alignment writers and the fail-closed graph reader.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const REGISTRY_JSON: &str = include_str!("../../../data/cache/mappings/rules.json");
pub const VERSION: &str = "1.0.0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityRule {
    pub id: String,
    pub version: String,
    pub statement: String,
    pub predicates: Vec<String>,
    pub sets: Vec<String>,
}

pub fn registry() -> &'static [IdentityRule] {
    static RULES: OnceLock<Vec<IdentityRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(REGISTRY_JSON).expect("checked-in rule registry");
        serde_json::from_value(v["rules"].clone()).expect("checked-in rule entries")
    })
}

pub fn rule(id: &str, version: &str) -> Option<&'static IdentityRule> {
    registry().iter().find(|r| r.id == id && r.version == version)
}

/// Select the implemented producer branch, including its later demotions. No fallback rule.
pub fn select(set: &str, subject: &str, object: &str, justification: &str, asserted: &str) -> Option<&'static str> {
    let lexical = justification == "semapv:LexicalMatching";
    Some(match set {
        "disease-xrefs" if object.starts_with("GARD:") && asserted == "skos:closeMatch" => "R-DIS-03",
        "disease-xrefs" => "R-DIS-01",
        "disease-orphanet-xrefs" => "R-DIS-02",
        "gard-xrefs" if asserted == "skos:closeMatch" => "R-DIS-05",
        "gard-xrefs" => "R-DIS-04",
        "gene-xrefs" if object.starts_with("UniProtKB:") => "R-GEN-02",
        "gene-xrefs" => "R-GEN-01",
        "trial-xrefs" if lexical => "R-TRI-02",
        "trial-xrefs" => "R-TRI-01",
        "org-ror" if lexical => "R-ORG-02",
        "org-ror" => "R-ORG-01",
        "funder-ror" if lexical => "R-ORG-02",
        "funder-ror" => "R-FUN-01",
        "org-ror-affiliation" if lexical => "R-ORG-04",
        "org-ror-affiliation" => "R-ORG-03",
        "work-ids" => "R-WRK-01",
        "drug-xrefs" => "R-DRG-01",
        "rxnorm-xrefs" => "R-DRG-02",
        "researcher-orcid" if justification == "semapv:CompositeMatching" => "R-PER-02",
        "researcher-orcid" => "R-PER-01",
        "assets-cell-identifiers" if object.starts_with("RRID:") => "R-CELL-01",
        "assets-cell-identifiers" if subject.starts_with("CVCL:") && object.starts_with("hpscreg:") => "R-CELL-02",
        "alliance-human-orthologs" => "R-ORTH-01",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_rules_are_unique_versioned_and_never_guess_unknown_sets() {
        let ids: std::collections::HashSet<_> = registry().iter().map(|r| &r.id).collect();
        assert_eq!(ids.len(), registry().len());
        assert!(
            registry()
                .iter()
                .all(|r| r.version == VERSION && !r.statement.is_empty())
        );
        assert_eq!(select("unknown", "A:1", "B:1", "", ""), None);
        assert!(
            rule("R-TRI-02", VERSION)
                .unwrap()
                .predicates
                .iter()
                .all(|p| p != "skos:exactMatch")
        );
    }
}
