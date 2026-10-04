//! JSON views: `/api/stats`, `/api/disease/{id}` (design/API.md shape) and `/api/match`
//! (the Python `api.py` JSON plus the D30.2 scorer fields; `scorer=atlas` keeps its probability).

use std::collections::BTreeSet;

use atlas_analytics::{Contribution, Matcher, Scorer, ranker_v3, ranking};
use atlas_core::disease::{Attributes, PhenotypeEdge};
use atlas_core::evidence::{GeneLink, PhenotypeAnnotation};
use atlas_core::identity::{CANDIDATE_SAME_AS, Candidate};
use atlas_core::node::{Edge, EdgeKind, Evidence, NodeKind, NodeRef};
use atlas_core::provenance::activity;
use atlas_core::{Atlas, DiseaseIdx, curie};
use serde_json::{Value, json};

use crate::routes::MatchRequest;

/// Python `round(x, n)`: decimal round-half-even of the exact binary value.
fn round(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// Python stats keys, `retired_diseases`, and the version and checksum of every source.
pub fn stats(atlas: &Atlas) -> Value {
    let mut v = serde_json::to_value(atlas.stats()).expect("stats serialise");
    let sources: Vec<Value> = atlas.provenance.entities.iter().map(crate::prov::entity).collect();
    v["sources"] = Value::Array(sources);
    v
}

pub use atlas_journeys::{GENE_RELATION, phenotype_relation};

/// Evidence for one HPOA row.
pub fn annotation_evidence(atlas: &Atlas, a: &PhenotypeAnnotation) -> Evidence {
    Evidence {
        source: format!("HPO annotation ({})", curie::prefix(&a.disease_id)),
        record: Some(atlas.provenance.cite(&a.record)),
        references: a.references.clone(),
        evidence_code: Some(a.evidence.clone()),
        frequency: a.frequency.as_ref().map(|f| f.raw.clone()),
        cohort: a.frequency.as_ref().and_then(|f| f.cohort),
        date: a.date().map(str::to_owned),
        ..Evidence::default()
    }
}

pub fn gene_evidence(atlas: &Atlas, g: &GeneLink) -> Evidence {
    let mut references = g.pmids.clone();
    references.push(g.source_disease.clone());
    Evidence {
        source: g.source.clone(),
        record: Some(atlas.provenance.cite(&g.record)),
        references,
        evidence_code: Some(match g.assessed {
            Some(true) => format!("{} (assessed)", g.association),
            Some(false) => format!("{} (not yet assessed)", g.association),
            None => g.association.clone(),
        }),
        ..Evidence::default()
    }
}

pub fn phenotype_edge(atlas: &Atlas, disease: &str, e: &PhenotypeEdge, absent: bool) -> Edge {
    let evidence = e.annotations.iter().map(|a| annotation_evidence(atlas, a)).collect();
    Edge::new(
        disease,
        phenotype_relation(absent),
        &atlas.hpo.term(e.term).id,
        EdgeKind::Observed,
        evidence,
    )
}

/// Gene node for a link: the gene table entry if the gene has an active disease, else from the link
/// (id: [`atlas_journeys::gene_id`]).
pub fn gene_ref(atlas: &Atlas, g: &GeneLink) -> NodeRef {
    match atlas.gene(&g.symbol) {
        Some(i) => atlas.node_ref(atlas_core::node::NodeKey {
            kind: NodeKind::Gene,
            idx: i,
        }),
        None => NodeRef {
            id: atlas_journeys::gene_id(atlas, g),
            kind: NodeKind::Gene,
            label: g.symbol.clone(),
        },
    }
}

/// Gene edges of a disease ([`atlas_journeys::gene_edges`]), one per symbol with every supporting
/// link, as API edges with their evidence.
pub fn gene_edges<'a>(atlas: &Atlas, disease: &str, links: &'a [GeneLink]) -> Vec<(NodeRef, Vec<&'a GeneLink>, Edge)> {
    atlas_journeys::gene_edges(atlas, disease, links)
        .into_iter()
        .map(|ge| {
            let node = gene_ref(atlas, ge.links[0]);
            let evidence = ge.links.iter().map(|g| gene_evidence(atlas, g)).collect();
            let edge = Edge::new(disease, GENE_RELATION, &node.id, EdgeKind::Observed, evidence);
            (node, ge.links, edge)
        })
        .collect()
}

