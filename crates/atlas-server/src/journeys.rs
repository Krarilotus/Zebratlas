//! Journey endpoints (design/JOURNEYS.md, API.md): resolve, connections, gaps, related, summary,
//! message, export, LLM connections. Lexical and graph work is deterministic and fast; LLM text is
//! added only where asked (`explain=`, unresolved or foreign-language `resolve`, summary, message)
//! and always says whether it came from the model or the template.

use std::convert::Infallible;

use atlas_core::DiseaseIdx;
use atlas_llm::tasks::{DraftKind, MessageCard, UserRole};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::connections::{self, plain_kind};
use crate::routes::{ApiError, ApiResult, AppState, internal, not_found};
use crate::{cards, export as ttl, llm, nodes, resolve as res};

fn condition(s: &AppState, id: &str) -> Result<DiseaseIdx, ApiError> {
    nodes::condition(s.atlas(), id).map_err(not_found)
}

fn lang_of(lang: &Option<String>) -> String {
    lang.as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("en")
        .to_owned()
}

#[derive(Deserialize)]
pub struct ResolveParams {
    #[serde(default)]
    q: String,
    lang: Option<String>,
    /// `0` disables the LLM fallback.
    llm: Option<u8>,
}

/// J1.1: resolution consumes the same D56 interpretation as submitted search.
#[cfg(test)]
pub async fn resolve(State(s): State<AppState>, headers: HeaderMap, Query(p): Query<ResolveParams>) -> ApiResult {
    resolve_with_matches(s, headers, p).await.map(|(body, _)| Json(body))
}

/// Return the same ranked matches to the filters/coverage adapter instead of retrieving twice.
pub async fn resolve_with_matches(
    s: AppState,
    headers: HeaderMap,
    p: ResolveParams,
) -> Result<(Value, Vec<atlas_core::search::domain::Hit>), ApiError> {
    let lang = lang_of(&p.lang);
    if p.q.len() > 512 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search.error.too_long".into()));
    }
    let found = crate::understand::text(&s, &headers, p.q.clone(), &lang, p.llm != Some(0)).await?;
    let r = res::from_hits(s.atlas(), &p.q, found.hits.clone());
    let mut body = res::body(s.atlas(), &p.q, &lang, &r);
    body["understood"] = found.json(&s);
    body["query_execution"] = found.query_execution;
    body["reconcile"] = found.metadata;
    body["suggestions"] = crate::find::suggestions(&s, &p.q, &lang, 3);
    crate::coverage::resolution(s.atlas(), &s.graph, &mut body);
    Ok((body, found.hits))
}

#[derive(Deserialize)]
pub struct ConnectionParams {
    kind: Option<String>,
    limit: Option<usize>,
    lang: Option<String>,
    /// LLM `why` sentences for the first N cards (default 0: template sentences only).
    explain: Option<usize>,
    /// Newline-delimited JSON: graph cards first, then completed model explanations.
    #[serde(default)]
    stream: bool,
}

