//! Provider-independent tools. MCP and connected-model bridges use this same dispatch method.
use super::*;
use atlas_llm::{ApiKey, Call, CompletionRequest, JsonSchema, Llm, LlmCall, Message, check_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

// The search HTTP boundary allows 90 seconds; leave time to project rows/proofs
// and return the accumulated receipts before that outer guard cancels us.
const UNDERSTAND_BUDGET: Duration = Duration::from_secs(80);

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub question: String,
    pub linked: Vec<LinkedEntity>,
    #[serde(default)]
    pub plan: Option<QueryPlan>,
    #[serde(default)]
    pub power_mode: bool,
    #[serde(default)]
    pub connection: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(skip)]
    pub visitor: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct QueryAnswer {
    pub plan: Option<QueryPlan>,
    /// Evidence-backed results are the answer. No freely generated factual prose.
    pub results: Vec<QueryResult>,
    pub model_provenance: Vec<LlmCall>,
    pub tool_trace: Vec<Value>,
    pub latency_ms: u64,
}

fn stopped_answer(
    results: Vec<QueryResult>,
    provenance: Vec<LlmCall>,
    mut trace: Vec<Value>,
    start: Instant,
    schema: &SchemaCard,
    reason: &str,
    error: Option<String>,
) -> Result<QueryAnswer, String> {
    // An initial provider rejection has no receipt/tool history to preserve.
    if results.is_empty() && provenance.is_empty() && trace.is_empty() {
        return Err(error.unwrap_or_else(|| reason.into()));
    }
    let mut coverage = prompt::coverage(&json!({}), &results, schema)?;
    if results.is_empty() {
        coverage["items"][0]["limitation"] =
            json!("No query executed successfully. Failed attempts do not establish that matching records are absent.");
    }
    trace.push(coverage);
    trace.push(json!({"stage":"query_status","status":if results.is_empty(){"not_executed"}else{"partial"},"reason":reason,"error":error,"successful_queries":results.len()}));
    Ok(QueryAnswer {
        plan: None,
        results,
        model_provenance: provenance,
        tool_trace: trace,
        latency_ms: start.elapsed().as_millis() as u64,
    })
}