/// HPO frequency class of the best raw frequency on the edge: `unknown` (no frequency recorded),
/// `zero_observed` (0 of n), else the HPO class. Absent (NOT) edges are `excluded` with state `absent`.
fn tier(e: &PhenotypeEdge) -> &'static str {
    let best = e
        .annotations
        .iter()
        .filter_map(|a| {
            let f = a.frequency.as_ref()?;
            match f.cohort {
                Some((n, m)) if m > 0 => Some(n as f64 / m as f64),
                _ => f.value,
            }
        })
        .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))));
    match best {
        None => "unknown",
        Some(p) if p >= 1.0 => "obligate",
        Some(p) if p >= 0.8 => "very_frequent",
        Some(p) if p >= 0.3 => "frequent",
        Some(p) if p >= 0.05 => "occasional",
        Some(p) if p > 0.0 => "very_rare",
        // observed in none of the examined patients: not the same as a NOT annotation (state "absent")
        Some(_) => "zero_observed",
    }
}

fn attribute_values(attrs: &Attributes) -> Vec<&str> {
    attrs.keys().map(String::as_str).collect()
}

/// D30.1 label-nominated identities around a node: its own `candidate_same_as` link (a source id
/// whose name matches one MONDO term) and the incoming ones (on a MONDO node). Never merges.
fn candidates(atlas: &Atlas, id: &str) -> (Value, Vec<Value>) {
    let node = |id: &str, label: &str| match atlas.disease_idx(id) {
        Some(i) => serde_json::to_value(atlas.disease_ref(i)).unwrap_or(Value::Null),
        None => json!({ "id": id, "kind": "disease", "label": label }),
    };
    let link = |c: &Candidate| {
        json!({
            "relation": CANDIDATE_SAME_AS,
            "source_id": c.source_id,
            "target": node(&c.target, &c.target_label),
            "matched_labels": c.matched_labels,
            "guard": c.guard,
            "activity": activity::IDENTITY_LABEL,
        })
    };
    let own = atlas.identity.candidate(id).map_or(Value::Null, |c| {
        let mut v = link(c);
        v["display"] = crate::copy_d30::candidate_same_as(&c.target, &c.target_label, &c.matched_labels);
        v
    });
    let incoming = atlas
        .identity
        .candidates_for(id)
        .map(|c| {
            let mut v = link(c);
            v["source"] = node(&c.source_id, &c.source_id);
            v["display"] = crate::copy_d30::candidate_source(&c.source_id);
            v
        })
        .collect();
    (own, incoming)
}

pub fn disease(atlas: &Atlas, idx: DiseaseIdx) -> Value {
    let d = atlas.disease_at(idx);
    let source_ids: Vec<Value> = d
        .source_ids
        .iter()
        .map(|s| {
            let merge = atlas.identity.merged_by(s);
            json!({
                "id": s,
                "merged_by": merge.map(|m| m.basis.as_str()),
                "mapping_basis": atlas.identity.mapping_basis(s).map(|b| b.as_str()),
                "activity": merge.map(|m| m.basis.activity_id()),
            })
        })
        .collect();
    let (candidate_same_as, candidate_sources) = candidates(atlas, &d.id);
    let related: Vec<Value> = d
        .related
        .iter()
        .map(|l| json!({ "id": l.target, "relation": l.relation, "asserted_by": l.asserted_by }))
        .collect();
    let genes: Vec<Value> = gene_edges(atlas, &d.id, &d.genes)
        .into_iter()
        .map(|(node, group, edge)| {
            let effect = group
                .iter()
                .map(|g| g.variant_effect())
                .find(|e| *e != "unknown")
                .unwrap_or("unknown");
            json!({ "gene": node, "variant_effect": effect, "edge": edge })
        })
        .collect();
    let phenotype = |e: &PhenotypeEdge, absent: bool| {
        json!({
            "term": atlas.term_ref(e.term),
            "ic": atlas.hpo.ic(e.term),
            "state": if absent { "absent" } else { "present" },
            "tier": if absent { "excluded" } else { tier(e) },
            "edge": phenotype_edge(atlas, &d.id, e, absent),
        })
    };
    let mut phenotypes: Vec<Value> = d.phenotypes.iter().map(|e| phenotype(e, false)).collect();
    phenotypes.extend(d.excluded.iter().map(|e| phenotype(e, true)));
    let prevalence: Vec<Value> = d
        .prevalence
        .iter()
        .map(|p| {
            json!({
                "kind": p.kind, "qualification": p.qualification, "prevalence_class": p.prevalence_class,
                "mean_value": p.mean_value, "geography": p.geography, "validated": p.validated,
                "pmids": p.pmids, "record": atlas.provenance.cite(&p.record),
            })
        })
        .collect();
    let classifications: Vec<Value> = d
        .classifications
        .iter()
        .map(|c| {
            json!({
                "property": c.property, "value": c.value, "reason": c.reason,
                "activity": atlas.provenance.activity(c.activity).id,
            })
        })
        .collect();
    let files: BTreeSet<&str> = d
        .derived_from
        .iter()
        .map(|r| atlas.provenance.entity(r.entity).file.as_str())
        .collect();
    json!({
        "node": atlas.disease_ref(idx),
        "definition": d.definition,
        "status": d.status,
        "rare": d.rare,
        "newly_described": d.is_newly_described(),
        "confidence": d.classification("confidence"),
        "synonyms": d.synonyms.iter().map(|n| &n.text).collect::<Vec<_>>(),
        "source_ids": source_ids,
        "mapping_basis": atlas.identity.mapping_basis(&d.id).map(|b| b.as_str()),
        "candidate_same_as": candidate_same_as,
        "candidate_sources": candidate_sources,
        "related_ids": related,
        "genes": genes,
        "phenotypes": phenotypes,
        "inheritance": attribute_values(&d.inheritance),
        "onset": attribute_values(&d.onset),
        "clinical_course": attribute_values(&d.clinical_course),
        "prevalence": prevalence,
        "classifications": classifications,
        "completeness": {
            "phenotypes_present": d.phenotypes.len(),
            "phenotypes_absent": d.excluded.len(),
            "genes": d.genes.len(),
            "prevalence": d.prevalence.len(),
            "onset": !d.onset.is_empty(),
            "inheritance": !d.inheritance.is_empty(),
            "definition": !d.definition.is_empty(),
            "sources": files,
        },
    })
}

