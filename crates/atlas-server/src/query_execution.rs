//! D56/D57a join: consume the one private Prepared and exact retrieval IDs.
use crate::{routes::AppState, understand::Understood};
use atlas_ask::query::boundary::{QueryConnection, SearchQuery};
use axum::http::HeaderMap;
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn execute(
    s: &AppState,
    headers: &HeaderMap,
    p: &atlas_intake::Prepared,
    found: &Understood,
    lang: &str,
    enabled: bool,
) -> Value {
    let status = |status| json!({"version":1,"status":status});
    if !enabled {
        return status("disabled");
    }
    if found.focus.is_empty() {
        return status("unlinked");
    }
    // This plan version cannot encode HPO polarity. Do not silently drop it on a rerun.
    if !found.present.is_empty() || !found.excluded.is_empty() {
        return status("unsupported_constraints");
    }
    let Some(model) = &s.llm.llm else {
        return status("model_unavailable");
    };
    // A failed selected personal extraction must never become a hosted planning request.
    if found.metadata["status"] == "unavailable" && headers.contains_key("x-llm-connection") {
        return status("model_unavailable");
    }
    let engine = s
        .query_engine
        .get_or_init(|| atlas_ask::query::QueryEngine::from_env(s.graph.clone()).map(|e| e.map(Arc::new)));
    let Ok(Some(engine)) = engine else {
        return status("executor_unavailable");
    };
    let explicit = ["x-llm-connection", "x-llm-key"]
        .iter()
        .any(|h| headers.get(*h).is_some_and(|v| !v.as_bytes().is_empty()));
    let model = if explicit {
        model.clone()
    } else {
        match model.registry().for_query_planning() {
            Ok(registry) => Arc::new(model.with_registry(registry)),
            Err(_) => return status("model_unavailable"),
        }
    };
    let call = crate::llm::call(&model, headers, []);
    let connection = QueryConnection {
        connection: explicit.then_some(call.connection),
        model: None,
        key: call.key,
        visitor: call.visitor,
    };
    let asker = atlas_ask::Asker::new(s.atlas.clone(), s.graph.clone(), model);
    let activity = format!("urn:atlas:intake:{}", p.sha256);
    match asker
        .understand_search_query(
            engine,
            SearchQuery {
                prepared: p,
                linked_ids: &found.focus,
                lang,
                input_activity_id: &activity,
                plan: None,
                power_mode: false,
            },
            connection,
        )
        .await
    {
        Ok(answer) => {
            let mut execution = serde_json::to_value(answer).expect("execution serializes");
            execution["version"] = json!(1);
            execution["status"] = json!("executed");
            // Labels and kinds come from the source-linked index, never from the planner.
            for linked in execution["linked"].as_array_mut().unwrap() {
                if let Some(hit) = found.hits.iter().find(|h| h.node.id == linked["id"]) {
                    linked["kind"] = json!(hit.node.kind);
                }
            }
            execution
        }
        Err(_) => status("execution_failed"), // no private/provider error strings in public output
    }
}

#[cfg(test)]
#[path = "query_execution_tests.rs"]
mod tests;
