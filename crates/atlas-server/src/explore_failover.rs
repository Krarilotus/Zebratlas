//! Bounded routing over the request's authorized registry. No new keys or models.
use crate::{explore_query::QueryOutcome, routes::AppState};
use atlas_ask::query::{
    QueryPlan,
    plan::{Direction, Filters, Hop, LinkedEntity, Pattern},
};
use atlas_intake::Prepared;
use atlas_llm::ProviderKind;
use axum::http::{HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

// Reserve eight seconds for source queries inside the BFF's 35-second budget.
const TOTAL: Duration = Duration::from_secs(24);
const ATTEMPT: Duration = Duration::from_secs(8);
const COOLDOWN: Duration = Duration::from_secs(60);
static COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

#[derive(Debug)]
pub struct RoutingFailure {
    pub code: &'static str,
    pub diagnostics: serde_json::Value,
}

fn failed_routing(
    linked: &[String],
    receipts: &[Receipt],
    failed: Vec<serde_json::Value>,
    keyword: &str,
) -> RoutingFailure {
    let limited = receipts
        .iter()
        .any(|r| matches!(r.status, "rate_limited" | "skipped_cooldown"));
    RoutingFailure {
        code: if linked.is_empty() {
            "unsupported_question"
        } else if limited {
            "model_rate_limited"
        } else {
            "model_unavailable"
        },
        diagnostics: json!({"status":"not_executed","linked_ids":linked,"routing":{"route":"unavailable","reason":keyword,"attempted":receipts,"failed_executions":failed},"question_status":"not_checked"}),
    }
}

#[derive(Clone)]
struct Candidate {
    connection: String,
    model: Option<String>,
    quota_group: String,
    primary: bool,
}
#[derive(Clone, Serialize)]
struct Receipt {
    connection: String,
    model: Option<String>,
    status: &'static str,
}

fn fallback_cost_allowed(
    kind: ProviderKind,
    name: &str,
    free: bool,
    demo: bool,
    base: Option<&str>,
    needs_key: bool,
) -> bool {
    let local = base
        .and_then(|base| reqwest::Url::parse(base).ok())
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]"));
    let kisski = name == "kisski"
        && demo
        && base.is_some_and(|base| base.trim_end_matches('/') == atlas_llm::registry::KISSKI_BASE_URL);
    free || (kind == ProviderKind::OpenAiCompatible && (kisski || (local && !needs_key)))
}

fn candidate_headers(headers: &HeaderMap, candidate: &Candidate) -> Result<HeaderMap, String> {
    let mut selected = headers.clone();
    if !candidate.primary {
        let private: Vec<_> = selected
            .keys()
            .filter(|name| {
                let name = name.as_str();
                name.starts_with("x-llm-") || name.starts_with("x-connector-") || name.starts_with("x-atlas-connector-")
            })
            .cloned()
            .collect();
        for name in private {
            selected.remove(name);
        }
    }
    selected.insert(
        "x-llm-connection",
        HeaderValue::from_str(&candidate.connection).map_err(|_| "Invalid configured connection".to_owned())?,
    );
    Ok(selected)
}

fn failure(error: &str) -> &'static str {
    if error.starts_with("free quota reached:") {
        return if error.contains("today's free budget is used up") {
            "daily_budget"
        } else if error.contains("not set up on this server") {
            "not_configured"
        } else {
            "local_quota"
        };
    }
    if error.starts_with("model_rate_limited:") || error.starts_with("rate limited:") {
        "rate_limited"
    } else if error.contains("not replayed") || error.contains("timed out") || error.contains("deadline") {
        "timeout"
    } else {
        "unavailable"
    }
}

fn cooling(group: &str) -> bool {
    let mut map = COOLDOWNS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    map.retain(|_, until| *until > Instant::now());
    map.contains_key(group)
}
fn cool(group: String) {
    let mut map = COOLDOWNS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    map.retain(|_, until| *until > Instant::now());
    if map.len() < 128 {
        map.insert(group, Instant::now() + COOLDOWN);
    }
}

async fn attempts<T, F, Fut>(candidates: Vec<Candidate>, mut call: F) -> (Option<T>, Vec<Receipt>, &'static str)
where
    F: FnMut(Candidate) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    attempts_bounded(candidates, &mut call, TOTAL, ATTEMPT).await
}

