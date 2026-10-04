//! Reproducible schema/evaluation CLI; writes only explicitly selected output/cache paths.
use atlas_ask::query::{
    conversation::QueryRequest,
    plan::{Filters, Pattern, SEEDS},
    schema::Lineage,
    *,
};
use atlas_llm::{Cache, CacheMode, Llm, Registry};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};

// Explicitly scripted providers exercise the actual plan/tool loops against nrese.
// They are transport/control-flow checks, never estimates of hosted-model accuracy.
#[derive(Debug)]
struct Script(std::sync::Mutex<std::collections::VecDeque<Value>>);
#[async_trait::async_trait]
impl atlas_llm::provider::Provider for Script {
    fn kind(&self) -> atlas_llm::ProviderKind {
        atlas_llm::ProviderKind::OpenAiCompatible
    }
    async fn probe(&self) -> atlas_llm::Availability {
        atlas_llm::Availability::ready("scripted fixture")
    }
    async fn complete(
        &self,
        _: &atlas_llm::CompletionRequest,
        _: &str,
        _: Option<&atlas_llm::ApiKey>,
    ) -> atlas_llm::Result<atlas_llm::request::ProviderOutput> {
        let text = self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| atlas_llm::LlmError::InvalidRequest("script exhausted".into()))?
            .to_string();
        Ok(atlas_llm::request::ProviderOutput {
            text,
            reported_model: Some("scripted-not-a-model".into()),
            usage: Default::default(),
            stop_reason: None,
            agent_version: Some("query_eval/v1".into()),
            sent: Default::default(),
        })
    }
}
fn scripted(replies: Vec<Value>) -> Llm {
    let mut registry = Registry::default();
    registry.insert(atlas_llm::registry::Connection {
        config: atlas_llm::ConnectionConfig {
            name: "scripted".into(),
            default_model: Some("scripted-not-a-model".into()),
            ..Default::default()
        },
        kind: atlas_llm::ProviderKind::OpenAiCompatible,
        key_policy: atlas_llm::KeyPolicy::None,
        provider: std::sync::Arc::new(Script(std::sync::Mutex::new(replies.into()))),
    });
    Llm::new(registry, Cache::new(std::env::temp_dir(), CacheMode::Off))
}

