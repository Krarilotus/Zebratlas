//! `/api/resolve` adapts the shared core ranker: three legacy choices plus all ranked matches.
//! Only unique authoritative lexical identities resolve automatically. Research leads stay choices.

use std::collections::HashSet;

use atlas_core::node::{NodeKey, NodeKind, NodeRef};
use atlas_core::search::SearchOptions;
use atlas_core::search::domain::{Index, Reason};
use atlas_core::{Atlas, DiseaseIdx, Graph};
use serde_json::{Value, json};

pub const MAX_CHOICES: usize = 3;

#[derive(Clone, Debug)]
pub struct Hit {
    pub node: NodeKey,
    pub matched: String,
    pub match_kind: &'static str,
}

#[derive(Debug, Default)]
pub struct Resolution {
    pub hits: Vec<Hit>,
    pub corrected: Option<String>,
    pub mention: Option<String>,
    /// Conditions with how they were reached: (condition, via gene symbol).
    pub conditions: Vec<(DiseaseIdx, Option<String>, &'static str, String)>,
    /// A Wikidata name led here (label or alias in any language): shown as "matched via Wikidata name".
    pub wikidata: bool,
    pub ranked: Vec<atlas_core::search::domain::Hit>,
}

/// Active conditions a gene *causes*. A gene whose only links are non-causal (Orphanet `UNKNOWN`
/// in a contiguous deletion, candidate, modifier) leads to no condition: it opens the gene page.
fn gene_conditions(atlas: &Atlas, g: u32) -> Vec<DiseaseIdx> {
    let gene = atlas.gene_at(g);
    let causal = |d: &DiseaseIdx| {
        atlas
            .disease_at(*d)
            .genes
            .iter()
            .any(|l| l.symbol == gene.symbol && l.is_causal())
    };
    let mut out: Vec<DiseaseIdx> = gene.diseases.iter().copied().filter(causal).collect();
    // MONDO nodes first (the merged identities), then the richest annotation
    out.sort_by_key(|d| {
        let x = atlas.disease_at(*d);
        (!x.id.starts_with("MONDO:"), std::cmp::Reverse(x.phenotypes.len()), *d)
    });
    out
}

pub fn resolve(atlas: &Atlas, graph: &Graph, index: &Index, q: &str) -> Resolution {
    from_hits(
        atlas,
        q,
        index.search(
            atlas,
            graph,
            q,
            SearchOptions {
                limit: 100,
                include_retired: false,
            },
        ),
    )
}

pub fn from_hits(atlas: &Atlas, _q: &str, ranked: Vec<atlas_core::search::domain::Hit>) -> Resolution {
    let mut r = Resolution {
        ranked: ranked.clone(),
        ..Resolution::default()
    };
    let mut seen = HashSet::new();
    for h in ranked {
        // Hierarchy, neighbourhood and mechanism expansions are research leads, never identity.
        if matches!(
            h.reason,
            Reason::BroaderCondition | Reason::NarrowerCondition | Reason::Neighbour | Reason::Mechanism
        ) {
            continue;
        }
        r.wikidata |= matches!(h.match_kind, "wikidata_label" | "wikidata_alias");
        if h.reason == Reason::Mention {
            r.mention.get_or_insert(h.matched.clone());
        }
        if h.reason == Reason::Typo {
            r.corrected.get_or_insert(h.matched.clone());
        }
        match h.key.kind {
            NodeKind::Disease if h.reason != Reason::CausalGene => {
                if seen.insert(h.key.idx) {
                    r.conditions.push((h.key.idx, None, h.match_kind, h.matched.clone()));
                }
            }
            NodeKind::Gene if h.reason != Reason::Words => {
                let symbol = atlas.gene_at(h.key.idx).symbol.clone();
                for d in gene_conditions(atlas, h.key.idx) {
                    if seen.insert(d) {
                        r.conditions
                            .push((d, Some(symbol.clone()), h.match_kind, h.matched.clone()));
                    }
                }
            }
            _ => {}
        }
        // Connected entities remain available through the additive ranked_matches field.
        if matches!(h.key.kind, NodeKind::Disease | NodeKind::Gene | NodeKind::Phenotype) {
            r.hits.push(Hit {
                node: h.key,
                matched: h.matched.clone(),
                match_kind: h.match_kind,
            });
        }
    }
    // Core search orders gene-linked conditions using symptoms; use that ordering in choices.
    let order: std::collections::HashMap<_, _> = r
        .ranked
        .iter()
        .enumerate()
        .filter(|(_, h)| h.key.kind == NodeKind::Disease)
        .map(|(i, h)| (h.key.idx, i))
        .collect();
    r.conditions
        .sort_by_key(|c| order.get(&c.0).copied().unwrap_or(usize::MAX));
    r
}

/// A lexical identity must be unique. Similar symptoms and partial names always stay choices.
pub fn certain(r: &Resolution) -> bool {
    if r.corrected.is_some() || r.conditions.is_empty() {
        return false;
    }
    let direct: Vec<_> = r
        .ranked
        .iter()
        .filter(|h| h.strong && h.key.kind == NodeKind::Disease)
        .collect();
    if direct.len() == 1 {
        // Other exact names/aliases may still name a different condition.
        return !r.ranked.iter().any(|h| {
            h.key.kind == NodeKind::Disease
                && h.key != direct[0].key
                && matches!(
                    h.reason,
                    Reason::ExactId
                        | Reason::ExactName
                        | Reason::Synonym
                        | Reason::Abbreviation
                        | Reason::TranslatedName
                        | Reason::TranslatedAlias
                )
        });
    }
    r.conditions.len() == 1
        && r.ranked.iter().any(|h| h.key.kind == NodeKind::Gene && h.strong)
        && !r.ranked.iter().any(|h| {
            h.key.kind == NodeKind::Disease
                && h.key.idx != r.conditions[0].0
                && matches!(
                    h.reason,
                    Reason::Words | Reason::Mention | Reason::Prefix | Reason::Phenotypes | Reason::TranslatedAlias
                )
        })
}

#[cfg(test)]
fn symptom_only(r: &Resolution) -> bool {
    r.ranked.iter().any(|h| h.reason == Reason::Phenotypes)
        && !r.ranked.iter().any(|h| {
            (h.key.kind == NodeKind::Disease && h.strong) || (h.key.kind == NodeKind::Gene && h.reason != Reason::Words)
        })
}

pub fn node_json(atlas: &Atlas, d: DiseaseIdx) -> NodeRef {
    atlas.disease_ref(d)
}

/// Response body without the LLM part.
pub fn body(atlas: &Atlas, q: &str, lang: &str, r: &Resolution) -> Value {
    let choices: Vec<Value> = r
        .conditions
        .iter()
        .take(MAX_CHOICES)
        .map(|(d, via, kind, matched)| {
            json!({
                "node": node_json(atlas, *d),
                "matched": matched,
                "match_kind": kind,
                "via_gene": via,
                "why": r.ranked.iter().find(|h| h.key.kind == NodeKind::Disease && h.key.idx == *d)
                    .map(|h| h.why.clone()).unwrap_or_else(|| via.as_ref().map(|g| format!("Linked to {g}")).unwrap_or_else(|| "Possible name match".into())),
                "evidence": r.ranked.iter().find(|h| h.key.kind == NodeKind::Disease && h.key.idx == *d).map(|h| &h.evidence),
                "synonyms": atlas.disease_at(*d).synonyms.iter().take(5).map(|n| &n.text).collect::<Vec<_>>(),
                "newly_described": atlas.disease_at(*d).is_newly_described(),
                "confidence": atlas.disease_at(*d).classification("confidence"),
            })
        })
        .collect();
    let gene = r.hits.iter().find(|h| h.node.kind == NodeKind::Gene);
    let status = match (r.conditions.is_empty(), gene) {
        (true, Some(_)) => "gene",
        (true, None) => "none",
        _ if certain(r) => "resolved",
        _ => "ambiguous",
    };
    let target = match status {
        "resolved" => Some(node_json(atlas, r.conditions[0].0)),
        "gene" => gene.map(|g| atlas.node_ref(g.node)),
        _ => None,
    };
    let hits: Vec<Value> = r
        .hits
        .iter()
        .take(10)
        .map(|h| json!({ "node": atlas.node_ref(h.node), "matched": h.matched, "match_kind": h.match_kind }))
        .collect();
    // the gene community is always offered when a gene led here (G2P/OMIM may map a gene to a
    // condition whose scope differs, e.g. SNAP25 -> CMS18; the gene page lists every condition)
    let gene_choice = gene.map(|g| atlas.node_ref(g.node));
    json!({
        "query": q,
        "lang": lang,
        "status": status,
        "target": target,
        "gene": gene_choice,
        "choices": choices,
        "more_choices": r.conditions.len().saturating_sub(MAX_CHOICES),
        "corrected": r.corrected,
        "mention": r.mention,
        "via_wikidata": r.wikidata,
        "hits": hits,
        "ranked_matches": r.ranked,
        "method": atlas_core::search::domain::METHOD,
        "reconcile": null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::provenance::ActivityIdx;

    fn fixture(ambiguous: bool) -> (Atlas, Graph, Index) {
        let mut root = atlas_core::Term::new("HP:0000118");
        root.name = "Phenotypic abnormality".into();
        let mut symptom = atlas_core::Term::new("HP:0001250");
        symptom.name = "Seizure".into();
        symptom.parents.push(root.id.clone());
        let mut d = atlas_core::Disease::new("TEST:a", ActivityIdx(0));
        d.name = "Fixture condition".into();
        d.phenotypes.push(atlas_core::PhenotypeEdge {
            term: 1,
            annotations: vec![],
        });
        let mut other = atlas_core::Disease::new("TEST:b", ActivityIdx(0));
        other.name = if ambiguous {
            d.name.clone()
        } else {
            "Fixture unrelated".into()
        };
        other.phenotypes.push(atlas_core::PhenotypeEdge {
            term: 0,
            annotations: vec![],
        });
        let atlas = Atlas::new(
            vec![root, symptom],
            atlas_core::DiseaseIdentity::default(),
            atlas_core::Provenance::default(),
            vec![d, other],
        );
        let graph = Graph::default();
        let index = Index::build(&atlas, &graph);
        (atlas, graph, index)
    }

    #[test]
    fn equal_authoritative_names_stay_choices() {
        let (atlas, graph, index) = fixture(true);
        let r = resolve(&atlas, &graph, &index, "Fixture condition");
        assert!(!certain(&r));
        assert_eq!(body(&atlas, "Fixture condition", "en", &r)["status"], "ambiguous");
        assert_eq!(r.conditions.len(), 2);
    }

    #[test]
    fn single_symptom_candidate_never_auto_resolves() {
        let (atlas, graph, index) = fixture(false);
        let r = resolve(&atlas, &graph, &index, "Seizure");
        assert_eq!(r.conditions.len(), 1);
        assert!(!certain(&r));
        assert!(symptom_only(&r));
        assert_eq!(body(&atlas, "Seizure", "en", &r)["status"], "ambiguous");
    }

    #[test]
    fn unique_causal_gene_is_not_ambiguous_with_a_prefix_of_the_same_condition() {
        let s = crate::test_support::state();
        let mut r = resolve(s.atlas(), &s.graph, &s.search, "STXBP1");
        assert_eq!(r.conditions.len(), 1);
        let mut prefix = r
            .ranked
            .iter()
            .find(|h| h.key.kind == NodeKind::Disease)
            .unwrap()
            .clone();
        prefix.reason = Reason::Prefix;
        prefix.strong = false;
        r.ranked.push(prefix.clone());
        assert!(
            certain(&r),
            "a second path to the same identity is not a second condition"
        );
        assert_eq!(body(s.atlas(), "STXBP1", "en", &r)["status"], "resolved");
        prefix.key.idx = r.conditions[0].0 + 1;
        r.ranked.push(prefix);
        assert!(
            !certain(&r),
            "a prefix to a different identity must still require a choice"
        );
    }

    #[test]
    fn unique_identifier_resolves_and_retains_ranked_matches() {
        let (atlas, graph, index) = fixture(true);
        let r = resolve(&atlas, &graph, &index, "TEST:a");
        assert!(certain(&r));
        let body = body(&atlas, "TEST:a", "en", &r);
        assert_eq!(body["target"]["id"], "TEST:a");
        assert!(
            body["ranked_matches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|h| h.get("why").is_some() && h.get("score").is_none())
        );
    }
}
