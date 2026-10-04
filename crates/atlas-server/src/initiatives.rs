//! Compact "Already working on this" results and the Partners-page collection.

use crate::nodes;
use crate::routes::{ApiResult, AppState, not_found};
use atlas_core::graph::{Initiative, OfficialAction, RecordWithhold, Relation};
use atlas_core::{Atlas, DiseaseIdx, Graph};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

const LIMIT: usize = 3;

fn family_action(action: &OfficialAction) -> bool {
    let a = action.audience.to_lowercase();
    a.contains("famil") || a.contains("patient") && !a.contains("organisation") || a.contains("caregiver")
}

fn action_value(action: &OfficialAction) -> Value {
    json!({"action": action.action, "url": action.url, "outcome": action.outcome,
        "audience": action.audience, "date": nodes::day(&action.retrieved_at),
        "retrieved_at": action.retrieved_at, "availability": action.availability,
        "source": {"url": action.source_url, "version": action.source_version,
            "sha256": action.sha256, "record_locator": action.record_locator}})
}

fn visible<'a>(graph: &'a Graph, initiative: &Initiative) -> Option<&'a atlas_core::graph::Organisation> {
    if !initiative.verified {
        return None;
    }
    let key = graph.node(&initiative.id)?;
    if graph.node_withheld(key).is_some() || graph.records_withheld(&initiative.records).is_some() {
        return None;
    }
    Some(graph.org(key.idx))
}

fn item(graph: &Graph, initiative: &Initiative, scope: &str, links: &[u32]) -> Option<Value> {
    let org = visible(graph, initiative)?;
    let action = initiative
        .actions
        .iter()
        .filter(|a| graph.records_withheld(&a.records).is_none())
        .min_by_key(|a| !family_action(a))?;
    let link_evidence: Vec<Value> = links
        .iter()
        .map(|&idx| {
            let edge = graph.edge(idx);
            json!({"id": edge.id(), "relation": edge.relation.as_str(), "target": edge.to,
            "provenance": format!("/api/provenance/{}", nodes::encode(&edge.id()))})
        })
        .collect();
    Some(json!({
        "id": initiative.id, "initiative": org.name, "description": org.description,
        "scope": scope, "research_only": initiative.research_only,
        "official_action": action_value(action),
        "links": link_evidence,
        "provenance": format!("/api/provenance/{}", nodes::encode(&initiative.id)),
    }))
}

/// Exact condition scope first, then its causal gene scopes. General rare-disease
/// support fills only an empty result, never crowds out a specific effort.
pub(crate) fn select(graph: &Graph, targets: &[(String, Relation)], general: bool) -> Vec<Value> {
    let mut specific = vec![];
    for initiative in &graph.data().initiatives {
        if visible(graph, initiative).is_none() {
            continue;
        }
        let mut links = vec![];
        let mut rank = usize::MAX;
        for (target, relation) in targets {
            for incident in graph.incident(target) {
                if incident.edge.from == initiative.id
                    && incident.edge.relation == *relation
                    && incident.edge.level == atlas_core::graph::LinkLevel::Curated
                    && graph.records_withheld(&incident.edge.records).is_none()
                {
                    if !links.contains(&incident.idx) {
                        links.push(incident.idx);
                    }
                    rank = rank.min(if *relation == Relation::ServesCondition { 0 } else { 1 });
                }
            }
        }
        if !links.is_empty()
            && let Some(value) = item(graph, initiative, if rank == 0 { "condition" } else { "gene" }, &links)
        {
            specific.push((rank, initiative.id.as_str(), value));
        }
    }
    specific.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    if !specific.is_empty() {
        return specific.into_iter().take(LIMIT).map(|(_, _, v)| v).collect();
    }
    if !general {
        return vec![];
    }
    let mut fallback: Vec<_> = graph
        .data()
        .initiatives
        .iter()
        .filter(|n| n.scopes.iter().any(|s| s.kind == "all_rare_diseases"))
        .filter_map(|n| item(graph, n, "all_rare_diseases", &[]).map(|v| (n.id.as_str(), v)))
        .collect();
    fallback.sort_by(|a, b| a.0.cmp(b.0));
    fallback.into_iter().take(LIMIT).map(|(_, v)| v).collect()
}

