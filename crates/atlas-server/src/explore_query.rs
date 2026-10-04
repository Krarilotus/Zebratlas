//! Lossless Zebra adapter for the canonical prepared-input query boundary.
//! This module does not perform another understanding/model pass or invent query rows.
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use atlas_ask::query::boundary::{QueryConnection, SearchQuery};
use atlas_ask::query::{QueryEngine, QueryResult};
use atlas_core::graph::RecordWithhold;
use atlas_intake::Prepared;
use axum::http::HeaderMap;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use crate::explore_sparql::{SparqlTriple, iri_to_id};
use crate::routes::AppState;

pub struct QueryOutcome {
    /// Complete canonical SearchAnswer; tables, counts, plans and tool traces survive.
    pub query_execution: Value,
    pub result_ids: Vec<String>,
    pub triples: Vec<SparqlTriple>,
    pub backend: String,
    pub sparql: Option<String>,
    pub elapsed_ms: u64,
    pub truncated: bool,
    pub model: Option<String>,
    pub connection: Option<String>,
    pub activities: Vec<String>,
    pub reasoning_proofs: Vec<Value>,
    pub reasoning_warning: Option<String>,
}

#[cfg(test)]
pub(super) static TEST_ENGINE: OnceLock<Arc<QueryEngine>> = OnceLock::new();

fn engine(state: &AppState) -> Result<Arc<QueryEngine>, String> {
    #[cfg(test)]
    if let Some(engine) = TEST_ENGINE.get() {
        return Ok(engine.clone());
    }
    static ENGINE: OnceLock<Result<Arc<QueryEngine>, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut engine = QueryEngine::from_env(state.graph.clone())?.ok_or_else(|| {
                "Canonical query schema is not configured; prepare the matching store schema".to_owned()
            })?;
            // Discovery/linking remains graph-owned, but this UI's query executions must
            // actually run on nrese rather than the core's optional one-hop Rust shortcut.
            engine.graph = None;
            Ok(Arc::new(engine))
        })
        .clone()
}

/// Semantic questions requiring aggregation, negation or comparison cannot be silently
/// reduced to an intent category or the linear typed traversal subset.
fn needs_power_mode(text: &str) -> bool {
    let q = format!(" {} ", text.to_lowercase());
    [
        "how many",
        "count",
        "number of",
        "average",
        "median",
        "at least",
        "at most",
        "except",
        "without",
        "exclude",
        "not ",
        "only ",
        "compare",
        "versus",
        " and ",
        " or ",
        "wie viele",
        "anzahl",
        "mindestens",
        "höchstens",
        "ohne",
        "nicht",
        "nur ",
        "vergleich",
    ]
    .iter()
    .any(|phrase| q.contains(phrase))
}

/// `prepared` is constructed once by intake before entity linking. The authenticated
/// middleware supplies state.llm; the private connector/key stays inside Rust.
pub async fn understand(
    state: &AppState,
    headers: &HeaderMap,
    prepared: &Prepared,
    linked_ids: &[String],
    lang: &str,
) -> Result<QueryOutcome, crate::explore_failover::RoutingFailure> {
    crate::explore_failover::understand(state, headers, prepared, linked_ids, lang).await
}

pub(crate) async fn understand_once(
    state: &AppState,
    headers: &HeaderMap,
    prepared: &Prepared,
    linked_ids: &[String],
    lang: &str,
) -> Result<QueryOutcome, String> {
    if prepared.sent.len() > 32_000 || linked_ids.len() > 8 || lang.len() > 64 {
        return Err("Canonical query input exceeds Zebra limits".into());
    }
    let llm = state.llm.llm.as_ref().ok_or("No selected model is available")?.clone();
    let engine = engine(state)?;
    let call = crate::llm::call(&llm, headers, linked_ids.iter().cloned());
    let input_activity = format!(
        "urn:atlas:zebra-intake:{}",
        atlas_intake::sha256_hex(prepared.sent.as_bytes())
    );
    let asker = atlas_ask::Asker::new(state.atlas.clone(), state.graph.clone(), llm);
    let answer = tokio::time::timeout(
        Duration::from_secs(90),
        asker.understand_search_query(
            &engine,
            SearchQuery {
                prepared,
                linked_ids,
                lang,
                input_activity_id: &input_activity,
                plan: None,
                power_mode: needs_power_mode(&prepared.sent),
            },
            QueryConnection {
                connection: (!call.fallback).then_some(call.connection),
                key: call.key,
                visitor: call.visitor,
                model: None,
            },
        ),
    )
    .await
    .map_err(|_| "Canonical query planning/execution timed out".to_owned())??;
    assemble(state, answer, linked_ids).await
}

