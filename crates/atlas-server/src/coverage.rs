//! Subject-scoped coverage: loading a gene cache does not mean every rare condition was searched.
use atlas_core::graph::RecordWithhold;
use atlas_core::{Atlas, DiseaseIdx, Graph};
use serde_json::{Value, json};

pub fn condition(atlas: &Atlas, graph: &Graph, d: DiseaseIdx) -> Value {
    let disease = atlas.disease_at(d);
    let genes = crate::nodes::causal_genes(atlas, d);
    let scoped = graph.coverage().iter().any(|c| {
        c.status == "loaded"
            && c.header_checksums_failed == 0
            && c.genes.iter().any(|g| genes.iter().any(|(_, symbol, _)| symbol == g))
    });
    // Trial data are global; their presence alone doesn't establish a researched neighbourhood.
    let curated = std::iter::once(disease.id.as_str())
        .chain(genes.iter().map(|(id, _, _)| id.as_str()))
        .any(|id| {
            graph.incident(id).any(|i| {
                matches!(
                    i.edge.relation,
                    atlas_core::graph::Relation::ServesCondition | atlas_core::graph::Relation::ServesGene
                ) && graph.records_withheld(&i.edge.records).is_none()
                    && graph.node(i.other).is_some_and(|k| graph.node_withheld(k).is_none())
            })
        });
    let known = scoped || curated;
    let label = &disease.name;
    let encoded = crate::nodes::encode(label);
    json!({
        "status": if known { "cached_evidence" } else { "outside_verified_neighbourhood" },
        "message": (!known).then(|| json!({ "key": "coverage.outside_verified_neighbourhood", "params": {},
            "fallback": "We have limited verified evidence for this condition. Other research may exist." })),
        "discovery_routes": [
            {"source":"ClinicalTrials.gov", "url":format!("https://clinicaltrials.gov/search?cond={encoded}")},
            {"source":"Europe PMC", "url":format!("https://europepmc.org/search?query={encoded}")},
            {"source":"CORDIS", "url":format!("https://cordis.europa.eu/search?q={encoded}")},
            {"source":"Orphanet", "url":"https://www.orpha.net/en"},
            {"source":"GARD", "url":"https://rarediseases.info.nih.gov/"},
            {"source":"NIH RePORTER", "method":"POST", "url":"https://api.reporter.nih.gov/v2/projects/search",
                "body":{"criteria":{"advanced_text_search":{"operator":"and", "search_field":"projecttitle,terms,abstracttext",
                    "search_text":label}}, "limit":100}},
            {"source":"OpenAlex", "url":format!("https://api.openalex.org/works?search={encoded}"),
                "requires_api_key":true}
        ],
        "discovery_routes_checked": false,
    })
}

/// A source's declared gene scope matches the subject, rather than merely being loaded.
/// This is a local index check, never a claim that a discovery link was fetched upstream.
pub fn scoped(c: &atlas_core::graph::Coverage, genes: &[String]) -> bool {
    c.status == "loaded" && c.header_checksums_failed == 0 && c.genes.iter().any(|g| genes.contains(g))
}

pub fn resolution(atlas: &Atlas, graph: &Graph, body: &mut Value) {
    if let Some(choices) = body["choices"].as_array_mut() {
        for choice in choices {
            if let Some(d) = choice["node"]["id"].as_str().and_then(|id| atlas.disease_idx(id)) {
                choice["coverage"] = condition(atlas, graph, d);
            }
        }
    }
    if let Some(d) = body["target"]["id"].as_str().and_then(|id| atlas.disease_idx(id)) {
        body["coverage"] = condition(atlas, graph, d);
    }
}

/// Both legacy search results and the filtered search items expose the same condition scope.
pub fn search(atlas: &Atlas, graph: &Graph, body: &mut Value) {
    for field in ["results", "items"] {
        if let Some(entries) = body[field].as_array_mut() {
            for entry in entries {
                if let Some(d) = entry["node"]["id"].as_str().and_then(|id| atlas.disease_idx(id)) {
                    entry["coverage"] = condition(atlas, graph, d);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_source_is_not_a_check_for_every_condition() {
        let mut c = atlas_core::graph::Coverage {
            status: "loaded".into(),
            genes: vec!["STXBP1".into()],
            ..Default::default()
        };
        assert!(scoped(&c, &["STXBP1".into()]));
        assert!(!scoped(&c, &["TCF4".into()]));
        c.header_checksums_failed = 1;
        assert!(!scoped(&c, &["STXBP1".into()]));
    }
}
