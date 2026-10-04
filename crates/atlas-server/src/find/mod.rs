//! Search everywhere (D40). One item representation drives filters, facets and graph masks.
pub mod checked;
pub mod filters;
#[cfg(test)]
mod tests;

use crate::routes::{ApiError, ApiResult, AppState, not_found};
use atlas_core::graph::{Job, OrgKind, RecordWithhold};
use atlas_core::node::{NodeKind, NodeRef};
use atlas_core::search::SearchOptions;
use atlas_core::{Atlas, Graph};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, Uri},
};
use filters::{Filters, Params};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub fn msg(key: &str, params: Value, fallback: impl Into<String>) -> Value {
    json!({"key":key,"params":params,"fallback":fallback.into()})
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub node: NodeRef,
    pub kind: String,
    pub matched: Option<String>,
    pub match_kind: String,
    pub via: Option<Value>,
    pub country: Vec<String>,
    pub languages: Vec<String>,
    pub recruiting: Option<bool>,
    pub why: Value,
}

fn invalid_filter(name: &str) -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        msg(
            "find.error.filter",
            json!({"filter":name}),
            format!("unknown {name} filter"),
        ),
    )
}

pub fn node(atlas: &Atlas, graph: &Graph, id: &str) -> Option<NodeRef> {
    if let Some(k) = graph.node(id) {
        return graph.node_withheld(k).is_none().then(|| graph.node_ref(k));
    }
    atlas
        .search()
        .search(id, SearchOptions::default())
        .into_iter()
        .find(|h| h.match_kind == atlas_core::search::MatchKind::Id)
        .map(|h| atlas.node_ref(h.node))
}

pub fn item(graph: &Graph, node: NodeRef, matched: Option<String>, match_kind: &str, via: Option<Value>) -> Item {
    let mut i = Item {
        kind: match node.kind {
            NodeKind::Disease => "condition",
            NodeKind::Gene => "gene",
            NodeKind::Phenotype => "phenotype",
            NodeKind::Study => "study",
            NodeKind::Grant => "funding",
            NodeKind::Paper => "paper",
            NodeKind::Person => "person",
            NodeKind::Organisation => "group",
            _ => "context",
        }
        .into(),
        node,
        matched,
        match_kind: match_kind.into(),
        via,
        country: vec![],
        languages: vec![],
        recruiting: None,
        why: Value::Null,
    };
    if let Some(k) = graph.node(&i.node.id) {
        match k.kind {
            NodeKind::Study => {
                let s = graph.study(k.idx);
                i.country = s.countries.iter().filter_map(|c| filters::country(c)).collect();
                i.recruiting = Some(s.is_recruiting());
            }
            NodeKind::Grant => {
                i.country = filters::country(&graph.grant(k.idx).country).into_iter().collect();
            }
            NodeKind::Organisation => {
                let o = graph.org(k.idx);
                if !matches!(o.kind, OrgKind::PatientGroup | OrgKind::ExpertCentre) {
                    i.kind = "context".into();
                }
                i.country = o.country.as_deref().and_then(filters::country).into_iter().collect();
                i.languages = o.languages.iter().filter_map(|s| filters::language(s)).collect();
            }
            NodeKind::Asset => {
                i.kind = match graph.asset(k.idx).kind.job() {
                    Some(Job::ModelsSamples) => "model_sample",
                    Some(Job::TherapyProgrammes) => "therapy_programme",
                    Some(Job::Funding) => "funding",
                    _ => "context",
                }
                .into();
                i.country = graph
                    .asset(k.idx)
                    .fact("country")
                    .and_then(filters::country)
                    .into_iter()
                    .collect();
            }
            _ => {}
        }
    }
    i.country.sort();
    i.country.dedup();
    i.languages.sort();
    i.languages.dedup();
    i.why = if let Some(v) = &i.via {
        msg(
            "find.why.linked",
            json!({"relation":v["relation"],"via":v["node"]}),
            format!("Linked through {}", v["node"]["label"].as_str().unwrap_or("")),
        )
    } else {
        msg(
            "find.why.matched",
            json!({"matched":i.matched}),
            format!("Matched {}", i.matched.as_deref().unwrap_or(&i.node.label)),
        )
    };
    i
}

pub fn linked(atlas: &Atlas, graph: &Graph, seeds: &[NodeRef]) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut items = vec![];
    for seed in seeds {
        for e in graph.incident(&seed.id) {
            if graph.records_withheld(&e.edge.records).is_some() {
                continue;
            }
            let Some(n) = node(atlas, graph, e.other) else {
                continue;
            };
            if !seen.insert(n.id.clone()) {
                continue;
            }
            let i = item(
                graph,
                n,
                None,
                "linked",
                Some(json!({"node":seed,"edge_id":e.edge.id(),"relation":e.edge.relation.as_str()})),
            );
            if i.kind != "context" {
                items.push(i);
            }
        }
    }
    items
}