async fn attempts_bounded<T, F, Fut>(
    candidates: Vec<Candidate>,
    mut call: F,
    total: Duration,
    per_attempt: Duration,
) -> (Option<T>, Vec<Receipt>, &'static str)
where
    F: FnMut(Candidate) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let start = Instant::now();
    let mut receipts = Vec::new();
    let mut reason = "model_unavailable";
    let candidates: Vec<_> = candidates.into_iter().take(4).collect();
    let count = candidates.len();
    for (index, candidate) in candidates.into_iter().enumerate() {
        if cooling(&candidate.quota_group) {
            receipts.push(Receipt {
                connection: candidate.connection,
                model: candidate.model,
                status: "skipped_cooldown",
            });
            reason = "model_rate_limited";
            continue;
        }
        let remaining = total.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        let receipt = Receipt {
            connection: candidate.connection.clone(),
            model: candidate.model.clone(),
            status: "succeeded",
        };
        let group = candidate.quota_group.clone();
        let fair = remaining / (count - index) as u32;
        match tokio::time::timeout(per_attempt.min(fair), call(candidate)).await {
            Ok(Ok(value)) => {
                receipts.push(receipt);
                return (Some(value), receipts, reason);
            }
            outcome => {
                let status = match outcome {
                    Ok(Err(error)) => failure(&error),
                    Err(_) => "timeout",
                    _ => unreachable!(),
                };
                if status == "rate_limited" {
                    reason = "model_rate_limited";
                    cool(group);
                }
                receipts.push(Receipt { status, ..receipt });
                // Shared admission gates stop all model routes. Monetary exhaustion
                // may continue to a guarded zero-cost provider; remote timeouts may
                // advance to a different provider, never replaying the old one.
                if status == "local_quota" {
                    break;
                }
            }
        }
    }
    (None, receipts, reason)
}