/// A source-grounded fallback returns only the actual deterministic plan's rows,
/// with explicit partial coverage. It never invents a natural-language answer.
pub(crate) async fn keyword_once(
    state: &AppState,
    prepared: &Prepared,
    linked_ids: &[String],
    _lang: &str,
) -> Result<QueryOutcome, String> {
    let linked = linked_ids
        .iter()
        .map(|id| {
            crate::nodes::any_ref(&state.atlas, &state.graph, id)
                .filter(|node| {
                    node.id == *id
                        && state
                            .graph
                            .node(id)
                            .is_none_or(|key| state.graph.node_withheld(key).is_none())
                })
                .map(|node| atlas_ask::query::LinkedEntity {
                    id: node.id,
                    label: node.label,
                })
                .ok_or_else(|| "Fallback requires available exact graph identifiers".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let jobs = crate::explore_failover::keyword_jobs(prepared, &linked)?;
    let graph = state.graph.clone();
    let sent = prepared.sent.clone();
    let filters = tokio::task::spawn_blocking(move || crate::explore::keyword_filters(&graph, &sent))
        .await
        .map_err(|_| "Keyword filter preparation failed".to_owned())?;
    let jobs = scoped_keyword_jobs(jobs, &filters);
    let engine = engine(state)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut results = Vec::new();
    let mut trace = Vec::new();
    for job in &jobs {
        let plan = &job.plan;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            trace.push(serde_json::json!({"stage":"keyword_query","category":job.category,"status":"not_executed","reason":"deadline_exceeded"}));
            continue;
        }
        match tokio::time::timeout(remaining, engine.run_plan(plan, &linked)).await {
        Ok(Ok(result)) => {
            trace.push(serde_json::json!({"stage":"keyword_query","category":job.category,"status":"executed","plan":plan,"evidence_query_index":results.len(),"scope":"Only this bounded source path; requested comparisons, suitability and reuse validation remain unchecked."}));
            results.push(result);
        }
        Ok(Err(_)) => trace.push(serde_json::json!({"stage":"keyword_query","category":job.category,"status":"not_executed","reason":"store_query_failed"})),
        Err(_) => trace.push(serde_json::json!({"stage":"keyword_query","category":job.category,"status":"not_executed","reason":"deadline_exceeded"})),
    }
    }
    if results.is_empty() {
        return Err("Keyword store queries did not execute successfully".into());
    }
    let elapsed = results.iter().map(|r| r.latency_ms).sum();
    trace.push(serde_json::json!({"stage":"coverage","arguments":{"coverage":[{"request":"The submitted question","status":"not_checked","evidence_query_indexes":[],"limitation":"Only the listed bounded keyword source paths were queried. Mechanistic differences, ranking, clinical applicability, reuse validation, and unqueried categories have not been answered."}]},"model_declared":false}));
    trace.push(serde_json::json!({"stage":"query_status","status":"partial","reason":"rule_based_fallback","successful_queries":results.len()}));
    let answer = atlas_ask::query::boundary::SearchAnswer {
        answer: atlas_ask::query::conversation::QueryAnswer {
            plan: (jobs.len() == 1).then(|| jobs[0].plan.clone()),
            results,
            model_provenance: vec![],
            tool_trace: trace,
            latency_ms: elapsed,
        },
        linked,
        semantic_focus: linked_ids.to_vec(),
        activity: serde_json::json!({"@type":"prov:Activity","model_calls":0,"route":"rule_based_fallback","input_sha256":atlas_intake::sha256_hex(prepared.sent.as_bytes())}),
    };
    assemble(state, answer, linked_ids).await
}

fn scoped_keyword_jobs(
    jobs: Vec<crate::explore_failover::KeywordJob>,
    filters: &crate::explore::Filters,
) -> Vec<crate::explore_failover::KeywordJob> {
    use atlas_ask::query::plan::{Direction, Hop, Pattern};
    let mut scoped = Vec::new();
    for mut job in jobs {
        let plan = &mut job.plan;
        if plan.pattern == Pattern::StudiesAcrossIdentities
            && (filters.country.is_some() || filters.recruiting.is_some())
        {
            plan.pattern = Pattern::Traverse;
            plan.hops = vec![
                Hop {
                    relation: atlas_journeys::GENE_RELATION.into(),
                    direction: Direction::Incoming,
                },
                Hop {
                    relation: "studies_condition".into(),
                    direction: Direction::Incoming,
                },
            ];
            plan.output = Some(atlas_core::node::NodeKind::Study);
            let mut direct = job.clone();
            direct.category = "gene_named_studies";
            direct.plan.hops = vec![Hop {
                relation: "names_gene".into(),
                direction: Direction::Incoming,
            }];
            direct.plan.filters.country = filters.country.clone();
            direct.plan.filters.recruiting = filters.recruiting;
            scoped.push(direct);
        }
        if job.plan.pattern == Pattern::Traverse {
            if matches!(job.category, "studies" | "patient_organisations") {
                job.plan.filters.country = filters.country.clone().or(job.plan.filters.country.take());
            }
            if job.category == "studies" {
                job.plan.filters.recruiting = filters.recruiting.or(job.plan.filters.recruiting);
            }
            if matches!(job.category, "models" | "resources") {
                job.plan.filters.kind = filters.kind.clone().or(job.plan.filters.kind.take());
            }
        }
        scoped.push(job);
    }
    scoped.truncate(4);
    if scoped.len() > 1 {
        for job in &mut scoped {
            job.plan.limit = 20;
        }
    }
    scoped
}

/// Direct user editing runs the same guarded executor without any model call.
pub async fn run(
    state: &AppState,
    sparql: &str,
    limit: usize,
    reasoning: bool,
    linked_ids: &[String],
    semantic_focus: Option<&[String]>,
) -> Result<QueryOutcome, String> {
    if reasoning && std::env::var_os("ATLAS_REASONING_MANIFEST").is_none() {
        return Err("A verified reasoning profile is not configured".into());
    }
    let linked = atlas_ask::query::link_query_entities(&state.atlas, &state.graph, linked_ids)?;
    let result = engine(state)?.run_sparql(sparql, limit, reasoning).await?;
    let answer = manual_answer(result, sparql, semantic_focus.unwrap_or(linked_ids), linked);
    assemble(state, answer, linked_ids).await
}

/// Keep contextual identifiers separate from the exact query and result receipt.
fn manual_answer(
    result: QueryResult,
    sparql: &str,
    semantic_focus: &[String],
    linked: Vec<atlas_ask::query::LinkedEntity>,
) -> atlas_ask::query::boundary::SearchAnswer {
    let elapsed = result.latency_ms;
    let activity = serde_json::json!({"@type":"prov:Activity","@id":format!("urn:atlas:manual-query:{}",atlas_intake::sha256_hex(sparql.as_bytes())),
        "prov:used":result.activity,"model_calls":0,"query_sha256":atlas_intake::sha256_hex(sparql.as_bytes())});
    let answer = atlas_ask::query::boundary::SearchAnswer {
        answer: atlas_ask::query::conversation::QueryAnswer {
            plan: None,
            results: vec![result],
            model_provenance: vec![],
            tool_trace: vec![],
            latency_ms: elapsed,
        },
        linked,
        semantic_focus: semantic_focus.to_vec(),
        activity,
    };
    answer
}

async fn assemble(
    state: &AppState,
    answer: atlas_ask::query::boundary::SearchAnswer,
    linked_ids: &[String],
) -> Result<QueryOutcome, String> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut triples = Vec::new();
    for result in &answer.answer.results {
        let (found, edges) = graph_items(state, result);
        for id in ordered_graph_ids(&result.data, &found) {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
        triples.extend(edges);
    }
    ids = ids.into_iter().take(100).collect();
    triples.sort_by(|a, b| (&a.from, &a.relation, &a.to).cmp(&(&b.from, &b.relation, &b.to)));
    triples.dedup();
    triples.truncate(160);
    let uses_reasoning = answer
        .answer
        .results
        .iter()
        .any(|r| r.activity["parameters"]["infer"] == true);
    let mut reasoning_proofs = Vec::new();
    let mut reasoning_warning = None;
    if uses_reasoning && std::env::var_os("ATLAS_REASONING_MANIFEST").is_some() {
        let mut candidates: Vec<_> = triples
            .iter()
            .filter(|t| t.relation == "subclass_of")
            .cloned()
            .collect();
        // Candidate edges are projected only after the actual store establishes
        // their proof. Inputs and result identifiers alone never assert an edge.
        for from in linked_ids {
            let from_kind = crate::nodes::any_ref(&state.atlas, &state.graph, from).map(|n| n.kind);
            if !matches!(
                from_kind,
                Some(atlas_core::node::NodeKind::Disease | atlas_core::node::NodeKind::Phenotype)
            ) {
                continue;
            }
            for to in &ids {
                let to_kind = crate::nodes::any_ref(&state.atlas, &state.graph, to).map(|n| n.kind);
                if from != to && from_kind == to_kind && candidates.len() < 16 {
                    candidates.push(SparqlTriple {
                        from: from.clone(),
                        relation: "subclass_of".into(),
                        to: to.clone(),
                    });
                }
            }
        }
        candidates.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        candidates.dedup();
        match crate::explore_reasoning::justify(&candidates).await {
            Ok(proofs) => {
                for p in &proofs {
                    if let (Some(from), Some(to)) = (p["source"].as_str(), p["target"].as_str()) {
                        let t = SparqlTriple {
                            from: from.into(),
                            relation: "subclass_of".into(),
                            to: to.into(),
                        };
                        if !triples.contains(&t) {
                            triples.push(t);
                        }
                    }
                }
                reasoning_proofs = proofs;
            }
            Err(error) => reasoning_warning = Some(error),
        }
    }
    let backends: BTreeSet<_> = answer.answer.results.iter().map(|r| r.backend.as_str()).collect();
    let backend = if backends.is_empty() {
        "none".to_owned()
    } else if backends.len() == 1 {
        backends.into_iter().next().unwrap_or("none").to_owned()
    } else {
        "mixed".to_owned()
    };
    let queries: Vec<_> = answer
        .answer
        .results
        .iter()
        .filter_map(|r| r.query.as_deref())
        .collect();
    let sparql = queries.first().map(|query| (*query).to_owned());
    let model = answer.answer.model_provenance.last().and_then(|p| {
        p.activity
            .parameters
            .get("model.requested")
            .cloned()
            .or_else(|| p.activity.parameters.get("requested_model").cloned())
    });
    let connection = answer
        .answer
        .model_provenance
        .last()
        .and_then(|p| p.activity.parameters.get("connection").cloned());
    let activities = answer
        .answer
        .model_provenance
        .iter()
        .map(|p| p.activity.id.clone())
        .collect();
    let elapsed_ms = answer.answer.latency_ms;
    let truncated = answer.answer.results.iter().any(|r| r.truncated);
    let mut query_execution = serde_json::to_value(&answer).map_err(|e| e.to_string())?;
    query_execution["status"] = serde_json::json!(if answer.answer.results.is_empty() {
        "not_executed"
    } else {
        "executed"
    });
    if let Some(status) = answer
        .answer
        .tool_trace
        .iter()
        .rev()
        .find(|t| t["stage"] == "query_status")
    {
        query_execution["question_status"] = status["status"].clone();
    }
    query_execution["reasoning"] = serde_json::json!({"proofs":reasoning_proofs,"warning":reasoning_warning});
    Ok(QueryOutcome {
        query_execution,
        result_ids: ids.into_iter().collect(),
        triples,
        backend,
        sparql,
        elapsed_ms,
        truncated,
        model,
        connection,
        activities,
        reasoning_proofs,
        reasoning_warning,
    })
}

/// Preserve executed table/row order; CURIE lexical order is not relevance.
fn ordered_graph_ids(data: &Value, allowed: &BTreeSet<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    data["results"]["bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row.as_object().into_iter().flat_map(|fields| fields.values()))
        .filter(|term| term["type"] == "uri")
        .filter_map(|term| term["value"].as_str().and_then(iri_to_id))
        .filter(|id| allowed.contains(id) && seen.insert(id.clone()))
        .collect()
}

fn known(state: &AppState, id: &str) -> bool {
    if state
        .graph
        .node(id)
        .is_some_and(|key| state.graph.node_withheld(key).is_some())
    {
        return false;
    }
    crate::nodes::any_ref(&state.atlas, &state.graph, id).is_some()
}

/// A label or an arbitrary URI in a model response is never enough to create a graph
/// node. Read only actual checked result bindings; preserve non-graph rows unchanged.
fn graph_items(state: &AppState, result: &QueryResult) -> (BTreeSet<String>, Vec<SparqlTriple>) {
    let mut ids = BTreeSet::new();
    let mut triples = Vec::new();
    for row in result.data["results"]["bindings"].as_array().into_iter().flatten() {
        for term in row.as_object().into_iter().flat_map(|row| row.values()) {
            if term["type"] == "uri" {
                if let Some(id) = term["value"].as_str().and_then(iri_to_id).filter(|id| known(state, id)) {
                    ids.insert(id);
                }
            }
        }
        // Canonical Rust traversal rows identify the actual returned assertion.
        if let Some(edge_value) = row["edge"]["value"].as_str() {
            let mapped = edge_value
                .strip_prefix("https://w3id.org/rare-disease-atlas/edge/")
                .and_then(|encoded| iri_to_id(&format!("https://w3id.org/rare-disease-atlas/id/{encoded}")));
            let edge_id = mapped.as_deref().unwrap_or(edge_value);
            if let Some(index) = state.graph.edge_by_id(edge_id) {
                let edge = state.graph.edge(index);
                if state.graph.records_withheld(&edge.records).is_none()
                    && known(state, &edge.from)
                    && known(state, &edge.to)
                {
                    ids.insert(edge.from.clone());
                    ids.insert(edge.to.clone());
                    triples.push(SparqlTriple {
                        from: edge.from.clone(),
                        relation: edge.relation.as_str().to_owned(),
                        to: edge.to.clone(),
                    });
                }
            }
        }
        let term_id = |name: &str| {
            (row[name]["type"] == "uri")
                .then(|| row[name]["value"].as_str().and_then(iri_to_id))
                .flatten()
        };
        if let (Some(from), Some(to), Some(predicate)) = (term_id("s"), term_id("o"), row["p"]["value"].as_str()) {
            if let Some(relation) = predicate
                .strip_prefix("https://w3id.org/rare-disease-atlas/vocab#")
                .or_else(|| (predicate == "http://www.w3.org/2000/01/rdf-schema#subClassOf").then_some("subclass_of"))
            {
                if known(state, &from)
                    && known(state, &to)
                    && (relation == "subclass_of"
                        || atlas_core::graph::Relation::parse(relation).is_some()
                        || [
                            atlas_journeys::GENE_RELATION,
                            atlas_journeys::phenotype_relation(false),
                            atlas_journeys::phenotype_relation(true),
                        ]
                        .contains(&relation))
                {
                    triples.push(SparqlTriple {
                        from,
                        relation: relation.to_owned(),
                        to,
                    });
                }
            }
        }
    }
    (ids, triples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projected_identifiers_keep_actual_row_order_without_alias_or_literal_matches() {
        let ids = ["NCT06948019", "NCT04712812", "NCT00023075"];
        let mut rows: Vec<_> = ids
            .iter()
            .map(|id| json!({"result":{"type":"uri","value":crate::explore_sparql::node_iri(id)}}))
            .collect();
        rows.push(rows[0].clone());
        rows.push(json!({"result":{"type":"literal","value":crate::explore_sparql::node_iri(ids[0])}}));
        let allowed = ids.iter().map(|id| id.to_string()).collect();
        assert_eq!(ordered_graph_ids(&json!({"results":{"bindings":rows}}), &allowed), ids);
    }
    #[test]
    fn recruiting_studies_and_models_keep_filters_on_their_own_source_paths() {
        let prepared = atlas_intake::prepare(
            atlas_intake::Input::Paste("STXBP1 recruiting studies and experimental models"),
            &Default::default(),
        )
        .unwrap();
        let linked = vec![atlas_ask::query::LinkedEntity {
            id: "HGNC:11444".into(),
            label: "STXBP1".into(),
        }];
        let jobs = scoped_keyword_jobs(
            crate::explore_failover::keyword_jobs(&prepared, &linked).unwrap(),
            &crate::explore::Filters {
                country: Some("Germany".into()),
                recruiting: Some(true),
                kind: None,
            },
        );
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].plan.hops[0].relation, "names_gene");
        assert_eq!(jobs[1].plan.hops[1].relation, "studies_condition");
        for job in &jobs[..2] {
            assert_eq!(job.plan.filters.recruiting, Some(true));
            assert_eq!(job.plan.filters.country.as_deref(), Some("Germany"));
        }
        assert_eq!(jobs[2].category, "models");
        assert!(jobs[2].plan.filters.recruiting.is_none());
        assert!(jobs[2].plan.filters.country.is_none());
        assert!(jobs.iter().all(|j| !j.plan.reasoning && j.plan.limit == 20));
    }
    #[test]
    fn aggregation_negation_and_combined_constraints_use_full_guarded_query_path() {
        for query in [
            "How many papers mention STXBP1?",
            "STXBP1 researchers without trials",
            "Studies in Germany and recruiting",
            "Wie viele STXBP1 Studien?",
        ] {
            assert!(needs_power_mode(query), "{query}");
        }
        assert!(!needs_power_mode("Find STXBP1 researchers"));
    }
    fn result(data: Value) -> QueryResult {
        QueryResult {
            backend: "nrese".into(),
            data,
            provenance: vec![],
            query: Some("SELECT ...".into()),
            lineage: atlas_ask::query::schema::Lineage {
                source_url: "urn:test:synthetic".into(),
                retrieved_at: String::new(),
                version: "fixture".into(),
                sha256: "0".repeat(64),
                record_locator: "synthetic".into(),
            },
            activity: json!({"synthetic":true}),
            latency_ms: 1,
            truncated: false,
            notes: vec![],
        }
    }
    #[test]
    fn rerun_semantic_focus_preserves_exact_answer_and_execution_parameters() {
        let rows = json!({"head":{"vars":["count"]},"results":{"bindings":[{"count":{"type":"literal","value":"0"}}]}});
        for (infer, cap) in [(false, 160), (true, 20)] {
            let mut executed = result(rows.clone());
            executed.query = Some("SELECT (COUNT(*) AS ?count) WHERE {} LIMIT 1".into());
            executed.activity = json!({"parameters":{"infer":infer,"row_cap":cap}});
            executed.provenance = vec![json!({"record":{"value":"urn:fixture:actual-record"}})];
            let expected = serde_json::to_value(&executed).unwrap();
            let answer = manual_answer(
                executed,
                "SELECT (COUNT(*) AS ?count) WHERE {} LIMIT 1",
                &["MONDO:9999999".into()],
                vec![],
            );
            let body = serde_json::to_value(answer).unwrap();
            assert_eq!(body["semantic_focus"], json!(["MONDO:9999999"]));
            assert_eq!(body["answer"]["results"][0], expected);
            assert_eq!(body["activity"]["model_calls"], 0);
            assert_eq!(body["answer"]["model_provenance"], json!([]));
        }
    }
    #[tokio::test]
    #[ignore = "requires hash-bound operational schema and actual local nrese; no model calls"]
    async fn actual_nrese_keyword_gene_record_is_partial_and_has_zero_model_calls() {
        // The synthetic state supplies only the prelinked HGNC identity. All
        // returned rows and provenance must come from the actual configured store.
        let state = crate::test_support::state();
        let prepared = atlas_intake::prepare(atlas_intake::Input::Paste("STXBP1"), &Default::default()).unwrap();
        let outcome = keyword_once(&state, &prepared, &["HGNC:11444".into()], "en")
            .await
            .unwrap();
        assert_eq!(outcome.backend, "nrese");
        assert_eq!(outcome.query_execution["question_status"], "partial");
        assert_eq!(outcome.query_execution["activity"]["model_calls"], 0);
        assert!(
            outcome.query_execution["answer"]["model_provenance"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let result = &outcome.query_execution["answer"]["results"][0];
        assert_eq!(result["backend"], "nrese");
        assert!(!result["data"]["results"]["bindings"].as_array().unwrap().is_empty());
        assert!(result["query"].as_str().unwrap().contains("HGNC%3A11444"));
    }
    #[test]
    fn literal_zero_count_is_preserved_without_fabricated_graph_nodes() {
        let state = crate::test_support::state();
        let result = result(
            json!({"head":{"vars":["count"]},"results":{"bindings":[{"count":{"type":"literal","value":"0","datatype":"http://www.w3.org/2001/XMLSchema#integer"}}]}}),
        );
        let (ids, edges) = graph_items(&state, &result);
        assert!(ids.is_empty() && edges.is_empty());
        assert_eq!(result.data["results"]["bindings"][0]["count"]["value"], "0");
    }
    #[test]
    fn graph_projection_requires_actual_canonical_result_uris_and_assertions() {
        let state = crate::test_support::state();
        let uri = |id: &str| json!({"type":"uri","value":crate::explore_sparql::node_iri(id)});
        let result = result(json!({"results":{"bindings":[
            {"result":uri("HGNC:invented"),"label":{"type":"literal","value":"STXBP1"}},
            {"s":uri("NCTTEST0"),"p":{"type":"uri","value":"https://w3id.org/rare-disease-atlas/vocab#studies_condition"},"o":uri("MONDO:9999999")}
        ]}}));
        let (ids, edges) = graph_items(&state, &result);
        assert!(!ids.contains("HGNC:invented"));
        assert!(ids.contains("NCTTEST0") && ids.contains("MONDO:9999999"));
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, "NCTTEST0");
        assert_eq!(edges[0].relation, "studies_condition");
    }

    #[tokio::test]
    async fn failed_planning_keeps_diagnostics_without_claiming_store_execution() {
        let state = crate::test_support::state();
        let trace =
            json!({"stage":"query_status","status":"not_executed","reason":"budget_exhausted","successful_queries":0});
        let answer = atlas_ask::query::boundary::SearchAnswer {
            answer: atlas_ask::query::conversation::QueryAnswer {
                plan: None,
                results: vec![],
                model_provenance: vec![],
                tool_trace: vec![trace.clone()],
                latency_ms: 15,
            },
            linked: vec![],
            semantic_focus: vec![],
            activity: json!({"synthetic":true}),
        };
        let outcome = assemble(&state, answer, &[]).await.unwrap();
        assert_eq!(outcome.backend, "none");
        assert!(outcome.sparql.is_none() && outcome.result_ids.is_empty() && outcome.triples.is_empty());
        assert_eq!(outcome.query_execution["status"], "not_executed");
        assert_eq!(outcome.query_execution["question_status"], "not_executed");
        assert_eq!(outcome.query_execution["answer"]["tool_trace"][0], trace);
    }
}
