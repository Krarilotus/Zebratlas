use super::{
    plan::{Direction, Pattern},
    schema::Lineage,
    *,
};
use atlas_core::{
    Graph,
    graph::{RecordWithhold, Relation},
    node::NodeKind,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_RESPONSE: usize = 1024 * 1024;
#[derive(Clone)]
pub struct QueryEngine {
    pub schema: Arc<SchemaCard>,
    pub graph: Option<Arc<Graph>>,
    endpoint: String,
    client: reqwest::Client,
    permits: Arc<tokio::sync::Semaphore>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub backend: String,
    pub data: Value,
    pub provenance: Vec<Value>,
    pub query: Option<String>,
    pub lineage: Lineage,
    pub activity: Value,
    pub latency_ms: u64,
    pub truncated: bool,
    pub notes: Vec<String>,
}

impl QueryEngine {
    /// Operator-only optional startup configuration; never derive a schema path/endpoint from HTTP.
    pub fn from_env(graph: Arc<Graph>) -> Result<Option<Self>, String> {
        let Some(path) = std::env::var_os("ATLAS_QUERY_SCHEMA") else {
            return Ok(None);
        };
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| format!("query schema could not be read: {e}"))?;
        let mut bytes = Vec::new();
        file.take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_RESPONSE {
            return Err("query schema byte cap exceeded".into());
        }
        let schema: SchemaCard = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if schema.schema_version != 1 || schema.graph.sha256.len() != 64 {
            return Err("unsupported query schema or unverified graph hash".into());
        }
        let endpoint =
            std::env::var("ATLAS_QUERY_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:3160/dataset/sparql".into());
        Self::new(schema, endpoint, Some(graph)).map(Some)
    }
    /// Endpoint is operator configuration, never model/client input. Redirects disabled.
    pub fn new(schema: SchemaCard, endpoint: impl Into<String>, graph: Option<Arc<Graph>>) -> Result<Self, String> {
        let endpoint = endpoint.into();
        let url = reqwest::Url::parse(&endpoint).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("invalid operator endpoint".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            schema: Arc::new(schema),
            graph,
            endpoint,
            client,
            permits: Arc::new(tokio::sync::Semaphore::new(2)),
        })
    }
    pub fn relations(&self) -> BTreeSet<String> {
        let mut set: BTreeSet<String> = self
            .schema
            .predicates
            .keys()
            .chain(&self.schema.known_absent)
            .filter_map(|p| p.strip_prefix(RA))
            .filter(|p| p.contains('_') && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(str::to_owned)
            .collect();
        if let Some(graph) = &self.graph {
            set.extend(graph.edges().iter().map(|e| e.relation.as_str().to_owned()));
        }
        set
    }
    async fn request(&self, query: &str, reasoning: bool) -> Result<Value, String> {
        let response = self
            .client
            .post(&self.endpoint)
            .query(&[("infer", if reasoning { "true" } else { "false" })])
            .header("accept", "application/sparql-results+json")
            .header("content-type", "application/sparql-query")
            .body(query.to_owned())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("SPARQL endpoint returned {}", response.status()));
        }
        if response.content_length().is_some_and(|n| n > MAX_RESPONSE as u64) {
            return Err("SPARQL response byte cap exceeded".into());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err("SPARQL response byte cap exceeded".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let data: Value = serde_json::from_slice(&bytes).map_err(|e| format!("invalid SPARQL JSON: {e}"))?;
        if !data["boolean"].is_boolean() && !data["results"]["bindings"].is_array() {
            return Err("endpoint did not return SELECT/ASK results".into());
        }
        Ok(data)
    }
    /// Resolve exact record subjects before reading metadata. Variable-subject
    /// optional provenance joins cause large scans on the operational nrese store.
    async fn source_records(&self, items: &BTreeSet<String>) -> Result<(Vec<Value>, Vec<String>), String> {
        let mut notes = Vec::new();
        if items.len() > 16 {
            notes.push("Source lookup limited to 16 returned graph items.".into());
        }
        let mut records = Vec::new();
        let mut source_hashes = std::collections::BTreeMap::new();
        for item in items.iter().take(16) {
            let query = format!(
                "{PREFIXES}SELECT ?record ?activity WHERE {{ {item} prov:wasDerivedFrom ?record OPTIONAL {{ {item} prov:wasGeneratedBy ?activity }} }} LIMIT 5"
            );
            let found = self.request(&guard::checked(&query, &self.schema, 4)?, false).await?;
            let mut refs: Vec<String> = found["results"]["bindings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    (r["record"]["type"] == "uri")
                        .then(|| r["record"]["value"].as_str())
                        .flatten()
                })
                .filter(|v| v.starts_with(BASE))
                .map(|v| format!("<{v}>"))
                .collect();
            if refs.len() > 4 {
                refs.truncate(4);
                notes.push("Per-item source records truncated at four.".into());
            }
            // Queries may return a source record directly instead of a node.
            if item.starts_with(&format!("<{BASE}record/")) && !refs.contains(item) {
                refs.push(item.clone());
            }
            for record in refs {
                if records.len() == 24 {
                    notes.push("Source-chain results truncated at 24 records.".into());
                    return Ok((records, notes));
                }
                let fields = [
                    ("url", "dcterms:source"),
                    ("locator", "ra:recordLocator"),
                    ("retrieved", "ra:retrievedAt"),
                    ("version", "dcterms:hasVersion"),
                    ("hash", "ra:sha256"),
                    ("source", "dcterms:isPartOf"),
                ];
                let body = fields
                    .iter()
                    .map(|(v, p)| format!("OPTIONAL {{ {record} {p} ?{v} }}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let query = format!(
                    "{PREFIXES}SELECT ?url ?locator ?retrieved ?version ?hash ?source WHERE {{ BIND({record} AS ?knownRecord) {body} }} LIMIT 1"
                );
                let data = self.request(&guard::checked(&query, &self.schema, 1)?, false).await?;
                let Some(mut row) = data["results"]["bindings"].as_array().and_then(|v| v.first()).cloned() else {
                    continue;
                };
                // No fields means the URI has no actual per-record metadata in this projection.
                if row.as_object().is_none_or(|v| v.is_empty()) {
                    continue;
                }
                row["item"] = json!({"type":"uri","value":item.trim_matches(['<','>'])});
                row["record"] = json!({"type":"uri","value":record.trim_matches(['<','>'])});
                if let Some(activity) = found["results"]["bindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| r["record"]["value"].as_str() == Some(record.trim_matches(['<', '>'])))
                    .map(|r| r["activity"].clone())
                    .filter(|a| a["type"] == "uri")
                {
                    row["activity"] = activity;
                }
                if row["source"]["type"] == "uri"
                    && let Some(source) = row["source"]["value"].as_str().filter(|v| v.starts_with(BASE))
                {
                    let source = source.to_owned();
                    if !source_hashes.contains_key(&source) && source_hashes.len() < 8 {
                        let query = format!(
                            "{PREFIXES}SELECT ?sourceHash ?sourceUrl WHERE {{ BIND(<{source}> AS ?knownSource) OPTIONAL {{ <{source}> ra:sha256 ?sourceHash }} OPTIONAL {{ <{source}> dcterms:source ?sourceUrl }} }} LIMIT 1"
                        );
                        let result = self.request(&guard::checked(&query, &self.schema, 1)?, false).await?;
                        let fields = result["results"]["bindings"]
                            .as_array()
                            .and_then(|v| v.first())
                            .cloned()
                            .unwrap_or(Value::Null);
                        source_hashes.insert(source.clone(), fields);
                    }
                    if let Some(fields) = source_hashes.get(&source).filter(|v| !v.is_null()) {
                        if !fields["sourceHash"].is_null() {
                            row["sourceHash"] = fields["sourceHash"].clone();
                        }
                        if !fields["sourceUrl"].is_null() {
                            row["sourceUrl"] = fields["sourceUrl"].clone();
                            if row["url"].is_null() {
                                row["url"] = fields["sourceUrl"].clone();
                                row["url_scope"] = json!("source_artifact");
                            }
                        }
                    }
                }
                records.push(row);
            }
        }
        Ok((records, notes))
    }
    pub async fn run_sparql(&self, query: &str, limit: usize, reasoning: bool) -> Result<QueryResult, String> {
        let checked = guard::checked(query, &self.schema, limit)?;
        let permit = self
            .permits
            .try_acquire()
            .map_err(|_| "SPARQL query capacity reached")?;
        let start = Instant::now();
        let result=tokio::time::timeout(Duration::from_secs(12),async {
            let mut data=self.request(&checked,reasoning).await?;
            let rows=data["results"]["bindings"].as_array_mut();
            let truncated=rows.as_ref().is_some_and(|r|r.len()>limit);
            if let Some(rows)=rows {rows.truncate(limit);}
            let mut notes=vec![];
            if data["results"]["bindings"].as_array().is_some_and(|r|r.is_empty()) { notes.push("No rows matched this query in the configured dataset.".into()); }
            let mut items=BTreeSet::new();
            if let Some(rows)=data["results"]["bindings"].as_array() {
                for row in rows { for term in row.as_object().into_iter().flat_map(|r|r.values()) {
                    if term["type"]=="uri" && let Some(v)=term["value"].as_str() && v.starts_with(BASE) {items.insert(format!("<{v}>"));}
                } }
            }
            // Attach exact records when the projection exposes a derivation; do not invent lineage.
            let mut provenance=vec![];
            if !items.is_empty() {
                match tokio::time::timeout(Duration::from_secs(4),self.source_records(&items)).await {
                    Ok(Ok((records,details)))=> {provenance=records;notes.extend(details);},
                    Ok(Err(e))=>notes.push(format!("Source chain unavailable: {e}")),
                    Err(_)=>notes.push("Source-chain deadline reached; query rows are preserved but per-record proof is unavailable.".into()),
                }
            }
            if provenance.is_empty() {notes.push("No per-record source chain returned; only query/release provenance is verified.".into());}
            let lineage=Lineage{source_url:self.schema.graph.source_url.clone(),retrieved_at:atlas_core::provenance::rfc3339(std::time::SystemTime::now()),version:format!("query-v1/{}",self.schema.graph.version),sha256:digest(checked.as_bytes()),record_locator:format!("query-sha256:{};graph-sha256:{};result-sha256:{}",digest(checked.as_bytes()),self.schema.graph.sha256,digest(data.to_string().as_bytes()))};
            for record in &mut provenance {
                let missing:Vec<_>=["url","locator","retrieved","version","hash"].into_iter().filter(|f|record[*f].is_null()).collect();
                record["unverified_fields"]=json!(missing);
            }
            let activity=json!({"@type":"prov:Activity","@id":format!("urn:atlas:query:{}",lineage.sha256),"prov:used":{"@type":"prov:Entity","source_url":self.schema.graph.source_url,"sha256":self.schema.graph.sha256,"version":self.schema.graph.version},"prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","name":"atlas-ask/query","version":env!("CARGO_PKG_VERSION")},"parameters":{"query_sha256":lineage.sha256,"infer":reasoning,"row_cap":limit,"response_byte_cap":MAX_RESPONSE}});
            Ok(QueryResult{backend:"nrese".into(),data,provenance,query:Some(checked.clone()),lineage,activity,latency_ms:start.elapsed().as_millis() as u64,truncated,notes})
        }).await.map_err(|_|"SPARQL execution/source-chain deadline exceeded".to_owned())?;
        drop(permit);
        result
    }
    pub async fn sample(&self, class: Option<&str>, predicate: Option<&str>) -> Result<QueryResult, String> {
        let body = match (class, predicate) {
            (Some(c), None) if self.schema.classes.contains_key(c) => {
                format!("?s a <{c}> . OPTIONAL {{ ?s rdfs:label ?label }}")
            }
            (None, Some(p)) if self.schema.allows_predicate(p) => format!("?s <{p}> ?o"),
            _ => return Err("sample exactly one known class or predicate".into()),
        };
        self.run_sparql(&format!("{PREFIXES}SELECT * WHERE {{ {body} }} LIMIT 5"), 5, false)
            .await
    }
    pub async fn run_plan(&self, plan: &QueryPlan, linked: &[LinkedEntity]) -> Result<QueryResult, String> {
        plan.validate(linked, &self.relations())?;
        if plan.pattern == Pattern::Traverse
            && plan.hops.len() == 1
            && !plan.reasoning
            && let Some(graph) = &self.graph
        {
            let hop = &plan.hops[0];
            let graph_outputs = plan
                .focus
                .iter()
                .flat_map(|id| graph.incident(id))
                .filter(|inc| {
                    inc.edge.relation.as_str() == hop.relation
                        && inc.outgoing == (hop.direction == Direction::Outgoing)
                        && graph.records_withheld(&inc.edge.records).is_none()
                })
                .all(|inc| graph.node(inc.other).is_some());
            if Relation::parse(&hop.relation).is_some() && graph_outputs {
                return self.fast(plan, graph);
            }
        }
        let observed = self.schema.predicates.keys().cloned().collect();
        let query = plan.compile_sparql_with_metadata(linked, &self.relations(), &observed)?;
        self.run_sparql(&query, plan.limit, plan.reasoning).await
    }
    fn fast(&self, plan: &QueryPlan, graph: &Graph) -> Result<QueryResult, String> {
        let start = Instant::now();
        let hop = &plan.hops[0];
        let mut rows = vec![];
        let mut provenance = vec![];
        let mut seen = BTreeSet::new();
        let mut truncated = false;
        let mut incidents = plan.focus.iter().flat_map(|id| graph.incident(id)).collect::<Vec<_>>();
        incidents.sort_by(|a, b| a.other.cmp(b.other).then_with(|| a.edge.id().cmp(&b.edge.id())));
        for inc in incidents {
            if inc.edge.relation.as_str() != hop.relation || inc.outgoing != (hop.direction == Direction::Outgoing) {
                continue;
            }
            if graph.records_withheld(&inc.edge.records).is_some() {
                continue;
            }
            if graph
                .node(&inc.edge.from)
                .is_some_and(|k| graph.node_withheld(k).is_some())
                || graph
                    .node(&inc.edge.to)
                    .is_some_and(|k| graph.node_withheld(k).is_some())
            {
                continue;
            }
            let Some(key) = graph.node(inc.other) else {
                return Err(
                    "one-hop output lives in atlas-core rather than the connected graph; compile this plan for nrese"
                        .into(),
                );
            };
            if plan.output.is_some_and(|k| k != key.kind) {
                continue;
            }
            if let Some(c) = &plan.filters.country {
                let has = match key.kind {
                    NodeKind::Study => graph.study(key.idx).countries.iter().any(|v| v == c),
                    NodeKind::Organisation => graph.org(key.idx).country.as_ref() == Some(c),
                    NodeKind::Grant => graph.grant(key.idx).country == *c,
                    _ => false,
                };
                if !has {
                    continue;
                }
            }
            if let Some(r) = plan.filters.recruiting
                && (key.kind != NodeKind::Study || graph.study(key.idx).is_recruiting() != r)
            {
                continue;
            }
            if let Some(k) = &plan.filters.kind {
                let kind = match key.kind {
                    NodeKind::Study => graph.study(key.idx).kind.as_str(),
                    NodeKind::Organisation => graph.org(key.idx).kind.as_str(),
                    NodeKind::Asset => graph.asset(key.idx).kind.as_str(),
                    _ => key.kind.as_str(),
                };
                if kind != k {
                    continue;
                }
            }
            let new = seen.insert(inc.other.to_owned());
            if new && rows.len() == plan.limit {
                truncated = true;
                break;
            }
            let node = graph.node_ref(key);
            if new {
                let mut row = json!({"result":{"type":"uri","value":iri("id",&node.id).trim_matches(['<','>'])},"label":{"type":"literal","value":node.label},"edge":{"type":"literal","value":inc.edge.id()},"edge_kind":{"type":"literal","value":inc.edge.kind.as_str()}});
                let official = match key.kind {
                    NodeKind::Organisation => graph
                        .org(key.idx)
                        .contact_url
                        .as_ref()
                        .or(graph.org(key.idx).url.as_ref()),
                    NodeKind::Asset => graph
                        .asset(key.idx)
                        .access
                        .url
                        .as_ref()
                        .or(graph.asset(key.idx).verify_url.as_ref()),
                    NodeKind::Grant => Some(&graph.grant(key.idx).url),
                    _ => graph
                        .node_records(key)
                        .iter()
                        .find_map(|r| graph.record(*r).url.as_ref()),
                };
                if let Some(url) = official.filter(|u| !u.is_empty()) {
                    row["officialPage"] = json!({"type":"uri","value":url});
                }
                rows.push(row);
            }
            for (role, r) in inc
                .edge
                .records
                .iter()
                .map(|r| ("assertion", r))
                .chain(graph.node_records(key).iter().map(|r| ("node", r)))
            {
                let record = graph.record(*r);
                let source = graph.provenance().entity(record.entity);
                let activity = if role == "assertion" {
                    Some(graph.provenance().activity(inc.edge.activity))
                } else {
                    graph.provenance().generator_of(record.entity)
                };
                provenance.push(json!({"item":node.id,"role":role,"edge":if role == "assertion" {Some(inc.edge.id())} else {None},"source_url":record.url.as_ref().unwrap_or(&source.url),"retrieved_at":record.fetched_at.as_ref().or(source.retrieved_at.as_ref()),"version":source.version,"sha256":atlas_core::graph::hex(&record.sha256),"hash_scope":record.hash,"record_locator":record.locator.to_string(),"source_sha256":source.sha256,"activity":activity}));
            }
        }
        let data =
            json!({"head":{"vars":["result","label","edge","edge_kind","officialPage"]},"results":{"bindings":rows}});
        let plan_bytes = serde_json::to_vec(plan).map_err(|e| e.to_string())?;
        let lineage = Lineage {
            source_url: "urn:atlas:in-memory-graph".into(),
            retrieved_at: atlas_core::provenance::rfc3339(std::time::SystemTime::now()),
            version: "typed-plan-v1".into(),
            sha256: digest(&plan_bytes),
            record_locator: format!(
                "plan-sha256:{};result-sha256:{}",
                digest(&plan_bytes),
                digest(data.to_string().as_bytes())
            ),
        };
        let mut notes = if seen.is_empty() {
            vec!["No matching records found under this plan in the in-memory graph.".into()]
        } else {
            vec![]
        };
        if provenance.is_empty() {
            notes.push("No per-record source chain returned; source fields are unverified.".into());
        }
        let activity = json!({"@type":"prov:Activity","@id":format!("urn:atlas:plan:{}",lineage.sha256),"prov:used":provenance.iter().map(|p|json!({"source_url":p["source_url"],"sha256":p["source_sha256"]})).collect::<Vec<_>>(),"prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","name":"atlas-ask/typed-plan","version":env!("CARGO_PKG_VERSION")},"parameters":{"plan":plan,"read_only":true}});
        Ok(QueryResult {
            backend: "rust".into(),
            data,
            provenance,
            query: None,
            lineage,
            activity,
            latency_ms: start.elapsed().as_millis() as u64,
            truncated,
            notes,
        })
    }
}