fn contribution(atlas: &Atlas, disease: DiseaseIdx, c: &Contribution) -> Value {
    let id = |t: u32| atlas.hpo.term(t).id.as_str();
    let evidence: Vec<Value> = c
        .evidence(atlas, disease)
        .iter()
        .map(|e| {
            json!({
                "source": e.disease_id,
                "evidence": e.evidence,
                "frequency": e.frequency.as_ref().map(|f| &f.raw),
                "references": e.references,
                "record": atlas.provenance.cite(&e.record),
            })
        })
        .collect();
    json!({
        "query_term": id(c.query_term),
        "query_label": atlas.hpo.term(c.query_term).name,
        "matched_term": c.matched_term.map(id),
        "matched_label": c.matched_term.map(|t| &atlas.hpo.term(t).name),
        "kind": c.kind,
        "log_lr": round(c.log_lr, 3),
        "frequency": c.frequency,
        "evidence": evidence,
    })
}

/// `/api/match`, as the Python `api.match`.
pub fn matches(m: &Matcher, req: &MatchRequest) -> Value {
    let atlas = m.atlas();
    let (present, unknown_present) = m.canonical_terms(&req.present);
    let (excluded, unknown_excluded) = m.canonical_terms(&req.excluded);
    let scorer = req.scorer;
    // the fusion inputs are validated (finite, same candidates); a failure is a bug, not user input
    let results = ranking::rank(m, scorer, &present, &excluded, req.top.min(100)).unwrap_or_default();
    let terms = |ts: &[u32]| -> Vec<Value> {
        ts.iter()
            .map(|&t| json!({ "id": atlas.hpo.term(t).id, "label": atlas.hpo.term(t).name }))
            .collect()
    };
    let matches: Vec<Value> = results
        .iter()
        .map(|r| {
            let e = &r.explanation;
            let contributions: Vec<Value> = e
                .contributions
                .iter()
                .map(|c| contribution(atlas, r.disease, c))
                .collect();
            let mut v = json!({
                "disease_id": atlas.disease_at(r.disease).id,
                "name": atlas.disease_at(r.disease).name,
                "score": if scorer == Scorer::Atlas { round(r.score, 3) } else { r.score },
                "rank": r.rank,
                "components": {
                    "atlas": { "score": round(r.atlas.score, 3), "midrank": r.atlas.midrank },
                    "resnik": { "score": r.resnik.score, "midrank": r.resnik.midrank },
                },
                "contributions": contributions,
            });
            // only the atlas scorer is a (flat-prior) probability; fused and Resnik scores are uncalibrated
            if scorer == Scorer::Atlas {
                v["probability"] = json!(round(e.probability, 4));
            }
            if let Some(c) = atlas.identity.candidate(&atlas.disease_at(r.disease).id) {
                v["candidate_same_as"] =
                    crate::copy_d30::candidate_same_as(&c.target, &c.target_label, &c.matched_labels);
            }
            v
        })
        .collect();
    json!({
        "scorer": scorer.as_str(),
        "scorer_version": scorer.version(),
        "claim": match scorer {
            Scorer::Fusion => crate::copy_d30::ranker_v3_claim(ranker_v3::RUN_ID),
            Scorer::Atlas | Scorer::Resnik => Value::Null,
        },
        "present": terms(&present),
        "excluded": terms(&excluded),
        "unknown_terms": ([unknown_present, unknown_excluded].concat()),
        "matches": matches,
    })
}