/// J1.3, J1.4, J2.3, J2.4, J3.1.
pub async fn connections(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(p): Query<ConnectionParams>,
) -> Result<Response, ApiError> {
    let d = condition(&s, &id)?;
    let atlas = s.atlas();
    let conn = connections::collect(atlas, &s.graph, d);
    let mut body = cards::connections(
        atlas,
        &s.graph,
        &conn,
        p.kind.as_deref(),
        p.limit.unwrap_or(10).clamp(1, 100),
    );
    let n = p.explain.unwrap_or(0).min(10);
    body["already_working_on_this"] = json!(crate::initiatives::for_condition(atlas, &s.graph, d));
    let lang = lang_of(&p.lang);
    let mut set = tokio::task::JoinSet::new();
    let cards_json = body["cards"].as_array().cloned().unwrap_or_default();
    for (i, card) in cards_json
        .iter()
        .enumerate()
        .take(if s.llm.llm.is_some() { n } else { 0 })
    {
        let model = s.llm.llm.as_ref().expect("configured above");
        let Some(f) = conn
            .found
            .iter()
            .find(|f| s.graph.node_ref(f.who).id == card["id"].as_str().unwrap_or(""))
        else {
            continue;
        };
        let facts = llm::card_facts(f);
        let who = s.graph.node_ref(f.who).label;
        let call = llm::call(model, &headers, facts.iter().map(|x| x.key.clone()))
            .with_deadline(std::time::Duration::from_secs(3));
        let (model, lang) = (model.clone(), lang.clone());
        set.spawn(async move { (i, atlas_llm::why_sentence(&model, &call, &who, &facts, &lang).await) });
    }
    if p.stream {
        let initial = json!({ "event": "connections", "data": body });
        let state = (
            Some(initial),
            set,
            cards_json,
            s.llm.runtime.clone(),
            s.withhold.clone(),
            false,
        );
        let rows = futures_util::stream::unfold(
            state,
            |(mut initial, mut tasks, cards, runtime, withhold, finished)| async move {
                if finished {
                    return None;
                }
                let (row, finished) = if let Some(row) = initial.take() {
                    (row, false)
                } else if let Some(done) = tasks.join_next().await {
                    match done {
                        Ok((i, generated)) => {
                            let activities = runtime.register(&generated.calls);
                            (
                                json!({ "event": "explanation", "card_id": cards[i]["id"], "data": explanation(&generated, activities) }),
                                false,
                            )
                        }
                        Err(_) => (json!({ "event": "error", "detail": "explanation unavailable" }), false),
                    }
                } else {
                    (json!({ "event": "done" }), true)
                };
                // Withholding must happen per row: an approval during an open stream takes effect.
                let row = crate::privacy::filter_stream_value(&withhold, row)
                    .unwrap_or_else(|| json!({ "event": "withheld" }));
                Some((
                    Ok::<_, Infallible>(format!("{row}\n")),
                    (initial, tasks, cards, runtime, withhold, finished),
                ))
            },
        );
        return Ok((
            [
                (header::CONTENT_TYPE, "application/x-ndjson"),
                (header::CACHE_CONTROL, "no-store"),
                (header::HeaderName::from_static("x-accel-buffering"), "no"),
            ],
            Body::from_stream(rows),
        )
            .into_response());
    }
    while let Some(done) = set.join_next().await {
        let (i, generated) = done.map_err(internal)?;
        let activities = s.llm.runtime.register(&generated.calls);
        body["cards"][i]["why"]["llm"] = explanation(&generated, activities);
    }
    Ok(Json(body).into_response())
}

fn explanation(generated: &atlas_llm::Generated, activities: Vec<String>) -> Value {
    json!({ "text": generated.text(), "origin": generated.origin, "lang": generated.lang,
        "sentences": generated.sentences, "validation": generated.validation, "activities": activities,
        "models": atlas_llm::model_labels(&generated.calls) })
}

/// J3.4.
pub async fn gaps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let d = condition(&s, &id)?;
    let conn = connections::collect(s.atlas(), &s.graph, d);
    Ok(Json(cards::gaps(s.atlas(), &s.graph, &conn)))
}

#[derive(Deserialize)]
pub struct RelatedParams {
    k: Option<usize>,
}

