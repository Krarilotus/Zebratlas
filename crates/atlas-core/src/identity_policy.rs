//! Shared source-asserted identity policy.
use crate::curie::prefix;
use std::collections::HashMap;

/// Accepted source-asserted closure. Construct only from a verified completion manifest.
#[derive(Clone, Debug, Default)]
pub struct AcceptedIdentity {
    parents: HashMap<String, String>,
    pub manifest_sha256: String,
}

impl AcceptedIdentity {
    pub fn new(pairs: impl IntoIterator<Item = (String, String)>, manifest_sha256: String) -> Self {
        let mut s = Self {
            parents: HashMap::new(),
            manifest_sha256,
        };
        for (a, b) in pairs {
            let (a, b) = (crate::curie::normalize(&a), crate::curie::normalize(&b));
            let (ra, rb) = (s.root(&a).to_owned(), s.root(&b).to_owned());
            if ra != rb {
                s.parents.insert(rb, ra);
            }
        }
        s
    }
    fn root<'a>(&'a self, id: &'a str) -> &'a str {
        let mut current = id;
        while let Some(next) = self.parents.get(current) {
            if next == current {
                break;
            }
            current = next;
        }
        current
    }
    pub fn representative(&self, id: &str) -> String {
        if !id.contains(':') {
            return id.to_owned();
        }
        let id = crate::curie::normalize(id);
        self.root(&id).to_owned()
    }
    pub fn equivalent(&self, a: &str, b: &str) -> bool {
        let (a, b) = (crate::curie::normalize(a), crate::curie::normalize(b));
        self.root(&a) == self.root(&b)
    }
}
pub const VERSION: &str = "1.0.0";
pub const CODE: &str = include_str!("identity_policy.rs");
/// Source receipt stable across Git's Windows line-ending conversion.
pub fn code_sha256() -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(CODE.replace("\r\n", "\n").as_bytes()))
}
pub const RULE: &str = "R-ID-03";
pub const LEGACY_XREF: &str = "semapv:DatabaseCrossReference";

pub fn valid_orcid(value: &str) -> bool {
    let Some(s) = crate::withhold::normalise_orcid(value) else {
        return false;
    };
    if s != value {
        return false;
    }
    let digits: Vec<_> = s.bytes().filter(|b| *b != b'-').collect();
    let total = digits[..15].iter().fold(0u32, |a, b| (a + u32::from(b - b'0')) * 2);
    let check = (12 - total % 11) % 11;
    digits[15] == if check == 10 { b'X' } else { b'0' + check as u8 }
}

pub fn source_method(method: &str) -> &str {
    if method == LEGACY_XREF {
        "semapv:BackgroundKnowledgeBasedMatching"
    } else {
        method
    }
}

pub fn is_identity(predicate: &str) -> bool {
    matches!(predicate, "skos:exactMatch" | "owl:sameAs" | "owl:equivalentClass")
}