fn canonical(v: &Value) -> Value {
    if v["boolean"].is_boolean() {
        return v["boolean"].clone();
    }
    let mut rows: Vec<_> = v["results"]["bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let mut terms: Vec<_> = r
                .as_object()
                .into_iter()
                .flat_map(|r| r.iter())
                .map(|(name, t)| {
                    let datatype = t["datatype"]
                        .as_str()
                        .unwrap_or("http://www.w3.org/2001/XMLSchema#string");
                    json!([name, t["type"], t["value"], datatype, t["xml:lang"]]).to_string()
                })
                .collect();
            terms.sort();
            terms
        })
        .collect();
    rows.sort();
    json!(rows)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("schema") {
        let input = PathBuf::from(args.get(2).ok_or("schema INPUT OUTPUT VERSION")?);
        let output = PathBuf::from(args.get(3).ok_or("schema INPUT OUTPUT VERSION")?);
        let version = args.get(4).cloned().unwrap_or_else(|| "unverified-version".into());
        let lineage = Lineage {
            source_url: format!(
                "file:///{}",
                std::fs::canonicalize(&input)?
                    .to_string_lossy()
                    .trim_start_matches("\\\\?\\")
                    .replace('\\', "/")
            ),
            retrieved_at: atlas_core::provenance::rfc3339(input.metadata()?.modified()?),
            version,
            sha256: String::new(),
            record_locator: "graph.ttl; strict full-file streaming scan".into(),
        };
        let start = Instant::now();
        let mut card = SchemaCard::generate(&input, lineage)?;
        if let Some(path) = std::env::var_os("ATLAS_SEMANTIC_UNITS_SPEC") {
            card = card.with_semantic_profile(&PathBuf::from(path))?;
        }
        std::fs::write(&output, serde_json::to_vec_pretty(&card)?)?;
        std::fs::write(output.with_extension("txt"), card.compact())?;
        println!(
            "schema: {} statements, {} classes, {} predicates, {} compact bytes, {:?}",
            card.statements,
            card.classes.len(),
            card.predicates.len(),
            card.compact().len(),
            start.elapsed()
        );
        return Ok(());
    }
    if args.get(1).map(String::as_str) != Some("eval") {
        return Err("use schema INPUT OUTPUT VERSION or eval CARD OUTPUT [--live]".into());
    }
    let card: SchemaCard = serde_json::from_slice(&std::fs::read(args.get(2).ok_or("card required")?)?)?;
    let output = PathBuf::from(args.get(3).ok_or("output required")?);
    let engine = QueryEngine::new(card, "http://127.0.0.1:3160/dataset/sparql", None)?;
    let verified: Value = match std::env::var_os("ATLAS_QUERY_GOLD") {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => serde_json::from_str(include_str!("../../../docs/research/SPARQL-READINESS-results.json"))?,
    };
    if verified["input"]["sha256"] != engine.schema.graph.sha256 {
        return Err("gold answers refer to another release".into());
    }
    let live = args.iter().any(|a| a == "--live");
    let use_script = args.iter().any(|a| a == "--scripted");
    let data = PathBuf::from(std::env::var("RARE_ATLAS_DATA")?);
    let _ = dotenvy::from_path(data.join("../.env"));
    let llm = Llm::new(
        Registry::presets(false)?,
        Cache::new(data.join("cache/nl2sparql/llm"), CacheMode::ReadWrite),
    );
    let cases = [
        (
            "Maria",
            Pattern::GroupsInBranch,
            "MONDO:0005027",
            "epilepsy",
            "Find patient groups serving conditions under the epilepsy branch, including subclass descendants. Return condition, group and officialPage.",
        ),
        (
            "Devon",
            Pattern::StudiesAcrossIdentities,
            "HGNC:11444",
            "STXBP1",
            "Find STXBP1 studies through exact identity links across registries, directly naming the gene or studying an associated condition. Return study and registryPage.",
        ),
        (
            "Dr. Osei",
            Pattern::SourceChain,
            "MONDO:0000023|has_associated_gene|HGNC:15625",
            "licensed assertion",
            "Trace this linked assertion's complete source chain. Return record, url, locator, retrieved, version, hash, hashScope, source, sourceHash, activity and agent.",
        ),
        (
            "Priya",
            Pattern::SharedMechanismModels,
            "HGNC:11444",
            "STXBP1",
            "Find models or cell lines for genes participating in the same mechanism as STXBP1. Return mechanism, gene, asset and officialPage.",
        ),
        (
            "Maria",
            Pattern::FundingForGene,
            "HGNC:11444",
            "STXBP1",
            "Find funding calls for conditions associated with STXBP1, with calls funding either the condition or the gene. Return condition, call and officialPage.",
        ),
        (
            "Devon",
            Pattern::AssociatedConditions,
            "HGNC:11444",
            "STXBP1",
            "Which released disease associations link to STXBP1? Return condition, identifier and source; preserve multiple source rows.",
        ),
        (
            "Devon",
            Pattern::GeneRecord,
            "HGNC:11444",
            "STXBP1",
            "Check the exact STXBP1 gene name and HGNC identifier, its naming record and hash. Return gene, identifier, record, url, locator and hash.",
        ),
        (
            "Priya",
            Pattern::ReleaseCoverage,
            "",
            "",
            "Count released nodes by kind and release status so I can judge data coverage. Return kind, releaseStatus and records.",
        ),
        (
            "Dr. Osei",
            Pattern::MissingEvidence,
            "",
            "",
            "Does any released ra:Edge derive from a record missing source URL, locator, retrieval time, version or hash? Return ASK boolean.",
        ),
        (
            "Dr. Osei",
            Pattern::ReleaseInputs,
            "",
            "",
            "Show the release projection's inputs and software provenance. Return agent, releaseVersion, source, sourceUrl, version, hash and licence.",
        ),
    ];
    let mut results = vec![];
    for (i, (persona, pattern, id, label, question)) in cases.into_iter().enumerate() {
        let linked = if id.is_empty() {
            vec![]
        } else {
            vec![LinkedEntity {
                id: id.into(),
                label: label.into(),
            }]
        };
        let plan = QueryPlan {
            version: 1,
            pattern,
            focus: linked.iter().map(|e| e.id.clone()).collect(),
            hops: vec![],
            filters: Filters::default(),
            output: None,
            limit: 100,
            reasoning: false,
        };
        let baseline = engine.run_sparql(SEEDS[i].1, 100, false).await?;
        let start = Instant::now();
        let compiled = engine.run_plan(&plan, &linked).await;
        let gold = &verified["queries"][i]["nrese_answer"];
        let gold_data = if gold.is_boolean() {
            json!({"boolean":gold})
        } else {
            let bindings = gold
                .as_array()
                .ok_or("invalid checked gold answer")?
                .iter()
                .map(|s| {
                    serde_json::from_str::<Value>(s.as_str().ok_or_else(|| std::io::Error::other("invalid gold row"))?)
                        .map_err(std::io::Error::other)
                })
                .collect::<Result<Vec<_>, _>>()?;
            json!({"results":{"bindings":bindings}})
        };
        let expected = canonical(&gold_data);
        if canonical(&baseline.data) != expected {
            return Err(format!("case {} baseline differs from independently checked gold", i + 1).into());
        }
        results.push(json!({"case":i+1,"persona":persona,"question":question,"mode":"deterministic_plan","correct":compiled.as_ref().is_ok_and(|r|canonical(&r.data)==expected),"invalid":compiled.is_err(),"latency_ms":start.elapsed().as_millis(),"error":compiled.as_ref().err(),"expected":expected,"baseline":baseline,"result":compiled.ok()}));
        println!(
            "case {} deterministic correct={}",
            i + 1,
            results.last().unwrap()["correct"]
        );
        if live || use_script {
            for power_mode in [false, true] {
                let scripted_llm = use_script.then(|| scripted(if power_mode { vec![
                    json!({"action":"call","tool":"describe_schema","arguments":{}}),
                    json!({"action":"call","tool":"run_sparql","arguments":{"query":SEEDS[i].1,"limit":100,"reasoning":false}}),
                    json!({"action":"answer","tool":null,"arguments":{}}),
                ] } else { vec![serde_json::to_value(&plan).unwrap()] }));
                let req = QueryRequest {
                    question: question.into(),
                    linked: linked.clone(),
                    power_mode,
                    connection: Some(if use_script {
                        "scripted".into()
                    } else {
                        llm.default_connection(false)
                    }),
                    ..Default::default()
                };
                let start = Instant::now();
                let answer = engine
                    .understand(scripted_llm.as_ref().unwrap_or(&llm), &req, None)
                    .await;
                let correct = answer
                    .as_ref()
                    .is_ok_and(|a| a.results.last().is_some_and(|r| canonical(&r.data) == expected));
                let invalid = answer.is_err();
                let (query_attempts, invalid_query_attempts) = answer
                    .as_ref()
                    .map(|a| {
                        let calls: Vec<_> = a.tool_trace.iter().filter(|t| t["tool"] == "run_sparql").collect();
                        let invalid = calls
                            .iter()
                            .filter(|t| {
                                t["output"]["error"].as_str().is_some_and(|e| {
                                    [
                                        "invalid SPARQL",
                                        "unknown predicate",
                                        "unknown class",
                                        "only SELECT",
                                        "SERVICE",
                                        "model query constant",
                                        "variable predicates",
                                        "query size",
                                        "negated property",
                                        "custom function",
                                    ]
                                    .iter()
                                    .any(|s| e.contains(s))
                                })
                            })
                            .count();
                        (calls.len(), invalid)
                    })
                    .unwrap_or((0, 0));
                let mode = match (use_script, power_mode) {
                    (true, false) => "scripted_plan",
                    (true, true) => "scripted_free_sparql",
                    (false, false) => "model_plan",
                    (false, true) => "free_sparql",
                };
                let row = json!({"case":i+1,"persona":persona,"question":question,"mode":mode,"correct":correct,"failed":invalid,"query_attempts":query_attempts,"invalid_query_attempts":invalid_query_attempts,"latency_ms":start.elapsed().as_millis(),"error":answer.as_ref().err(),"answer":answer.ok()});
                println!(
                    "case {} {} correct={} invalid={} latency_ms={}",
                    i + 1,
                    row["mode"],
                    correct,
                    invalid,
                    row["latency_ms"]
                );
                results.push(row);
                std::fs::write(
                    &output,
                    serde_json::to_vec_pretty(
                        &json!({"schema":"atlas.nl2sparql-eval","version":1,"graph":engine.schema.graph,"rows":results}),
                    )?,
                )?;
            }
        }
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &json!({"schema":"atlas.nl2sparql-eval","version":1,"graph":engine.schema.graph,"rows":results}),
        )?,
    )?;
    Ok(())
}