impl QueryEngine {
    pub fn tools(&self) -> Value {
        json!([
            {"name":"describe_schema","description":"Generated schema and release coverage","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
            {"name":"sample","description":"Sample one known class or predicate","inputSchema":{"type":"object","properties":{"class":{"type":"string"},"predicate":{"type":"string"}},"additionalProperties":false}},
            {"name":"run_sparql","description":"Read-only SELECT/ASK, schema checked, 10s query timeout, 100 rows, 1MiB response","inputSchema":{"type":"object","required":["query"],"properties":{"query":{"type":"string","maxLength":16384},"limit":{"type":"integer","minimum":1,"maximum":100},"reasoning":{"type":"boolean"}},"additionalProperties":false}}
        ])
    }
    pub async fn dispatch(&self, name: &str, args: Value) -> Result<Value, String> {
        let tool = self
            .tools()
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .cloned()
            .ok_or("unknown tool")?;
        check_json(&tool["inputSchema"], &args.to_string())?;
        match name {
            "describe_schema" => Ok(json!({"card":self.schema.compact(),"graph":self.schema.graph})),
            "sample" => serde_json::to_value(self.sample(args["class"].as_str(), args["predicate"].as_str()).await?)
                .map_err(|e| e.to_string()),
            "run_sparql" => serde_json::to_value(
                self.run_sparql(
                    args["query"].as_str().ok_or("query required")?,
                    args["limit"].as_u64().unwrap_or(100) as usize,
                    args["reasoning"].as_bool().unwrap_or(false),
                )
                .await?,
            )
            .map_err(|e| e.to_string()),
            _ => Err("unknown tool".into()),
        }
    }
    pub async fn understand(&self, llm: &Llm, req: &QueryRequest, key: Option<ApiKey>) -> Result<QueryAnswer, String> {
        // The loop times out each await using its remaining budget. An outer
        // timeout would drop its accumulated results, receipts and errors.
        self.understand_inner(llm, req, key).await
    }
    async fn understand_inner(
        &self,
        llm: &Llm,
        req: &QueryRequest,
        key: Option<ApiKey>,
    ) -> Result<QueryAnswer, String> {
        if req.question.len() > 192 * 1024
            || req.linked.len() > 32
            || req.lang.as_ref().is_some_and(|s| s.len() > 64)
            || req.model.as_ref().is_some_and(|s| s.len() > 256)
            || req.connection.as_ref().is_some_and(|s| s.len() > 128)
        {
            return Err("question/entity context too large".into());
        }
        let start = Instant::now();
        if let Some(plan) = &req.plan {
            let result = self.run_plan(plan, &req.linked).await?;
            return Ok(QueryAnswer {
                plan: Some(plan.clone()),
                results: vec![result],
                model_provenance: vec![],
                tool_trace: vec![],
                latency_ms: start.elapsed().as_millis() as u64,
            });
        }
        let named = req.connection.clone().filter(|s| !s.trim().is_empty());
        let fallback = named.is_none() && key.is_none();
        let mut call = Call::new(named.unwrap_or_else(|| llm.default_connection(key.is_some())))
            .with_fallback(fallback)
            .with_private()
            .with_inputs([format!("graph-sha256:{}", self.schema.graph.sha256)]);
        if let Some(k) = key {
            call = call.with_key(k);
        }
        if let Some(v) = &req.visitor {
            call = call.with_visitor(v);
        }
        let mut messages = vec![
            Message::system(prompt::system(&self.schema, req.lang.as_deref().unwrap_or("en"))),
            Message::user(prompt::linked_context(&req.question, &req.linked)),
        ];
        let mut provenance = vec![];
        let mut trace = vec![];
        if !req.power_mode {
            let schema = QueryPlan::schema(&req.linked, &self.relations());
            // Semantic failures are returned to the model once as well as schema failures.
            for attempt in 0..2 {
                let mut request = CompletionRequest::new(messages.clone())
                    .with_schema(JsonSchema::new("atlas_query_plan", schema.clone()))
                    .with_temperature(0.0)
                    .with_max_tokens(1500)
                    .with_deadline(UNDERSTAND_BUDGET.saturating_sub(start.elapsed()));
                if let Some(m) = &req.model {
                    request = request.with_model(m);
                }
                let remaining = UNDERSTAND_BUDGET.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    return stopped_answer(
                        vec![],
                        provenance,
                        trace,
                        start,
                        &self.schema,
                        "deadline_exceeded",
                        Some("understand deadline exceeded".into()),
                    );
                }
                let done = match tokio::time::timeout(remaining, llm.complete(&call, request)).await {
                    Ok(Ok(done)) => done,
                    Ok(Err(error)) => {
                        return stopped_answer(
                            vec![],
                            provenance,
                            trace,
                            start,
                            &self.schema,
                            "model_error",
                            Some(error.to_string()),
                        );
                    }
                    Err(_) => {
                        return stopped_answer(
                            vec![],
                            provenance,
                            trace,
                            start,
                            &self.schema,
                            "deadline_exceeded",
                            Some("understand deadline exceeded".into()),
                        );
                    }
                };
                let text = done.response.text.clone();
                provenance.push(done.provenance);
                let parsed = check_json(&schema, &text)
                    .and_then(|v| serde_json::from_value::<QueryPlan>(v).map_err(|e| e.to_string()));
                let error = match parsed {
                    Ok(plan) => match self.run_plan(&plan, &req.linked).await {
                        Ok(result) => {
                            return Ok(QueryAnswer {
                                plan: Some(plan),
                                results: vec![result],
                                model_provenance: provenance,
                                tool_trace: trace,
                                latency_ms: start.elapsed().as_millis() as u64,
                            });
                        }
                        Err(e) => e,
                    },
                    Err(e) => e,
                };
                trace.push(json!({"stage":"typed_plan","attempt":attempt+1,"error":error}));
                if attempt == 1 {
                    break;
                }
                messages.push(Message::assistant(prompt::invalid_step_context(&text)));
                messages.push(Message::user(format!("Correct this error: {error}")));
            }
        }
        // Researcher mode can explore and refine. At most six calls, no factual prose output.
        messages[0].content.push_str(&prompt::power(&self.tools()));
        let schema = json!({"type":"object","additionalProperties":false,"required":["action","tool","arguments"],"properties":{"action":{"enum":["call","answer"]},"tool":{"enum":["describe_schema","sample","run_sparql",null]},"arguments":{"type":"object"}}});
        let mut results = vec![];
        for step_index in 0..6 {
            let mut request = CompletionRequest::new(messages.clone())
                .with_schema(JsonSchema::new("atlas_query_step", schema.clone()))
                .with_temperature(0.0)
                .with_max_tokens(2500)
                .with_deadline(UNDERSTAND_BUDGET.saturating_sub(start.elapsed()));
            if let Some(m) = &req.model {
                request = request.with_model(m);
            }
            let remaining = UNDERSTAND_BUDGET.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return stopped_answer(
                    results,
                    provenance,
                    trace,
                    start,
                    &self.schema,
                    "deadline_exceeded",
                    Some("understand deadline exceeded".into()),
                );
            }
            let done = match tokio::time::timeout(remaining, llm.complete(&call, request)).await {
                Ok(Ok(done)) => done,
                Ok(Err(error)) => {
                    return stopped_answer(
                        results,
                        provenance,
                        trace,
                        start,
                        &self.schema,
                        "model_error",
                        Some(error.to_string()),
                    );
                }
                Err(_) => {
                    return stopped_answer(
                        results,
                        provenance,
                        trace,
                        start,
                        &self.schema,
                        "deadline_exceeded",
                        Some("understand deadline exceeded".into()),
                    );
                }
            };
            let text = done.response.text.clone();
            provenance.push(done.provenance);
            let step = match check_json(&schema, &text) {
                Ok(s) => s,
                Err(e) => {
                    trace.push(json!({"stage":"tool_step_validation","step":step_index+1,"error":e}));
                    messages.push(Message::assistant(prompt::invalid_step_context(&text)));
                    messages.push(Message::user(format!("Invalid tool step: {e}")));
                    continue;
                }
            };
            if step["action"] == "answer" {
                if !results.is_empty() {
                    match prompt::coverage(&step["arguments"], &results, &self.schema) {
                        Ok(coverage) => trace.push(coverage),
                        Err(error) => {
                            trace.push(json!({"stage":"coverage_validation","step":step_index+1,"error":error}));
                            messages.push(Message::user(format!("Correct coverage declaration: {error}")));
                            continue;
                        }
                    }
                    return Ok(QueryAnswer {
                        plan: None,
                        results,
                        model_provenance: provenance,
                        tool_trace: trace,
                        latency_ms: start.elapsed().as_millis() as u64,
                    });
                }
                trace.push(json!({"stage":"tool_step_validation","step":step_index+1,"error":"No query has executed successfully; answer cannot establish a checked gap."}));
                messages.push(Message::user("Run a query before answering."));
                continue;
            }
            let tool = step["tool"].as_str().unwrap_or("");
            let linked_check = if tool == "run_sparql" {
                guard::linked_entities(step["arguments"]["query"].as_str().unwrap_or(""), &req.linked)
            } else {
                Ok(())
            };
            let dispatched = match linked_check {
                Ok(()) => match tokio::time::timeout(
                    UNDERSTAND_BUDGET.saturating_sub(start.elapsed()),
                    self.dispatch(tool, step["arguments"].clone()),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err("research loop deadline exceeded before this tool completed".into()),
                },
                Err(e) => Err(e),
            };
            let output = match dispatched {
                Ok(value) => {
                    if tool == "run_sparql" {
                        results.push(serde_json::from_value(value.clone()).map_err(|e| e.to_string())?);
                    }
                    value
                }
                Err(e) => json!({"error":e}),
            };
            trace.push(json!({"tool":tool,"arguments":step["arguments"],"output":output}));
            messages.push(Message::assistant(prompt::step_context(&step)));
            messages.push(Message::user(format!(
                "Tool result preview:\n{}",
                prompt::tool_context(tool, &output)
            )));
            if start.elapsed() > UNDERSTAND_BUDGET {
                return stopped_answer(
                    results,
                    provenance,
                    trace,
                    start,
                    &self.schema,
                    "deadline_exceeded",
                    Some("research loop deadline exceeded".into()),
                );
            }
        }
        stopped_answer(
            results,
            provenance,
            trace,
            start,
            &self.schema,
            "budget_exhausted",
            None,
        )
    }
}
