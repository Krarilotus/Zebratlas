use super::*;
use atlas_core::node::NodeKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkedEntity {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Traverse,
    GroupsInBranch,
    StudiesAcrossIdentities,
    SourceChain,
    SharedMechanismModels,
    FundingForGene,
    AssociatedConditions,
    GeneRecord,
    ReleaseCoverage,
    MissingEvidence,
    ReleaseInputs,
}
pub const SEEDS: [(&str, &str); 10] = [
    (
        "groups_in_branch",
        include_str!("../../../../docs/sparql/01-patient-groups-in-branch.rq"),
    ),
    (
        "studies_across_identities",
        include_str!("../../../../docs/sparql/02-trials-for-gene-across-identities.rq"),
    ),
    (
        "source_chain",
        include_str!("../../../../docs/sparql/03-edge-to-source-chain.rq"),
    ),
    (
        "shared_mechanism_models",
        include_str!("../../../../docs/sparql/04-models-for-shared-mechanism.rq"),
    ),
    (
        "funding_for_gene",
        include_str!("../../../../docs/sparql/05-funding-for-condition.rq"),
    ),
    (
        "associated_conditions",
        include_str!("../../../../docs/sparql/06-conditions-associated-with-stxbp1.rq"),
    ),
    (
        "gene_record",
        include_str!("../../../../docs/sparql/07-check-gene-name-and-provenance.rq"),
    ),
    (
        "release_coverage",
        include_str!("../../../../docs/sparql/08-what-is-withheld.rq"),
    ),
    (
        "missing_evidence",
        include_str!("../../../../docs/sparql/09-missing-evidence-fields.rq"),
    ),
    (
        "release_inputs",
        include_str!("../../../../docs/sparql/10-reproduce-release-inputs.rq"),
    ),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Outgoing,
    Incoming,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hop {
    pub relation: String,
    pub direction: Direction,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filters {
    pub country: Option<String>,
    pub recruiting: Option<bool>,
    pub kind: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryPlan {
    pub version: u32,
    pub pattern: Pattern,
    pub focus: Vec<String>,
    pub hops: Vec<Hop>,
    pub filters: Filters,
    pub output: Option<NodeKind>,
    pub limit: usize,
    pub reasoning: bool,
}

impl QueryPlan {
    pub fn schema(linked: &[LinkedEntity], relations: &BTreeSet<String>) -> Value {
        let focus_items = if linked.is_empty() {
            json!({"type":"string"})
        } else {
            json!({"enum":linked.iter().map(|e|&e.id).collect::<Vec<_>>()})
        };
        json!({"type":"object","additionalProperties":false,
            "required":["version","pattern","focus","hops","filters","output","limit","reasoning"],
            "properties":{
            "version":{"const":1},
            "pattern":{"enum":["traverse","groups_in_branch","studies_across_identities","source_chain","shared_mechanism_models","funding_for_gene","associated_conditions","gene_record","release_coverage","missing_evidence","release_inputs"]},
            "focus":{"type":"array","maxItems":if linked.is_empty(){0}else{8},"uniqueItems":true,"items":focus_items},
            "hops":{"type":"array","maxItems":4,"items":{"type":"object","additionalProperties":false,"required":["relation","direction"],"properties":{"relation":{"enum":relations},"direction":{"enum":["outgoing","incoming"]}}}},
            "filters":{"type":"object","additionalProperties":false,"required":["country","recruiting","kind"],"properties":{"country":{"type":["string","null"],"maxLength":100},"recruiting":{"type":["boolean","null"]},"kind":{"type":["string","null"],"maxLength":100}}},
            "output":{"enum":[null,"disease","gene","phenotype","pathway","paper","study","grant","person","organisation","asset"]},
            "limit":{"type":"integer","minimum":1,"maximum":100},"reasoning":{"type":"boolean"}}})
    }

    pub fn validate(&self, linked: &[LinkedEntity], relations: &BTreeSet<String>) -> Result<(), String> {
        if self.version != 1 || !(1..=100).contains(&self.limit) || self.hops.len() > 4 || self.focus.len() > 8 {
            return Err("invalid plan version or limits".into());
        }
        if self.focus.iter().any(|id| !linked.iter().any(|e| &e.id == id)) {
            return Err("focus must use pre-linked graph identifiers".into());
        }
        if self.focus.iter().collect::<BTreeSet<_>>().len() != self.focus.len() {
            return Err("duplicate focus identifiers".into());
        }
        if self.hops.iter().any(|h| !relations.contains(&h.relation)) {
            return Err("unknown relation type".into());
        }
        if self.pattern == Pattern::Traverse {
            if self.focus.is_empty() || self.hops.is_empty() {
                return Err("traversal requires focus and hops".into());
            }
        } else {
            if !self.hops.is_empty()
                || self.output.is_some()
                || self.filters.country.is_some()
                || self.filters.recruiting.is_some()
                || self.filters.kind.is_some()
            {
                return Err("seed patterns do not accept traversal fields; use traverse".into());
            }
            let global = matches!(
                self.pattern,
                Pattern::ReleaseCoverage | Pattern::MissingEvidence | Pattern::ReleaseInputs
            );
            if self.focus.len() != usize::from(!global) {
                return Err("seed pattern requires one focus (global patterns require none)".into());
            }
            if !global {
                let id = &self.focus[0];
                let valid = match self.pattern {
                    Pattern::GroupsInBranch => id.starts_with("MONDO:"),
                    Pattern::SourceChain => id.split('|').count() == 3,
                    _ => id.starts_with("HGNC:"),
                };
                if !valid {
                    return Err("focus has wrong identifier type for seed pattern".into());
                }
            }
        }
        if [self.filters.country.as_ref(), self.filters.kind.as_ref()]
            .into_iter()
            .flatten()
            .any(|s| s.len() > 100)
        {
            return Err("filter too long".into());
        }
        Ok(())
    }

    pub fn compile_sparql(&self, linked: &[LinkedEntity], relations: &BTreeSet<String>) -> Result<String, String> {
        self.compile_sparql_with_metadata(linked, relations, &BTreeSet::new())
    }

    /// Metadata filters require an observed property in this exact loaded schema.
    /// Older releases keep their explicit unsupported-filter behavior.
    pub fn compile_sparql_with_metadata(
        &self,
        linked: &[LinkedEntity],
        relations: &BTreeSet<String>,
        observed_predicates: &BTreeSet<String>,
    ) -> Result<String, String> {
        self.validate(linked, relations)?;
        if self.pattern != Pattern::Traverse {
            let name = serde_json::to_value(self.pattern).unwrap();
            let mut query = SEEDS
                .iter()
                .find(|(n, _)| Some(*n) == name.as_str())
                .unwrap()
                .1
                .to_owned();
            if let Some(id) = self.focus.first() {
                let old = match self.pattern {
                    Pattern::GroupsInBranch => iri("id", "MONDO:0005027"),
                    Pattern::SourceChain => iri("edge", "MONDO:0000023|has_associated_gene|HGNC:15625"),
                    _ => iri("id", "HGNC:11444"),
                };
                let kind = if self.pattern == Pattern::SourceChain {
                    "edge"
                } else {
                    "id"
                };
                query = query.replace(&old, &iri(kind, id));
                if self.pattern == Pattern::GeneRecord {
                    // Identity constrains the query, not the client's supplied display label.
                    query = query.replace(
                        "?gene rdfs:label \"STXBP1\"",
                        &format!("VALUES ?gene {{ {} }}\n  ?gene rdfs:label ?label", iri("id", id)),
                    );
                }
            }
            return Ok(query);
        }
        if self.filters.country.is_some() && !observed_predicates.contains(&format!("{RA}country")) {
            return Err("country filter is not projected into this RDF dataset".into());
        }
        if self.filters.kind.is_some() && !observed_predicates.contains(&format!("{RA}kind")) {
            return Err("kind filter is not projected into this RDF dataset".into());
        }
        let mut body = format!(
            "VALUES ?n0 {{ {} }}\n",
            self.focus.iter().map(|id| iri("id", id)).collect::<Vec<_>>().join(" ")
        );
        for (i, h) in self.hops.iter().enumerate() {
            let (a, b) = if h.direction == Direction::Outgoing {
                (i, i + 1)
            } else {
                (i + 1, i)
            };
            body.push_str(&format!("?n{a} ra:{} ?n{b} .\n", h.relation));
        }
        let last = self.hops.len();
        if let Some(k) = self.output {
            body.push_str(&format!("?n{last} ra:nodeKind {} .\n", literal(k.as_str())));
        }
        if let Some(recruiting) = self.filters.recruiting {
            body.push_str(&format!("?n{last} ra:status ?status . FILTER({} (?status IN (\"RECRUITING\",\"NOT_YET_RECRUITING\",\"ENROLLING_BY_INVITATION\")))\n",if recruiting {""}else{"!"}));
        }
        if let Some(country) = &self.filters.country {
            body.push_str(&format!("?n{last} ra:country {} .\n", literal(country)));
        }
        if let Some(kind) = &self.filters.kind {
            body.push_str(&format!(
                "FILTER(EXISTS {{ ?n{last} ra:kind {} }} || EXISTS {{ ?n{last} ra:nodeKind {} }})\n",
                literal(kind),
                literal(kind)
            ));
        }
        body.push_str(&format!(
            "BIND(?n{last} AS ?result) OPTIONAL {{ ?result rdfs:label ?label }}\n"
        ));
        Ok(format!(
            "{PREFIXES}SELECT DISTINCT ?result ?label WHERE {{ {body} }} ORDER BY ?result LIMIT {}",
            self.limit + 1
        ))
    }
}