/// Conservative syntax screen, not DOI registration or attachment verification.
pub fn valid_doi(s: &str) -> bool {
    let Some((registrant, suffix)) = s.strip_prefix("10.").and_then(|s| s.split_once('/')) else {
        return false;
    };
    registrant.len() >= 4
        && registrant.bytes().all(|c| c.is_ascii_digit())
        && !suffix.is_empty()
        && !suffix.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Frozen subset of published SEMAPV terms used by this application; unknown terms fail closed.
pub fn known_justification(s: &str) -> bool {
    matches!(
        s,
        "semapv:BackgroundKnowledgeBasedMatching"
            | "semapv:UnspecifiedMatching"
            | "semapv:CompositeMatching"
            | "semapv:LexicalMatching"
            | "semapv:ManualMappingCuration"
            | "semapv:MappingChaining"
            | "semapv:LogicalReasoning"
            | "semapv:AgentBasedMatching"
            | "semapv:LLMBasedMatching"
            | "semapv:LexicalSimilarityThresholdMatching"
    )
}

pub fn regime(id: &str) -> Option<&'static str> {
    Some(match prefix(id) {
        "MONDO" | "ORPHA" | "OMIM" | "DOID" | "NCIT" | "GARD" | "UMLS" | "MEDGEN" | "MESH" | "ICD10CM" | "ICD10WHO"
        | "icd11.foundation" | "icd11.code" => "disease-concept",
        "OMIMPS" => "phenotypic-series",
        "HGNC" | "NCBIGene" | "ENSEMBL" => "human-gene",
        "UniProtKB" => "protein",
        "NCT" | "EUDRACT" | "EUCT" | "ISRCTN" | "ICTRP" | "JRCT" | "JAPICCTI" | "CTRI" | "DRKS" | "UMIN" | "ANZCTR"
        | "CHICTR" | "IRCT" | "NTR" | "KCT" | "PACTR" | "TCTR" | "RBR" | "OMON" => "trial-registration",
        "ROR" | "atlasorg" | "cordis.org" | "gtr.org" | "epicare-centre" | "nih.ic" | "crossref.funder"
        | "ctgov.org" | "reporter.org" => "organisation",
        "PMID" | "PMC" | "PMCID" | "DOI" | "PPR" | "AGR" | "CBA" | "ETH" | "CTX" | "HIR" => "work",
        "ORCID" | "OPENALEX.AUTHOR" | "REPORTER.PI" | "pubmed.author" | "epmc.author" | "openalex" | "reporter-pi"
        | "cordis-person" | "author-mention" => "person",
        "CHEMBL.COMPOUND" | "DRUGBANK" | "UNII" => "substance",
        // An RxCUI may be an ingredient or a product; only the versioned ingredient rule permits equality.
        "RXCUI" | "RXNORM" => "rxnorm-concept",
        "CVCL" | "hpscreg" | "RRID" => "cell-line",
        _ => return None,
    })
}

pub fn valid_curie(s: &str) -> bool {
    let Some((p, local)) = s.split_once(':') else {
        return false;
    };
    if p.is_empty() || local.is_empty() || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    match p {
        "DOI" => valid_doi(local),
        "HGNC" | "NCBIGene" | "PMID" | "MONDO" | "ORPHA" | "OMIM" | "OMIMPS" | "GARD" | "NCIT" | "DOID" | "RXNORM"
        | "RXCUI" => {
            let local = if p == "NCIT" {
                local.strip_prefix('C').unwrap_or("")
            } else {
                local
            };
            !local.is_empty() && local.bytes().all(|c| c.is_ascii_digit())
        }
        "ENSEMBL" => local
            .strip_prefix("ENSG")
            .is_some_and(|n| n.len() == 11 && n.bytes().all(|c| c.is_ascii_digit())),
        "ORCID" => valid_orcid(local),
        _ => true, // Other publishers have heterogeneous identifier grammars; no invented retirement check.
    }
}

/// Shared row policy for fresh producers and immutable intake. Structural checks follow this gate.
pub fn eligibility(
    subject: &str,
    object: &str,
    justification: &str,
    conflict: &str,
    rule_id: &str,
) -> Result<(), &'static str> {
    if !conflict.trim().is_empty() {
        return Err("source_conflict");
    }
    if !known_justification(justification) {
        return Err("unknown_justification");
    }
    if !valid_curie(subject) || !valid_curie(object) {
        return Err("invalid_identifier");
    }
    let (Some(a), Some(b)) = (regime(subject), regime(object)) else {
        return Err("unknown_entity_regime");
    };
    if a != b && !(rule_id == "R-DRG-02" && [a, b].contains(&"rxnorm-concept") && [a, b].contains(&"substance")) {
        return Err("incompatible_entity_regime");
    }
    if a == "person" && prefix(subject) != "ORCID" && prefix(object) != "ORCID" {
        return Err("person_without_orcid");
    }
    // Historical organisation rules used composite evidence or an API match/inspection score.
    // A vocabulary relabel cannot rehabilitate those assertions as source equivalence.
    if matches!(rule_id, "R-ORG-01" | "R-ORG-03" | "R-ORG-05" | "R-ORG-06" | "R-ORG-07") {
        return Err("organisation_without_source_equivalence");
    }
    if !matches!(
        justification,
        "semapv:BackgroundKnowledgeBasedMatching" | "semapv:ManualMappingCuration"
    ) {
        return Err("candidate_method");
    }
    Ok(())
}