pub fn collect(s: &AppState, p: &Params) -> (Vec<Value>, Vec<Item>) {
    let hits = crate::search::ranked(
        s,
        &p.q,
        SearchOptions {
            limit: 100,
            include_retired: p.include_retired,
        },
    );
    from_ranked(s, &hits)
}

pub fn from_ranked(s: &AppState, hits: &[atlas_core::search::domain::Hit]) -> (Vec<Value>, Vec<Item>) {
    let mut results = vec![];
    let mut items = vec![];
    for h in hits {
        let mut v = serde_json::to_value(h).expect("search hit serialises");
        if h.key.kind == NodeKind::Disease
            && let Some(c) = s.atlas().identity.candidate(&h.node.id)
        {
            v["candidate_same_as"] = crate::copy_d30::candidate_same_as(&c.target, &c.target_label, &c.matched_labels);
        }
        results.push(v);
        let mut i = item(&s.graph, h.node.clone(), Some(h.matched.clone()), h.match_kind, None);
        i.why = h
            .why_messages
            .first()
            .map(|m| json!({"key":m.key,"params":m.params,"fallback":h.why,"text":h.why,"messages":h.why_messages}))
            .unwrap_or(Value::Null);
        items.push(i);
    }
    (results, items)
}

pub fn block(items: &[Item], f: &Filters, limit: usize) -> Value {
    let selected: Vec<_> = items.iter().filter(|i| f.accepts(i, "")).collect();
    json!({"items":selected.iter().take(limit).collect::<Vec<_>>(),"total":selected.len(),"facets":f.facets(items),"applies_to":filters::applies_to()})
}

pub fn suggestions(s: &AppState, q: &str, lang: &str, limit: usize) -> Value {
    let mut hits = s.search.suggest(
        s.atlas(),
        &s.graph,
        q,
        SearchOptions {
            limit,
            include_retired: false,
        },
    );
    for h in &mut hits {
        h.localise(lang);
    }
    json!(hits)
}

pub async fn suggest(State(s): State<AppState>, Query(mut p): Query<Params>) -> ApiResult {
    p.suggest = Some(1);
    search(State(s), HeaderMap::new(), Query(p)).await
}

pub async fn search(State(s): State<AppState>, headers: HeaderMap, Query(p): Query<Params>) -> ApiResult {
    if p.q.len() > 512 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search.error.too_long".into()));
    }
    search_request(&s, &headers, p).await
}

/// Long pastes use the body, not a URL. Documents use /api/intake and the same owner.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupRequest {
    q: String,
    limit: Option<usize>,
}

/// Ephemeral autocomplete: no query in a URL, provider call or shared text cache.
pub async fn lookup(State(s): State<AppState>, Json(p): Json<LookupRequest>) -> ApiResult {
    let limit = p.limit.unwrap_or(6);
    if p.q.trim().is_empty() || p.q.chars().any(char::is_control) || !(1..=6).contains(&limit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            msg("find.error.query", json!({}), "Invalid search request."),
        ));
    }
    if p.q.len() > 128 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search.error.too_long".into()));
    }
    search(
        State(s),
        HeaderMap::new(),
        Query(Params {
            q: p.q,
            limit: Some(limit),
            suggest: Some(1),
            llm: Some(0),
            ..Params::default()
        }),
    )
    .await
}

pub async fn search_text(State(s): State<AppState>, headers: HeaderMap, Json(p): Json<Params>) -> ApiResult {
    search_request(&s, &headers, p).await
}

async fn search_request(s: &AppState, headers: &HeaderMap, p: Params) -> ApiResult {
    let f = p.filters().map_err(invalid_filter)?;
    let lang = p.lang.as_deref().unwrap_or("en");
    let limit = p.limit.unwrap_or(20).min(100);
    if p.suggest == Some(1) {
        let choices = suggestions(s, &p.q, lang, limit);
        return Ok(Json(json!({"query":p.q,"results":choices,
            "suggestions":choices,"method":"atlas-typeahead-v1"})));
    }
    if p.q.trim().is_empty() {
        return Ok(Json(
            json!({"query":"","results":[],"items":[],"total":0,"suggestions":[],"understood":null}),
        ));
    }
    let found =
        crate::understand::text_options(s, headers, p.q.clone(), lang, p.llm != Some(0), p.include_retired).await?;
    let (ranked, items) = from_ranked(s, &found.hits);
    let mut body = block(&items, &f, limit);
    body["query"] = json!(p.q);
    body["results"] = json!(
        ranked
            .into_iter()
            .filter(|h| items
                .iter()
                .find(|i| i.node.id == h["node"]["id"])
                .is_some_and(|i| f.accepts(i, "")))
            .take(limit)
            .collect::<Vec<_>>()
    );
    body["filters"] = json!(f);
    body["checked"] = checked::build(s.atlas(), &s.graph, &items, "query");
    body["understood"] = found.json(s);
    body["query_execution"] = found.query_execution;
    body["suggestions"] = suggestions(s, &p.q, lang, 3);
    body["method"] = json!(atlas_core::search::domain::METHOD);
    body["phenotype_method"] = json!(atlas_analytics::Scorer::Fusion.version());
    crate::coverage::search(s.atlas(), &s.graph, &mut body);
    Ok(Json(body))
}