pub async fn understand(
    state: &AppState,
    headers: &HeaderMap,
    prepared: &Prepared,
    linked_ids: &[String],
    lang: &str,
) -> Result<QueryOutcome, RoutingFailure> {
    let mut candidates = Vec::new();
    if let Some(llm) = &state.llm.llm {
        let call = crate::llm::call(llm, headers, linked_ids.iter().cloned());
        let explicit_primary = !call.fallback;
        let primary = call.connection;
        let mut names = vec![primary.clone()];
        let mut others = if explicit_primary {
            Vec::new()
        } else {
            llm.default_chain()
        };
        if !explicit_primary && let Ok(configured) = std::env::var("ATLAS_QUERY_FAILOVER_CONNECTIONS") {
            others.extend(
                configured
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .take(8)
                    .map(str::to_owned),
            );
        }
        names.extend(
            others
                .into_iter()
                .map(|n| if n == "kisski" { "hosted-kisski".into() } else { n })
                .filter(|n| n != &primary)
                .take(16),
        );
        let mut seen = BTreeSet::new();
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Ok(conn) = llm.registry().get(&name) else {
                continue;
            };
            let is_primary = name == primary;
            let info = conn.info(None);
            let eligible_kind = !conn.kind.is_cli() && conn.kind != ProviderKind::Connector;
            let free = llm.registry().free_tier(&name).is_some();
            // Paid-family fallback must remain under its configured free-tier guard.
            let safe_cost = fallback_cost_allowed(
                conn.kind,
                &name,
                free,
                conn.config.demo_only == Some(true),
                conn.config.base_url.as_deref(),
                info.needs_key,
            );
            if (!is_primary || !explicit_primary)
                && (!eligible_kind || !safe_cost || (info.needs_key && !info.key_from_env_available && !free))
            {
                continue;
            }
            let key_scope = if is_primary {
                headers.get("x-llm-key").map(|k| atlas_intake::sha256_hex(k.as_bytes()))
            } else {
                None
            };
            let quota_group = format!(
                "{}:{}:{}",
                conn.kind.as_str(),
                conn.config.base_url.as_deref().unwrap_or(""),
                key_scope
                    .as_deref()
                    .unwrap_or(conn.config.key_env.as_deref().unwrap_or(&name))
            );
            candidates.push(Candidate {
                connection: name,
                model: conn.config.default_model.clone(),
                quota_group,
                primary: is_primary,
            });
        }
    }
    let failed_receipts = Mutex::new(Vec::new());
    let failed_ref = &failed_receipts;
    let (outcome, mut receipts, reason) = attempts(candidates, |candidate| async move {
        let selected = candidate_headers(headers, &candidate)?;
        let outcome = crate::explore_query::understand_once(state, &selected, prepared, linked_ids, lang).await?;
        if outcome.query_execution["status"] == "not_executed" {
            // Preserve only the stable failure classification, never raw provider output.
            let trace = outcome.query_execution["answer"]["tool_trace"].as_array();
            let error = trace.and_then(|trace| trace.iter().rev().find_map(|t| t["error"].as_str())).unwrap_or("model unavailable");
            let status = failure(error);
            let sanitized: Vec<_> = trace.into_iter().flatten().take(16).map(|t| {
                let mut t = t.clone();
                if t.get("error").is_some() { t["error"] = json!(status); }
                t
            }).collect();
            failed_ref.lock().unwrap_or_else(|p| p.into_inner()).push(json!({
                "connection":candidate.connection,"model_provenance":outcome.query_execution["answer"]["model_provenance"],
                "activity":outcome.query_execution["activity"],"tool_trace":sanitized
            }));
            // Keep stable classifications across the lossless answer boundary.
            return Err(match status {
                "rate_limited" => "model_rate_limited:",
                "local_quota" => "free quota reached: the free assistant is switched off right now",
                "daily_budget" => "free quota reached: today's free budget is used up",
                "timeout" => "deadline exceeded",
                _ => "model unavailable",
            }.into());
        }
        Ok((outcome, candidate.primary))
    }).await;
    if let Some((value, _)) = &outcome {
        if let Some(receipt) = receipts.last_mut() {
            if let Some(model) = &value.model {
                receipt.model = Some(model.clone());
            }
            if let Some(connection) = &value.connection {
                receipt.connection = connection.clone();
            }
        }
    }
    let selected_primary = outcome.as_ref().map(|(_, primary)| *primary);
    let failed = failed_receipts.into_inner().unwrap_or_else(|p| p.into_inner());
    let mut outcome = if let Some((value, _)) = outcome {
        value
    } else {
        let value = tokio::time::timeout(
            Duration::from_secs(8),
            crate::explore_query::keyword_once(state, prepared, linked_ids, lang),
        )
        .await;
        match value {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return Err(failed_routing(
                    linked_ids,
                    &receipts,
                    failed,
                    if error.starts_with("Keyword fallback") {
                        "keyword_unsupported"
                    } else {
                        "keyword_store_unavailable"
                    },
                ));
            }
            Err(_) => return Err(failed_routing(linked_ids, &receipts, failed, "keyword_store_timeout")),
        }
    };
    let route = match selected_primary {
        Some(true) => "primary",
        Some(false) => "fallback_model",
        None => "keyword",
    };
    outcome.query_execution["routing"] = json!({"route":route,"connection":outcome.connection,"model":outcome.model,"reason":if route=="primary"{None}else{Some(reason)},"attempted":receipts,"failed_executions":failed});
    Ok(outcome)
}

/// Conservative, closed typed plans over already indexed identifiers. This is
/// a partial discovery query, not an interpretation of every requested constraint.
#[derive(Clone)]
pub struct KeywordJob {
    pub category: &'static str,
    pub plan: QueryPlan,
}

