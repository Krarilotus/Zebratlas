//! Reads existing filtered HTTP API responses verbatim; never bypass the privacy filter.
use crate::{hash, protocol::GraphQuery};
use async_trait::async_trait;
use serde_json::{Value, json};

#[async_trait]
pub trait GraphReader: Send + Sync {
    async fn read(&self, query: GraphQuery) -> anyhow::Result<Value>;
}

pub struct HttpGraph {
    origin: url::Url,
    client: reqwest::Client,
}
impl HttpGraph {
    pub fn new(origin: &str) -> anyhow::Result<Self> {
        Ok(Self {
            origin: crate::client::origin(origin)?,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
        })
    }
    pub fn url(&self, query: &GraphQuery) -> anyhow::Result<url::Url> {
        let mut url = self.origin.clone();
        match query {
            GraphQuery::Search { query, limit } => {
                anyhow::ensure!(
                    !query.is_empty() && query.len() <= 512 && (1..=20).contains(limit),
                    "invalid search"
                );
                url.set_path("/api/search");
                url.query_pairs_mut()
                    .append_pair("q", query)
                    .append_pair("limit", &limit.to_string());
            }
            GraphQuery::Node { id }
            | GraphQuery::Edge { id }
            | GraphQuery::Provenance { id }
            | GraphQuery::Connections { id } => {
                anyhow::ensure!(
                    !id.is_empty() && id.len() <= 256 && id != "." && id != ".." && !id.chars().any(char::is_control),
                    "invalid id"
                );
                let mut path = url.path_segments_mut().map_err(|_| anyhow::anyhow!("invalid origin"))?;
                path.clear().push("api");
                match query {
                    GraphQuery::Node { .. } => {
                        path.push("disease").push(id);
                    }
                    GraphQuery::Connections { .. } => {
                        path.push("condition").push(id).push("connections");
                    }
                    _ => {
                        path.push("provenance").push(id);
                    }
                }
            }
        }
        // Graph reads must never invoke an LLM implicitly.
        if matches!(query, GraphQuery::Connections { .. }) {
            url.query_pairs_mut().append_pair("explain", "0");
        }
        Ok(url)
    }
}
#[async_trait]
impl GraphReader for HttpGraph {
    async fn read(&self, query: GraphQuery) -> anyhow::Result<Value> {
        let url = self.url(&query)?;
        let mut response = self.client.get(url.clone()).send().await?;
        anyhow::ensure!(response.status().is_success(), "graph read unavailable");
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 256 * 1024,
                "graph result too large; narrow the query"
            );
            bytes.extend_from_slice(&chunk);
        }
        response_entity(url.as_str(), &bytes)
    }
}

/// Preserve evidence verbatim and identify the actual filtered API response bytes.
pub fn response_entity(url: &str, bytes: &[u8]) -> anyhow::Result<Value> {
    let data: Value = serde_json::from_slice(bytes)?;
    Ok(json!({"data":data,"provenance":{
        "@type":"prov:Entity", "source_url":url,
        "retrieved_at":humantime::format_rfc3339(std::time::SystemTime::now()).to_string(),
        "version":"atlas-api-v1", "sha256":hash(bytes), "record_locator":"$",
        "hash_form":"http-response-bytes", "note":"Underlying graph evidence is retained in data; this hash identifies the retrieved response."
    }}))
}