pub async fn resolve(State(s): State<AppState>, headers: HeaderMap, uri: Uri, Query(p): Query<Params>) -> ApiResult {
    let f = p.filters().map_err(invalid_filter)?;
    let rp = Query::<crate::journeys::ResolveParams>::try_from_uri(&uri).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            msg("find.error.query", json!({}), "Invalid search request."),
        )
    })?;
    let (mut body, matches) = crate::journeys::resolve_with_matches(s.clone(), headers, rp.0).await?;
    // Retired-inclusive coverage uses a different query; resolution remains live-only.
    let (_, mut items) = if p.include_retired {
        collect(&s, &p)
    } else {
        from_ranked(&s, &matches)
    };
    if let Some(id) = body["target"]["id"].as_str()
        && let Some(target) = node(s.atlas(), &s.graph, id)
    {
        let links = linked(s.atlas(), &s.graph, &[target]);
        body["linked"] = block(&links, &f, 100);
        items.extend(links);
    }
    body["filters"] = json!(f);
    body["checked"] = checked::build(s.atlas(), &s.graph, &items, "query");
    body["items"] = block(&items, &f, p.limit.unwrap_or(20).min(100))["items"].clone();
    Ok(Json(body))
}

pub async fn graph_view(State(s): State<AppState>, Query(p): Query<Params>) -> ApiResult {
    let f = p.filters().map_err(invalid_filter)?;
    let limit = p.limit.unwrap_or(80).clamp(1, 300);
    let focus = p
        .focus
        .as_deref()
        .filter(|f| !f.is_empty())
        .map(|id| {
            node(s.atlas(), &s.graph, id)
                .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.item_unknown", json!({"id":id}))))
        })
        .transpose()?;
    let seeds = match &focus {
        Some(n) => vec![n.clone()],
        None => {
            let mut degree = HashMap::<&str, usize>::new();
            for e in s
                .graph
                .edges()
                .iter()
                .filter(|e| s.graph.records_withheld(&e.records).is_none())
            {
                for id in [&e.from, &e.to] {
                    if s.atlas()
                        .disease_idx(id)
                        .is_some_and(|d| s.atlas().disease_at(d).rare && s.atlas().disease_at(d).is_active())
                    {
                        *degree.entry(id).or_default() += 1;
                    }
                }
            }
            let mut degree: Vec<_> = degree.into_iter().collect();
            degree.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            degree
                .into_iter()
                .take(8)
                .filter_map(|(id, _)| node(s.atlas(), &s.graph, id))
                .collect()
        }
    };
    let mut items: Vec<_> = seeds
        .iter()
        .cloned()
        .map(|n| item(&s.graph, n, None, "linked", None))
        .collect();
    items.extend(linked(s.atlas(), &s.graph, &seeds));
    let mut seen = HashSet::new();
    items.retain(|i| seen.insert(i.node.id.clone()));
    let truncated = items.len() > limit;
    items.truncate(limit);
    let ids: HashSet<_> = items.iter().map(|i| i.node.id.as_str()).collect();
    let edges: Vec<_> = s
        .graph
        .edges()
        .iter()
        .filter(|e| {
            ids.contains(e.from.as_str())
                && ids.contains(e.to.as_str())
                && s.graph.records_withheld(&e.records).is_none()
        })
        .map(|e| json!({"id":e.id(),"from":e.from,"to":e.to,"relation":e.relation.as_str()}))
        .collect();
    let nodes: Vec<_> = items.iter().map(|i| json!({"node":i.node,"kind":i.kind,"masked":!f.accepts(i,""),"degree":s.graph.incident(&i.node.id).filter(|e| s.graph.records_withheld(&e.edge.records).is_none() && node(s.atlas(),&s.graph,e.other).is_some()).count(),"country":i.country,"recruiting":i.recruiting})).collect();
    Ok(Json(
        json!({"focus":focus,"nodes":nodes,"edges":edges,"filters":f,"facets":f.facets(&items),"applies_to":filters::applies_to(),"truncated":truncated}),
    ))
}