pub fn keyword_jobs(prepared: &Prepared, linked: &[LinkedEntity]) -> Result<Vec<KeywordJob>, String> {
    if linked.is_empty() || linked.len() > 8 {
        return Err("Keyword fallback needs a known indexed entity; choose an exact term or suggested spelling".into());
    }
    let words: BTreeSet<_> = prepared
        .sent
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .collect();
    let has = |terms: &[&str]| terms.iter().any(|s| words.contains(*s));
    let gene = linked.iter().all(|e| e.id.starts_with("HGNC:"));
    let disease = linked.iter().all(|e| e.id.starts_with("MONDO:"));
    if !gene && !disease {
        if linked
            .iter()
            .all(|e| e.id.starts_with("HGNC:") || e.id.starts_with("MONDO:"))
        {
            // Each exact scope has its own plan; this does not assert that a
            // mentioned gene and disease are causally related to one another.
            let genes: Vec<_> = linked.iter().filter(|e| e.id.starts_with("HGNC:")).cloned().collect();
            let diseases: Vec<_> = linked.iter().filter(|e| e.id.starts_with("MONDO:")).cloned().collect();
            let mut jobs = keyword_jobs(prepared, &genes)?;
            jobs.extend(keyword_jobs(prepared, &diseases)?);
            jobs.truncate(4);
            for job in &mut jobs {
                job.plan.limit = 20;
            }
            return Ok(jobs);
        }
        return Err("Keyword fallback needs one unambiguous gene or disease scope".into());
    }
    let plan = QueryPlan {
        version: 1,
        pattern: Pattern::Traverse,
        focus: linked.iter().map(|e| e.id.clone()).collect(),
        hops: vec![],
        filters: Filters::default(),
        output: None,
        limit: 40,
        reasoning: false,
    };
    let hop = |relation: &str, direction| Hop {
        relation: relation.into(),
        direction,
    };
    let groups = has(&[
        "group",
        "groups",
        "organization",
        "organizations",
        "organisation",
        "organisationen",
        "families",
        "support",
        "communities",
    ]);
    let studies = has(&[
        "trial",
        "trials",
        "study",
        "studies",
        "studie",
        "studien",
        "recruit",
        "recruiting",
    ]);
    let resources = has(&[
        "asset",
        "assets",
        "resource",
        "resources",
        "registry",
        "registries",
        "biobank",
        "dataset",
        "biomarker",
        "biomarkers",
        "outcome",
        "outcomes",
    ]);
    let models = has(&["model", "models", "cell", "cells", "animal"]);
    let researchers = has(&[
        "researcher",
        "researchers",
        "expert",
        "experts",
        "scientist",
        "scientists",
    ]);
    let symptoms = disease && has(&["symptom", "symptoms", "phenotype", "phenotypes", "features"]);
    let conditions = has(&[
        "condition",
        "conditions",
        "disease",
        "diseases",
        "diagnosis",
        "krankheit",
    ]);
    let mut jobs = Vec::new();
    let mut add = |category, pattern, hops, output| {
        let mut value = plan.clone();
        value.pattern = pattern;
        value.hops = hops;
        value.output = output;
        jobs.push(KeywordJob { category, plan: value });
    };
    use atlas_core::node::NodeKind;
    // Independent source paths stay independent tables: a registry request is
    // not a patient organisation, and a co-mention is not a collaboration.
    if studies {
        if gene && linked.len() == 1 {
            add("studies", Pattern::StudiesAcrossIdentities, vec![], None);
        } else if disease {
            add(
                "studies",
                Pattern::Traverse,
                vec![hop("studies_condition", Direction::Incoming)],
                Some(NodeKind::Study),
            );
        }
    }
    if resources {
        add(
            "resources",
            Pattern::Traverse,
            vec![hop("resource_for", Direction::Incoming)],
            Some(NodeKind::Asset),
        );
    }
    if models {
        add(
            "models",
            Pattern::Traverse,
            vec![hop("model_of", Direction::Incoming)],
            Some(NodeKind::Asset),
        );
    }
    if researchers {
        let relation = if gene { "about_gene" } else { "about_condition" };
        add(
            "paper_authors",
            Pattern::Traverse,
            vec![
                hop(relation, Direction::Incoming),
                hop("author_of", Direction::Incoming),
            ],
            Some(NodeKind::Person),
        );
        if disease {
            add(
                "gene_paper_authors",
                Pattern::Traverse,
                vec![
                    hop("has_associated_gene", Direction::Outgoing),
                    hop("about_gene", Direction::Incoming),
                    hop("author_of", Direction::Incoming),
                ],
                Some(NodeKind::Person),
            );
        }
    }
    if groups {
        let mut hops = Vec::new();
        if gene {
            hops.push(hop("has_associated_gene", Direction::Incoming));
        }
        hops.push(hop("serves_condition", Direction::Incoming));
        add(
            "patient_organisations",
            Pattern::Traverse,
            hops,
            Some(NodeKind::Organisation),
        );
    }
    if symptoms {
        add(
            "annotated_phenotypes",
            Pattern::Traverse,
            vec![hop("has_phenotype", Direction::Outgoing)],
            Some(NodeKind::Phenotype),
        );
    }
    let selected = studies || resources || models || researchers || groups || symptoms;
    if conditions && gene && linked.len() == 1 && !selected {
        add("associated_conditions", Pattern::AssociatedConditions, vec![], None);
    }
    // Disease genes provide useful anchored records, but do not answer a
    // comparison of mechanisms or establish treatment applicability.
    if disease && (!selected || symptoms) {
        add(
            "associated_genes",
            Pattern::Traverse,
            vec![hop("has_associated_gene", Direction::Outgoing)],
            Some(NodeKind::Gene),
        );
    }
    if !selected && !conditions && gene && linked.len() == 1 {
        add("gene_record", Pattern::GeneRecord, vec![], None);
    }
    jobs.truncate(4);
    let cap = if jobs.len() > 1 { 20 } else { 40 };
    for job in &mut jobs {
        job.plan.limit = cap;
        if job.category == "studies" && job.plan.pattern == Pattern::Traverse && has(&["recruit", "recruiting"]) {
            job.plan.filters.recruiting = Some(true);
        }
    }
    if jobs.is_empty() {
        return Err("Keyword fallback cannot safely express this request".into());
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_headers_drop_primary_credentials_model_and_connector_fields() {
        let mut headers = HeaderMap::new();
        for name in [
            "x-llm-key",
            "x-llm-model",
            "x-connector-session",
            "x-atlas-connector-token",
        ] {
            headers.insert(name, HeaderValue::from_static("private-primary"));
        }
        let candidate = Candidate {
            connection: "kisski".into(),
            model: None,
            quota_group: "fixture".into(),
            primary: false,
        };
        let selected = candidate_headers(&headers, &candidate).unwrap();
        assert_eq!(selected["x-llm-connection"], "kisski");
        assert!(!selected.contains_key("x-llm-key"));
        assert!(!selected.contains_key("x-llm-model"));
        assert!(!selected.contains_key("x-connector-session"));
        assert!(!selected.contains_key("x-atlas-connector-token"));
        assert!(headers.contains_key("x-llm-key"));
    }
    #[test]
    fn generic_compatible_cloud_api_is_not_assumed_free() {
        assert!(!fallback_cost_allowed(
            ProviderKind::OpenAiCompatible,
            "paid-compatible",
            false,
            false,
            Some("https://api.example.invalid/v1"),
            true
        ));
        assert!(fallback_cost_allowed(
            ProviderKind::OpenAiCompatible,
            "kisski",
            false,
            true,
            Some("https://chat-ai.academiccloud.de/v1"),
            true
        ));
        assert!(!fallback_cost_allowed(
            ProviderKind::OpenAiCompatible,
            "kisski",
            false,
            true,
            Some("https://paid.example.invalid/v1"),
            true
        ));
        assert!(fallback_cost_allowed(
            ProviderKind::OpenAiCompatible,
            "local",
            false,
            false,
            Some("http://127.0.0.1:11434/v1"),
            false
        ));
        assert!(!fallback_cost_allowed(
            ProviderKind::OpenRouter,
            "paid",
            false,
            false,
            None,
            true
        ));
    }
    #[tokio::test]
    async fn ordered_mock_failover_shares_prepared_input_and_suppresses_same_quota() {
        let input = atlas_intake::prepare(atlas_intake::Input::Paste("KIF1A studies"), &Default::default()).unwrap();
        let hash = atlas_intake::sha256_hex(input.sent.as_bytes());
        let candidates = vec![
            Candidate {
                connection: "fixture-primary".into(),
                model: None,
                quota_group: "fixture-shared-quota".into(),
                primary: true,
            },
            Candidate {
                connection: "fixture-other-model".into(),
                model: None,
                quota_group: "fixture-shared-quota".into(),
                primary: false,
            },
            Candidate {
                connection: "fixture-kisski".into(),
                model: None,
                quota_group: "fixture-kisski-quota".into(),
                primary: false,
            },
        ];
        let (result, receipts, _) = attempts(candidates, |candidate| {
            assert_eq!(atlas_intake::sha256_hex(input.sent.as_bytes()), hash);
            async move {
                if candidate.primary {
                    Err("model_rate_limited: ignored raw secret".into())
                } else {
                    Ok(7)
                }
            }
        })
        .await;
        assert_eq!(result, Some(7));
        assert_eq!(
            receipts.iter().map(|r| r.status).collect::<Vec<_>>(),
            vec!["rate_limited", "skipped_cooldown", "succeeded"]
        );
        assert!(!serde_json::to_string(&receipts).unwrap().contains("secret"));
    }
    #[tokio::test]
    async fn timeout_can_advance_to_distinct_provider_without_replaying() {
        let candidates = vec![
            Candidate {
                connection: "fixture-timeout".into(),
                model: None,
                quota_group: "fixture-timeout".into(),
                primary: true
            };
            2
        ];
        let (result, receipts, _) =
            attempts::<(), _, _>(candidates, |_| async { Err("deadline exceeded (not replayed)".into()) }).await;
        assert!(result.is_none());
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].status, "timeout");
    }
    #[tokio::test]
    async fn shared_admission_stops_models_but_daily_budget_can_continue() {
        for (error, expected) in [
            ("free quota reached: you reached the free limit for this hour", 1),
            ("free quota reached: all free slots are busy", 1),
            ("free quota reached: the free assistant is switched off right now", 1),
            ("free quota reached: today's free budget is used up", 2),
        ] {
            let candidates = (0..2)
                .map(|i| Candidate {
                    connection: format!("fixture-admission-{i}"),
                    model: None,
                    quota_group: format!("admission-{i}"),
                    primary: i == 0,
                })
                .collect();
            let (value, receipts, _) = attempts(candidates, |c| async move {
                if c.primary { Err(error.into()) } else { Ok(17) }
            })
            .await;
            assert_eq!(receipts.len(), expected);
            assert_eq!(value, (expected == 2).then_some(17));
        }
    }
    #[tokio::test]
    async fn fair_outer_deadline_attempts_last_provider_and_leaves_keyword_budget() {
        let candidates = (0..4)
            .map(|i| Candidate {
                connection: format!("fixture-fair-{i}"),
                model: None,
                quota_group: format!("fair-{i}"),
                primary: i == 0,
            })
            .collect();
        let start = Instant::now();
        let (value, receipts, _) = attempts_bounded(
            candidates,
            |c| async move {
                if c.connection == "fixture-fair-3" {
                    Ok(31)
                } else {
                    std::future::pending::<Result<i32, String>>().await
                }
            },
            Duration::from_millis(160),
            Duration::from_millis(100),
        )
        .await;
        assert_eq!(value, Some(31));
        assert_eq!(receipts.len(), 4);
        assert!(start.elapsed() < Duration::from_millis(300));
        assert_eq!(receipts[3].status, "succeeded");
    }
    #[test]
    fn keyword_plans_use_only_indexed_focus_and_closed_relations() {
        let prepared = atlas_intake::prepare(
            atlas_intake::Input::Paste("KIF1A disease patient organizations"),
            &Default::default(),
        )
        .unwrap();
        let linked = vec![LinkedEntity {
            id: "HGNC:888".into(),
            label: "KIF1A".into(),
        }];
        let jobs = keyword_jobs(&prepared, &linked).unwrap();
        let plan = &jobs[0].plan;
        assert_eq!(plan.focus, vec!["HGNC:888"]);
        assert_eq!(plan.hops[0].relation, "has_associated_gene");
        assert_eq!(plan.hops[1].relation, "serves_condition");
        assert!(!plan.reasoning);
        assert!(keyword_jobs(&prepared, &[]).is_err());
    }

    fn jobs_for(text: &str, id: &str) -> Vec<KeywordJob> {
        let prepared = atlas_intake::prepare(atlas_intake::Input::Paste(text), &Default::default()).unwrap();
        keyword_jobs(
            &prepared,
            &[LinkedEntity {
                id: id.into(),
                label: "Synthetic exact indexed focus".into(),
            }],
        )
        .unwrap()
    }
    #[test]
    fn registry_study_models_are_independent_source_categories_not_patient_groups() {
        let jobs = jobs_for(
            "Which patient registries, natural history studies, experimental models, and biomarkers exist for this disease?",
            "MONDO:0018982",
        );
        assert_eq!(
            jobs.iter().map(|j| j.category).collect::<Vec<_>>(),
            ["studies", "resources", "models"]
        );
        assert!(jobs.iter().all(|j| j.plan.limit == 20 && !j.plan.reasoning));
        assert_eq!(jobs[0].plan.hops[0].relation, "studies_condition");
        assert_eq!(jobs[1].plan.hops[0].relation, "resource_for");
        assert_eq!(jobs[2].plan.hops[0].relation, "model_of");
        let only_registry = jobs_for("patient registries for this condition", "MONDO:0018982");
        assert_eq!(only_registry.len(), 1);
        assert_eq!(only_registry[0].category, "resources");
    }
    #[test]
    fn disease_comparison_and_gene_research_fallback_keep_bounded_factual_paths() {
        let jobs = jobs_for(
            "Rett syndrome: related diseases, shared pathways, distinctive symptoms and mechanistic differences",
            "MONDO:0010726",
        );
        assert_eq!(
            jobs.iter().map(|j| j.category).collect::<Vec<_>>(),
            ["annotated_phenotypes", "associated_genes"]
        );
        assert_eq!(jobs[0].plan.hops[0].direction, Direction::Outgoing);
        assert_eq!(jobs[0].plan.hops[0].relation, "has_phenotype");
        let jobs = jobs_for(
            "AP4B1 researchers, models, resources, proposals and contacts",
            "HGNC:569",
        );
        assert_eq!(
            jobs.iter().map(|j| j.category).collect::<Vec<_>>(),
            ["resources", "models", "paper_authors"]
        );
        assert_eq!(jobs[2].plan.hops[1].relation, "author_of");
        assert!(jobs.iter().all(|j| j.plan.focus == ["HGNC:569"] && j.plan.limit <= 20));
    }
    #[test]
    fn failed_keyword_routing_preserves_safe_attempts_and_exact_linked_scope() {
        let attempts = vec![Receipt {
            connection: "fixture-free".into(),
            model: None,
            status: "skipped_cooldown",
        }];
        let failure = failed_routing(
            &["MONDO:0010726".into()],
            &attempts,
            vec![json!({"activity":{"model_calls":0}})],
            "keyword_unsupported",
        );
        assert_eq!(failure.code, "model_rate_limited");
        assert_eq!(failure.diagnostics["status"], "not_executed");
        assert_eq!(failure.diagnostics["linked_ids"], json!(["MONDO:0010726"]));
        assert_eq!(
            failure.diagnostics["routing"]["attempted"][0]["status"],
            "skipped_cooldown"
        );
        assert_eq!(
            failure.diagnostics["routing"]["failed_executions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(failure.diagnostics.get("results").is_none());
        assert_eq!(
            failed_routing(&[], &attempts, vec![], "keyword_unsupported").code,
            "unsupported_question"
        );
    }

    #[test]
    fn mixed_exact_scopes_remain_separate_and_disease_authors_have_a_real_gene_path() {
        let prepared = atlas_intake::prepare(
            atlas_intake::Input::Paste("AP4B1 and SPG47 researchers, models and resources"),
            &Default::default(),
        )
        .unwrap();
        let linked = vec![
            LinkedEntity {
                id: "HGNC:569".into(),
                label: "AP4B1".into(),
            },
            LinkedEntity {
                id: "MONDO:0013479".into(),
                label: "SPG47".into(),
            },
        ];
        let jobs = keyword_jobs(&prepared, &linked).unwrap();
        assert_eq!(jobs.len(), 4);
        assert!(jobs.iter().all(|j| j.plan.focus.len() == 1 && j.plan.limit == 20));
        assert!(
            jobs.iter()
                .any(|j| j.category == "paper_authors" && j.plan.focus == ["HGNC:569"])
        );
        let disease_jobs = jobs_for("SPG47 researchers", "MONDO:0013479");
        assert_eq!(disease_jobs[1].category, "gene_paper_authors");
        assert_eq!(
            disease_jobs[1]
                .plan
                .hops
                .iter()
                .map(|h| h.relation.as_str())
                .collect::<Vec<_>>(),
            ["has_associated_gene", "about_gene", "author_of"]
        );
    }
}