/// J2.1, J2.2, J3.2: `SimilarityIndex::related` (mechanism agent) plus, per related condition, a
/// summary of its exact assets from the connected graph (J2.3).
pub async fn related(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(p): Query<RelatedParams>,
) -> Result<Response, ApiError> {
    let d = condition(&s, &id)?;
    let cid = s.atlas().disease_at(d).id.clone();
    let unavailable = |status: &str| {
        let msg = crate::copy_extra::msg(&format!("api.related.{status}"), json!({}));
        let body = json!({ "detail": msg["fallback"], "detail_msg": msg, "status": status });
        Ok((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response())
    };
    match s.related.get() {
        None => {
            return unavailable("warming_up");
        }
        Some(Err(_)) => return unavailable("unavailable"),
        Some(Ok(_)) => {}
    }
    let k = p.k.unwrap_or(10).clamp(1, 50);
    let s2 = s.clone();
    let body = tokio::task::spawn_blocking(move || index_report(&s2, &cid, k))
        .await
        .map_err(internal)?;
    body.map(|b| Json(b).into_response()).map_err(internal)
}

/// A cluster with its stability; only `stable` clusters are presented as findings.
fn cluster_summary(c: &atlas_analytics::Cluster) -> Value {
    let stable = c.stability.verdict == "stable";
    let note = crate::copy_extra::msg(if stable { "cluster.stable" } else { "cluster.unstable" }, json!({}));
    let why = vec![crate::copy_extra::msg(
        "cluster.shared_records",
        json!({
            "processes": c.processes.iter().take(3).map(|p| &p.name).collect::<Vec<_>>(),
            "symptoms": c.phenotypes.iter().take(3).map(|p| &p.name).collect::<Vec<_>>()
        }),
    )];
    json!({
        "id": c.id, "label": c.label, "why": why.iter().map(|m| &m["fallback"]).collect::<Vec<_>>(), "why_msg": why, "members": c.members.len(),
        "stability": c.stability, "finding": stable,
        "note": note["fallback"], "note_msg": note,
    })
}

/// `/api/cluster/{id}` (cluster id `C1`… or a member condition id).
pub async fn cluster(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let (Some(Ok(index)), Some(report)) = (s.related.get(), s.clusters.get()) else {
        let msg = crate::copy_extra::msg("api.clusters.warming_up", json!({}));
        let body = json!({ "detail": msg["fallback"], "detail_msg": msg, "status": "warming_up" });
        return Ok((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response());
    };
    let c = report
        .clusters
        .iter()
        .find(|c| c.id == id)
        .or_else(|| report.cluster_of(index, &id))
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.cluster_unknown", json!({"id": id}))))?;
    let mut body = serde_json::to_value(c).map_err(internal)?;
    body["summary"] = cluster_summary(c);
    body["why"] = body["summary"]["why"].clone();
    body["why_msg"] = body["summary"]["why_msg"].clone();
    if let Some(counters) = body["counterexamples"].as_array_mut() {
        for c in counters {
            let msg = crate::copy_extra::msg("cluster.counterexample", json!({"condition": c["other"]["label"]}));
            c["why"] = msg["fallback"].clone();
            c["why_msg"] = msg;
        }
    }
    body["slice"] = json!({ "rule": report.slice_rule, "size": report.slice_size, "modularity": report.modularity, "seed_ari": report.seed_ari, "bootstrap_ari": report.bootstrap_ari });
    Ok(Json(body).into_response())
}

/// Related report with each neighbour's exact assets (studies, organisations, grants).
fn index_report(s: &AppState, cid: &str, k: usize) -> Result<Value, String> {
    let Some(Ok(index)) = s.related.get() else {
        return Err("related conditions unavailable".into());
    };
    let report = index.related(cid, k).map_err(|e| e.to_string())?;
    let mut body = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    crate::copy_related::decorate(&mut body);
    body["cluster"] = match s.clusters.get() {
        Some(c) => c.cluster_of(index, cid).map_or(Value::Null, cluster_summary),
        None => json!({ "status": "computing" }),
    };
    let atlas = s.atlas();
    if let Some(items) = body["items"].as_array_mut() {
        for item in items {
            let Some(nid) = item["neighbour"]["id"].as_str() else {
                continue;
            };
            let Some(nd) = atlas.disease_idx(nid) else { continue };
            let conn = connections::collect(atlas, &s.graph, nd);
            let mut counts = serde_json::Map::new();
            for f in conn.found.iter().filter(|f| f.exact) {
                let c = counts.entry(f.kind.to_owned()).or_insert(json!(0));
                *c = json!(c.as_u64().unwrap_or(0) + 1);
            }
            let top: Vec<Value> = conn
                .found
                .iter()
                .filter(|f| f.exact && matches!(f.kind, "patient_group" | "registry" | "natural_history" | "trial"))
                .take(3)
                .map(|f| cards::card(s.atlas(), &s.graph, f))
                .collect();
            item["assets"] = json!({ "exact_counts": counts, "top": top });
        }
    }
    Ok(body)
}

#[derive(Deserialize)]
pub struct LangParam {
    lang: Option<String>,
}

/// J1.2: plain two-sentence summary from cited facts (LLM, else template).
pub async fn summary(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(p): Query<LangParam>,
) -> ApiResult {
    let d = condition(&s, &id)?;
    let atlas = s.atlas();
    let facts = llm::condition_facts(atlas, d);
    let label = atlas.disease_at(d).name.clone();
    let lang = lang_of(&p.lang);
    let Some(model) = s.llm.llm.clone() else {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            crate::copy_extra::msg("api.error.llm_unavailable", json!({})),
        ));
    };
    let call = llm::call(&model, &headers, facts.iter().map(|f| f.key.clone()))
        .with_deadline(std::time::Duration::from_secs(3));
    let g = atlas_llm::plain_summary(&model, &call, &label, &facts, &lang).await;
    let activities = s.llm.runtime.register(&g.calls);
    Ok(Json(json!({
        "condition": atlas.disease_ref(d), "text": g.text(), "text_msg": g.message(), "sentences": g.sentences, "origin": g.origin,
        "lang": g.lang, "requested_lang": g.requested_lang, "facts": facts, "validation": g.validation,
        "activities": activities, "models": atlas_llm::model_labels(&g.calls),
    })))
}

#[derive(Deserialize)]
pub struct MessageRequest {
    /// Condition id.
    condition: String,
    /// Card id (`who.id` of a connection card).
    card: String,
    #[serde(default = "default_role")]
    role: UserRole,
    #[serde(default = "default_kind")]
    kind: DraftKind,
    lang: Option<String>,
    sender: Option<String>,
}