pub fn for_condition(atlas: &Atlas, graph: &Graph, d: DiseaseIdx) -> Vec<Value> {
    if !atlas.disease_at(d).is_active() {
        return vec![];
    }
    let mut targets = vec![(atlas.disease_at(d).id.clone(), Relation::ServesCondition)];
    targets.extend(
        nodes::causal_genes(atlas, d)
            .into_iter()
            .map(|(id, _, _)| (id, Relation::ServesGene)),
    );
    select(graph, &targets, atlas.disease_at(d).rare)
}

pub fn for_gene(atlas: &Atlas, graph: &Graph, g: u32) -> Vec<Value> {
    let gene = atlas.gene_at(g);
    let mut targets = vec![(gene.id().to_owned(), Relation::ServesGene)];
    let mut rare = false;
    for &d in &gene.diseases {
        let dis = atlas.disease_at(d);
        if dis.rare && dis.is_active() && dis.genes.iter().any(|l| l.symbol == gene.symbol && l.is_causal()) {
            targets.push((dis.id.clone(), Relation::ServesCondition));
            rare = true;
        }
    }
    select(graph, &targets, rare)
}

pub(crate) fn references(graph: &Graph) -> Vec<Value> {
    graph
        .data()
        .initiatives
        .iter()
        .filter(|n| !n.verified)
        .filter_map(|n| {
            let key = graph.node(&n.id)?;
            if graph.node_withheld(key).is_some() || graph.records_withheld(&n.records).is_some() {
                return None;
            }
            let org = graph.org(key.idx);
            let reported_actions: Vec<_> = n
                .reported_actions
                .iter()
                .filter(|a| graph.records_withheld(&a.records).is_none())
                .map(|a| {
                    json!({"url":a.url,"action":a.action,"outcome":a.outcome,"audience":a.audience,
                "verification":a.verification,"destination_status":a.destination_status,"active":false,
                "source":{"kind":"provided_context","version":a.source_version,"sha256":a.sha256,
                    "retrieved_at":a.retrieved_at,"record_locator":a.record_locator}})
                })
                .collect();
            Some(
                json!({"id":n.id,"initiative":org.name,"url":org.url,"verification":"unverified",
            "reason":n.exclusion_reason,"official_action":null,"reported_actions":reported_actions,
            "provenance":format!("/api/provenance/{}",nodes::encode(&n.id))}),
            )
        })
        .collect()
}

pub async fn partners(State(s): State<AppState>) -> ApiResult {
    let mut initiatives: Vec<Value> = s
        .graph
        .data()
        .initiatives
        .iter()
        .filter_map(|n| {
            let org = visible(&s.graph, n)?;
            let mut value = item(&s.graph, n, "publisher_stated", &[]).unwrap_or_else(|| {
                json!({
                    "id": n.id, "initiative": org.name, "description": org.description,
                    "scope": "publisher_stated", "research_only": n.research_only,
                    "official_action": null, "links": [],
                    "provenance": format!("/api/provenance/{}",nodes::encode(&n.id)),
                })
            });
            value["scopes"] = json!(n.scopes);
            value["official_actions"] = n
                .actions
                .iter()
                .filter(|a| s.graph.records_withheld(&a.records).is_none())
                .map(action_value)
                .collect();
            value["languages"] = json!(visible(&s.graph, n)?.languages);
            Some(value)
        })
        .collect();
    initiatives.sort_by(|a, b| a["initiative"].as_str().cmp(&b["initiative"].as_str()));
    Ok(Json(
        json!({"initiatives": initiatives,"references":references(&s.graph)}),
    ))
}

/// Additive gene detail endpoint, using the same selection as gene jobs.
pub async fn gene(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let atlas = s.atlas();
    let idx = atlas
        .gene(s.graph.canonical_id(id.trim()))
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.gene_unknown", json!({"id": id}))))?;
    let gene = atlas.gene_at(idx);
    let conditions: Vec<_> = gene.diseases.iter().map(|&d| atlas.disease_ref(d)).collect();
    Ok(Json(
        json!({"node": {"id": gene.id(), "kind":"gene", "label": gene.symbol},
        "conditions": conditions, "already_working_on_this": for_gene(atlas,&s.graph,idx)}),
    ))
}
