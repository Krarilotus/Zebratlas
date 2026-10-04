//! Proof-backed ontology hierarchy reads from the operator's verified nrese profile.
use crate::explore_sparql::{SparqlTriple, node_iri};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Duration};
use tokio::sync::Semaphore;

const PREDICATE: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const MAX_BYTES: usize = 256 * 1024;
static GATE: Semaphore = Semaphore::const_new(2);
static PROFILE: OnceLock<Result<(String, String), String>> = OnceLock::new();

fn profile() -> Result<(String, String), String> {
    PROFILE
        .get_or_init(|| {
            let path = std::env::var("ATLAS_REASONING_MANIFEST")
                .map_err(|_| "Reasoning profile is not configured".to_owned())?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if bytes.len() > MAX_BYTES {
                return Err("Reasoning manifest byte cap exceeded".into());
            }
            let p: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if p["format"] != "zebratlas-verified-reasoning-profile-v1"
                || p["runtime_reasoning"] != true
                || p["engine"] != "nrese"
                || p["rule"] != "rdfs-subclass-transitivity"
                || p["verification_receipts"].as_array().is_none_or(|r| r.len() < 2)
            {
                return Err("Unverified or unsupported reasoning profile".into());
            }
            let proof = p["proof_endpoint"].as_str().ok_or("Missing proof endpoint")?.to_owned();
            let query = p["endpoint"].as_str().ok_or("Missing query endpoint")?.to_owned();
            for endpoint in [&proof, &query] {
                let url = reqwest::Url::parse(endpoint).map_err(|e| e.to_string())?;
                if url.scheme() != "http"
                    || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                {
                    return Err("Reasoning endpoint must be operator-configured loopback HTTP".into());
                }
            }
            Ok((proof, query))
        })
        .clone()
}

async fn get(client: &reqwest::Client, endpoint: &str, params: &[(&str, &str)]) -> Result<Value, String> {
    let response = client
        .get(endpoint)
        .query(params)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Proof read failed ({})", response.status()));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("Proof response byte cap exceeded".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub async fn justify(triples: &[SparqlTriple]) -> Result<Vec<Value>, String> {
    let (proof_endpoint, query_endpoint) = profile()?;
    let _permit = GATE
        .try_acquire()
        .map_err(|_| "Reasoning proof reads are busy".to_owned())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(6), async {
        let mut results = Vec::new();
        for triple in triples.iter().filter(|t| t.relation == "subclass_of").take(16) {
            let source = node_iri(&triple.from);
            let target = node_iri(&triple.to);
            let subj = format!("<{source}>");
            let pred = format!("<{PREDICATE}>");
            let obj = format!("<{target}>");
            let params = [("subj", subj.as_str()), ("pred", pred.as_str()), ("obj", obj.as_str())];
            let proof = get(&client, &proof_endpoint, &params).await?;
            let steps = proof["steps"].as_array().ok_or("Missing proof steps")?;
            if steps.is_empty() || steps.len() > 64 || steps[0]["subject"] != source
                || steps[0]["predicate"] != PREDICATE || steps[0]["object"] != target {
                return Err("Proof does not establish the returned hierarchy triple".into());
            }
            let origin = steps[0]["origin"].as_str().ok_or("Missing proof origin")?.to_owned();
            if origin != "inferred" && origin != "asserted" { return Err("Unsupported proof origin".into()); }
            for step in steps.iter().filter(|s| s["origin"] == "inferred") {
                let premises = step["premises"].as_array().ok_or("Missing inferred premises")?;
                if step["predicate"] != PREDICATE || step["rule"].as_str().is_none_or(str::is_empty)
                    || premises.is_empty() || premises.iter().any(|p| p.as_u64().is_none_or(|i| i as usize >= steps.len())) {
                    return Err("Invalid hierarchy proof rule/premise indexes".into());
                }
            }
            let justification = get(&client, &proof_endpoint, &[
                ("subj", subj.as_str()), ("pred", pred.as_str()), ("obj", obj.as_str()), ("justifications", "one")
            ]).await?;
            if justification["verified"] != true || justification["complete"] != true {
                return Err("Unverified or incomplete hierarchy justification".into());
            }
            let mut records = Vec::new();
            for step in steps.iter().filter(|s| s["origin"] == "asserted") {
                let s = step["subject"].as_str().ok_or("Missing premise subject")?;
                let o = step["object"].as_str().ok_or("Missing premise object")?;
                if step["predicate"] != PREDICATE || ![s, o].iter().all(|v| v.starts_with("https://w3id.org/rare-disease-atlas/id/") && !v.contains(['<', '>', '"', '\\', '\n', '\r'])) {
                    return Err("Unsupported ontology proof premise".into());
                }
                let query = format!("PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> PREFIX prov: <http://www.w3.org/ns/prov#> PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#> PREFIX dcterms: <http://purl.org/dc/terms/> SELECT ?record ?locator ?hash ?source ?sourceHash ?url ?retrieved ?version WHERE {{ ?edge rdf:reifies <<( <{s}> <{PREDICATE}> <{o}> )>> ; prov:wasDerivedFrom ?record . ?record ra:recordLocator ?locator ; ra:sha256 ?hash ; dcterms:isPartOf ?source . ?source ra:sha256 ?sourceHash . OPTIONAL {{ ?source dcterms:source ?url }} OPTIONAL {{ ?source ra:retrievedAt ?retrieved }} OPTIONAL {{ ?source dcterms:hasVersion ?version }} }} LIMIT 32");
                let response = get(&client, &query_endpoint, &[("query", query.as_str()), ("infer", "false")]).await?;
                let rows = response["results"]["bindings"].as_array().ok_or("Missing premise records")?;
                if rows.is_empty() { return Err("Hierarchy proof premise lacks verified source/hash/locator".into()); }
                records.push(json!({"premise":step,"records":rows}));
            }
            if records.is_empty() { return Err("Hierarchy proof has no asserted source premises".into()); }
            results.push(json!({"source":triple.from,"relation":"subclass_of","target":triple.to,
                "origin":origin,"engine":"nrese","rule":"rdfs-subclass-transitivity",
                "proof":proof,"justification":justification,"premise_records":records}));
        }
        Ok(results)
    }).await.map_err(|_| "Hierarchy proof deadline exceeded".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires the verified private nrese hierarchy store and ATLAS_REASONING_MANIFEST"]
    async fn actual_nrese_hierarchy_proof_preserves_source_chain() {
        let triples = [SparqlTriple {
            from: "MONDO:0000015".into(),
            relation: "subclass_of".into(),
            to: "MONDO:0003778".into(),
        }];
        let proofs = justify(&triples).await.expect("actual ontology proof and records");
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0]["origin"], "inferred");
        assert_eq!(proofs[0]["justification"]["verified"], true);
        assert!(!proofs[0]["premise_records"].as_array().unwrap().is_empty());
    }
}