fn default_role() -> UserRole {
    UserRole::Parent
}

fn default_kind() -> DraftKind {
    DraftKind::Outreach
}

/// J1.5, J2.5: a ready-to-send message to a card's recipient, every factual sentence cited.
pub async fn message(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<MessageRequest>) -> ApiResult {
    let d = condition(&s, &req.condition)?;
    let atlas = s.atlas();
    let conn = connections::collect(atlas, &s.graph, d);
    let f = conn
        .found
        .iter()
        .find(|f| s.graph.node_ref(f.who).id == req.card)
        .ok_or_else(|| {
            not_found(crate::copy_extra::msg(
                "api.error.card_unknown",
                json!({"card": req.card, "condition": req.condition}),
            ))
        })?;
    let card = cards::card(s.atlas(), &s.graph, f);
    let mut facts = llm::card_facts(f);
    facts.extend(llm::condition_facts(atlas, d).into_iter().take(2));
    let mc = MessageCard {
        recipient: s.graph.node_ref(f.who).label,
        recipient_kind: plain_kind(f.kind).to_owned(),
        channel: card["reach"]["url"].as_str().map(str::to_owned),
        condition: conn.label.clone(),
        facts: facts.clone(),
    };
    let Some(model) = s.llm.llm.clone() else {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            crate::copy_extra::msg("api.error.llm_unavailable", json!({})),
        ));
    };
    let lang = lang_of(&req.lang);
    let call = llm::call(&model, &headers, facts.iter().map(|x| x.key.clone()));
    let draft = atlas_llm::draft_message(&model, &call, &mc, req.role, req.kind, &lang, req.sender.as_deref()).await;
    let activities = s.llm.runtime.register(&draft.body.calls);
    Ok(Json(json!({
        "card": card, "subject": draft.subject,
        "subject_msg": (draft.body.origin == atlas_llm::tasks::Origin::Template).then(|| atlas_llm::tasks::copy::msg("write.template.subject", json!({"condition": mc.condition}))),
        "text": draft.text(), "text_msg": draft.body.message(), "channel": draft.channel,
        "kind": draft.kind, "lang": draft.body.lang, "origin": draft.body.origin,
        "sentences": draft.body.sentences, "validation": draft.body.validation, "facts": facts,
        "activities": activities, "models": atlas_llm::model_labels(&draft.body.calls),
    })))
}

#[derive(Deserialize)]
pub struct ProbeParam {
    #[serde(default)]
    probe: bool,
    connection: Option<String>,
}

/// Settings UI: the configured LLM connections (never a key).
pub async fn llm_connections(State(s): State<AppState>, Query(p): Query<ProbeParam>) -> ApiResult {
    let Some(model) = s.llm.llm.clone() else {
        return Ok(Json(
            json!({ "connections": [], "default": null, "detail": crate::copy_extra::msg("api.error.llm_unavailable", json!({}))["fallback"], "detail_msg": crate::copy_extra::msg("api.error.llm_unavailable", json!({})) }),
        ));
    };
    if let Some(connection) = p.connection {
        let checked = model.check_connection(&connection).await.map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                crate::copy_extra::msg("api.error.llm_unavailable", json!({})),
            )
        })?;
        return Ok(Json(json!(checked)));
    }
    let list = model.list_connections(p.probe).await;
    let default = model.default_connection(false);
    // D48: the model the no-key default runs, as the UI names it (catalog message).
    let default_model_label = list
        .iter()
        .find(|c| c.name == default)
        .and_then(|c| c.model_label.clone());
    Ok(Json(json!({
        "connections": list, "default": default, "default_model_label": default_model_label,
        "default_chain": model.default_chain(), "default_with_own_key": model.default_connection(true),
    })))
}

#[derive(Deserialize)]
pub struct ExportParams {
    condition: String,
}

/// RDF 1.2 Turtle + PROV-O of a condition's subgraph, streamed.
pub async fn export(State(s): State<AppState>, Query(p): Query<ExportParams>) -> Result<Response, ApiError> {
    let d = condition(&s, &p.condition)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(8);
    tokio::task::spawn_blocking(move || {
        for chunk in ttl::Export::new(s.atlas(), &s.graph, d) {
            if tx.blocking_send(Ok(chunk)).is_err() {
                break;
            }
        }
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|c| (c, rx)) });
    let name = p.condition.replace(':', "_");
    Ok((
        [
            (header::CONTENT_TYPE, "text/turtle; charset=utf-8".to_owned()),
            (header::CONTENT_DISPOSITION, format!("inline; filename=\"{name}.ttl\"")),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}
