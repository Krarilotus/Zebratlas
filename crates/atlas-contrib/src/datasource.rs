//! Conservative discovery staging. Checks are observations, never ingestion permission.

use std::path::Path;

use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::{ContribError, Result};
use crate::fetch::{Fetched, Fetcher};
use crate::model::{Check, CheckReport, CheckStatus, Contribution, ContributionKind, State, Submission};
use crate::util::now_rfc3339;

const SAMPLE_BYTES: usize = 64 * 1024;
const SOURCES: &str = include_str!("../../../docs/design/SOURCES.md");

fn retain_response(page: &Fetched, data_root: Option<&Path>) -> Value {
    let mut entity = json!({"url": page.record.final_url.as_deref().unwrap_or(&page.record.url),
        "retrieved_at": page.record.retrieved_at, "version": format!("retrieval snapshot {}", page.record.retrieved_at),
        "sha256": page.record.sha256, "record_locator": "whole response", "bytes": page.record.bytes,
        "snapshot": null, "retention": "no successful response bytes"});
    let Some(body) = &page.body else {
        return entity;
    };
    let hash = format!("{:x}", Sha256::digest(body));
    if page.record.sha256.as_deref() != Some(&hash) {
        entity["retention"] = json!("response hash mismatch; snapshot not written");
        return entity;
    }
    let Some(root) = data_root else {
        entity["retention"] = json!("RARE_ATLAS_DATA not configured; response not retained");
        return entity;
    };
    let relative = format!("cache/contrib-datasource/http/{hash}.bin");
    let path = root.join(&relative);
    let retained = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("snapshot parent"))?;
        if path.exists() {
            let previous = std::fs::read(&path)?;
            if previous != *body {
                return Err(std::io::Error::other("existing content-addressed snapshot differs"));
            }
        } else {
            // Unique temporary file so concurrent proposals never overwrite each other's writes.
            let tmp = path.with_extension(format!("{}.tmp", crate::util::new_id()));
            std::fs::write(&tmp, body)?;
            if let Err(error) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                if !path.exists() || std::fs::read(&path)? != *body {
                    return Err(error);
                }
            }
        }
        Ok(())
    })();
    match retained {
        Ok(()) => {
            entity["snapshot"] = json!(relative);
            entity["retention"] = json!("retained");
        }
        Err(_) => entity["retention"] = json!("snapshot write failed; raw response not retained"),
    }
    entity
}

fn check(name: &str, found: bool, detail: Value) -> Check {
    let msg = crate::copy::msg(
        if found {
            "contribute.check.sample_found"
        } else {
            "contribute.check.sample_not_found"
        },
        json!({"kind": name}),
    );
    Check {
        name: name.into(),
        status: if found { CheckStatus::Pass } else { CheckStatus::Warn },
        code: format!("{name}_{}", if found { "found" } else { "not_found" }),
        message: msg["fallback"].as_str().unwrap_or_default().to_owned(),
        message_msg: msg,
        blocking: false,
        detail,
    }
}

fn bounded(s: &str) -> &str {
    let mut end = s.len().min(SAMPLE_BYTES);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn sample(f: &Fetched) -> &str {
    if f.ok() {
        bounded(f.sample.as_deref().unwrap_or_default())
    } else {
        ""
    }
}

/// Locate published licence statements, including DataCite rights and schema.org JSON-LD.
fn licence_evidence(s: &str) -> Vec<Value> {
    fn visit(v: &Value, path: &str, out: &mut Vec<Value>) {
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    let p = format!("{path}/{}", k.replace('~', "~0").replace('/', "~1"));
                    if matches!(
                        k.to_ascii_lowercase().as_str(),
                        "license" | "licence" | "rights" | "rightsuri" | "rightsidentifier"
                    ) && (v.as_str().is_some_and(|s| !s.trim().is_empty())
                        || v.as_object().is_some_and(|o| {
                            o.get("@id")
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.trim().is_empty())
                        }))
                    {
                        out.push(json!({"record_locator": p, "statement": v}));
                    }
                    visit(v, &p, out);
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    visit(v, &format!("{path}/{i}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        visit(&v, "json-pointer:", &mut out);
    }
    let scripts = Regex::new(r"(?is)<script\b[^>]*>(.*?)</script>").unwrap();
    for m in scripts.captures_iter(s) {
        if let Ok(v) = serde_json::from_str::<Value>(&m[1]) {
            visit(
                &v,
                &format!("sample-byte:{}:json-pointer:", m.get(1).unwrap().start()),
                &mut out,
            );
        }
    }
    let statements = Regex::new(r"(?i)SPDX-License-Identifier:\s*[A-Za-z0-9.+-]+|https?://creativecommons\.org/(?:licenses|publicdomain)/[^\s<>\x22']+|\bCC[- ]BY(?:[- ](?:SA|NC|ND)){0,3}(?:[- ]\d\.\d)?\b|\b(?:MIT License|Apache License|GNU (?:General Public|Lesser General Public|Affero General Public) License|Creative Commons(?: [A-Za-z0-9.-]+){0,5}|licensed under[^<>\r\n]{1,160}|licen[cs]e\s*:\s*[^<>\r\n]{1,160}|permission is hereby granted[^<>\r\n]{0,120})").unwrap();
    for m in statements.find_iter(s).take(8) {
        out.push(json!({"record_locator": format!("sample-byte:{}:{}", m.start(), m.end()), "statement": m.as_str()}));
    }
    out.truncate(20);
    out
}

fn formats(f: &Fetched) -> Vec<&'static str> {
    if !f.ok() {
        return Vec::new();
    }
    let s = sample(f);
    let lower = s.to_ascii_lowercase();
    let ct = f
        .record
        .content_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let path = Url::parse(f.record.final_url.as_deref().unwrap_or(&f.record.url))
        .map(|u| u.path().to_ascii_lowercase())
        .unwrap_or_default();
    let mut out = Vec::new();
    let json_value = serde_json::from_str::<Value>(s).ok();
    if ct.contains("json") || json_value.is_some() {
        out.push("JSON");
    }
    if json_value
        .as_ref()
        .is_some_and(|v| v.get("openapi").is_some() || v.get("swagger").is_some())
        || lower
            .lines()
            .any(|l| l.starts_with("openapi:") || l.starts_with("swagger:"))
    {
        out.push("OpenAPI");
    }
    let lines: Vec<_> = s
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .take(3)
        .collect();
    if ct.contains("csv")
        || path.ends_with(".csv")
        || (lines.len() > 1
            && lines[0].contains(',')
            && lines[0].split(',').count() == lines[1].split(',').count()
            && !lines[0].contains('<'))
    {
        out.push("CSV");
    }
    if ct.contains("tab-separated") || path.ends_with(".tsv") || lines.first().is_some_and(|l| l.contains('\t')) {
        out.push("TSV");
    }
    if lines.first().is_some_and(|l| {
        let cols: Vec<_> = l.split('\t').collect();
        ["subject_id", "predicate_id", "object_id"]
            .iter()
            .all(|k| cols.contains(k))
    }) {
        out.push("SSSOM");
    }
    if ct.contains("rdf")
        || ct.contains("turtle")
        || ct.contains("n-triples")
        || ct.contains("ld+json")
        || lower.contains("<rdf:rdf")
        || lower
            .lines()
            .any(|l| l.starts_with("@prefix ") || l.starts_with("prefix "))
    {
        out.push("RDF");
    }
    out
}

fn advertised_formats(f: &Fetched) -> Vec<Value> {
    let Some(base) = Url::parse(f.record.final_url.as_deref().unwrap_or(&f.record.url)).ok() else {
        return Vec::new();
    };
    let links = Regex::new(r#"(?i)href\s*=\s*[\x22']([^\x22']+)[\x22']"#).unwrap();
    let mut out = Vec::new();
    for link in links.captures_iter(sample(f)).take(100) {
        let Some(url) = base.join(&link[1]).ok() else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        let path = url.path().to_ascii_lowercase();
        let format = if path.ends_with(".sssom.tsv") {
            "SSSOM"
        } else if path.ends_with(".csv") {
            "CSV"
        } else if path.ends_with(".tsv") {
            "TSV"
        } else if path.ends_with(".json") {
            "JSON"
        } else if [".rdf", ".ttl", ".nt", ".nq", ".jsonld"]
            .iter()
            .any(|e| path.ends_with(e))
        {
            "RDF"
        } else {
            continue;
        };
        out.push(json!({"format": format, "url": url.as_str(), "record_locator": format!("sample-byte:{}:{}", link.get(1).unwrap().start(), link.get(1).unwrap().end()), "download_verified": false}));
    }
    out
}

fn identifiers(s: &str) -> Vec<Value> {
    let re = Regex::new(r"(?i)\b(MONDO|OMIM|ORPHA|ORPHANET|HGNC|HPO|HP)[:_][0-9]+\b|\bNCT[0-9]{8}\b").unwrap();
    let mut out = Vec::new();
    for m in re.find_iter(s).take(100) {
        let token = m.as_str().to_ascii_uppercase();
        let prefix = token.split([':', '_']).next().unwrap_or_default();
        let system = if token.starts_with("NCT") {
            "NCT"
        } else if prefix == "HP" {
            "HPO"
        } else if prefix == "ORPHANET" {
            "ORPHA"
        } else {
            prefix
        };
        if !out.iter().any(|v: &Value| v["system"] == system) {
            out.push(json!({"system": system, "example": m.as_str(), "record_locator": format!("sample-byte:{}:{}", m.start(), m.end())}));
        }
    }
    out
}

fn urls(s: &str) -> Vec<String> {
    Regex::new(r#"https?://[^\s<>\x22'`\]\)\}]+"#)
        .unwrap()
        .find_iter(s)
        .map(|m| m.as_str().trim_end_matches([',', '.', ';']).to_string())
        .collect()
}

fn same_resource(proposed: &str, known: &str) -> bool {
    match (Url::parse(proposed), Url::parse(known)) {
        (Ok(a), Ok(b)) => {
            a.host_str() == b.host_str()
                && a.port_or_known_default() == b.port_or_known_default()
                && (b.path() == "/"
                    || a.path().trim_end_matches('/') == b.path().trim_end_matches('/')
                    || (b.path().ends_with('/') && a.path().starts_with(b.path())))
        }
        _ => false,
    }
}

fn known_sources(url: &str) -> Check {
    let root = std::env::var("RARE_ATLAS_DATA").ok();
    known_sources_from(url, root.as_deref().map(Path::new))
}

fn known_sources_from(url: &str, root: Option<&Path>) -> Check {
    let mut matches = Vec::new();
    let mut inputs = vec![
        json!({"url": "repo:docs/design/SOURCES.md", "retrieved_at": now_rfc3339(), "version": "compiled snapshot", "sha256": format!("{:x}", Sha256::digest(SOURCES.as_bytes())), "record_locator": "whole document"}),
    ];
    for known in urls(SOURCES) {
        if same_resource(url, &known) {
            matches.push(json!({"url": known, "source": "docs/design/SOURCES.md"}));
        }
    }
    let mut cache_status = "RARE_ATLAS_DATA not configured".to_string();
    if let Some(root) = root {
        let path = root.join("cache/kg-scan/resources.json");
        cache_status = "optional kg-scan cache missing".into();
        if path.exists() {
            cache_status = "kg-scan cache unreadable or over 4 MiB; comparison incomplete".into();
        }
        if path.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024) {
            match std::fs::read(&path) {
                Ok(bytes) if bytes.len() <= 4 * 1024 * 1024 => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) if v["schema"] == "kg_scan.resources" && v["version"] == 1 && v["records"].is_array() => {
                        cache_status = "loaded".into();
                        inputs.push(json!({"url": "cache:kg-scan/resources.json", "retrieved_at": now_rfc3339(), "version": v["version"], "sha256": format!("{:x}", Sha256::digest(&bytes)), "record_locator": "whole document"}));
                        for (i, record) in v["records"].as_array().expect("validated records").iter().enumerate() {
                            let mut resource_urls = Vec::new();
                            if let Some(home) = record["url"].as_str() {
                                resource_urls.push((home, format!("/records/{i}/url")));
                            }
                            if let Some(endpoints) = record.pointer("/access/endpoints").and_then(Value::as_array) {
                                for (j, endpoint) in endpoints.iter().enumerate() {
                                    if let Some(url) = endpoint.as_str() {
                                        resource_urls.push((url, format!("/records/{i}/access/endpoints/{j}")));
                                    }
                                }
                            }
                            for (known, locator) in resource_urls {
                                if same_resource(url, known) {
                                    matches.push(json!({"url": known, "source": "cache/kg-scan/resources.json", "record_locator": locator, "resource_id": record["id"]}));
                                }
                            }
                        }
                    }
                    _ => {
                        cache_status =
                            "kg-scan cache invalid JSON or unsupported schema/version; comparison incomplete".into()
                    }
                },
                _ => cache_status = "kg-scan cache unreadable or over 4 MiB; comparison incomplete".into(),
            }
        }
    }
    let mut result = check(
        "known_source",
        !matches.is_empty(),
        json!({"matches": matches, "inputs": inputs, "optional_cache": cache_status}),
    );
    result.status = if matches.is_empty() {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    if cache_status.contains("incomplete") {
        result.status = CheckStatus::Warn;
    }
    result.set_message(crate::copy::msg(
        if matches.is_empty() {
            "contribute.check.known_source_unmatched"
        } else {
            "contribute.check.known_source_matched"
        },
        json!({}),
    ));
    result
}

pub(crate) async fn run(id: &str, sub: &Submission, fetcher: &dyn Fetcher, others: &[Contribution]) -> CheckReport {
    let source = sub.data_source.as_ref().expect("validated data_source submission");
    if source.url.is_empty() {
        let mut manual = check("resource_url", false, json!({"description_only": true}));
        manual.code = "no_url".into();
        manual.status = CheckStatus::Skipped;
        manual.set_message(crate::copy::msg("contribute.check.description_only", json!({})));
        return CheckReport {
            checked_at: now_rfc3339(),
            agent: crate::AGENT.into(),
            checks: vec![manual],
            ..Default::default()
        };
    }
    let page = fetcher.fetch(&source.url).await;
    let root = std::env::var("RARE_ATLAS_DATA").ok();
    let data_root = root.as_deref().map(Path::new);
    let mut snapshots = vec![retain_response(&page, data_root)];
    let mut checks = vec![crate::checks::url_check("resource_url", Some(&page))];
    let mut licence = licence_evidence(sample(&page));
    let mut fetched = vec![page.record.clone()];
    // Follow at most two explicit same-origin licence links. Each uses the same SSRF-safe fetcher.
    if licence.is_empty() && page.ok() {
        let base = Url::parse(page.record.final_url.as_deref().unwrap_or(&source.url)).ok();
        let links = Regex::new(r#"(?i)href\s*=\s*[\x22']([^\x22']+)[\x22']"#).unwrap();
        let mut seen = Vec::new();
        for link in links.captures_iter(sample(&page)) {
            let Some(url) = base.as_ref().and_then(|b| b.join(&link[1]).ok()) else {
                continue;
            };
            if base.as_ref().is_none_or(|b| b.origin() != url.origin())
                || !url.path().to_ascii_lowercase().contains("licen")
                || seen.contains(&url)
            {
                continue;
            }
            seen.push(url.clone());
            let license_page = fetcher.fetch(url.as_str()).await;
            snapshots.push(retain_response(&license_page, data_root));
            let mut safety = crate::checks::url_check("licence_url", Some(&license_page));
            safety.name = format!("licence_url_{}", seen.len());
            checks.push(safety);
            let mut evidence = licence_evidence(sample(&license_page));
            // A LICENSE file itself is a published statement; its content still needs review.
            if evidence.is_empty() && license_page.ok() && !sample(&license_page).trim().is_empty() {
                evidence.push(json!({"record_locator": "sample-byte:0", "statement": sample(&license_page).chars().take(250).collect::<String>()}));
            }
            for e in &mut evidence {
                e["url"] = json!(url.as_str());
            }
            licence.extend(evidence);
            fetched.push(license_page.record);
            if seen.len() >= 2 {
                break;
            }
        }
    }
    for e in &mut licence {
        if e.get("url").is_none() {
            e["url"] = json!(page.record.final_url.as_deref().unwrap_or(&source.url));
        }
    }
    let detected_formats = formats(&page);
    let advertised = advertised_formats(&page);
    let detected_ids = identifiers(sample(&page));
    checks.push(check("licence", !licence.is_empty(), json!({"evidence": licence, "declared": source.licence, "declared_spdx_id": source.spdx_id, "verified_permission": false})));
    checks.push(check("formats", !detected_formats.is_empty() || !advertised.is_empty(), json!({"detected": detected_formats, "advertised": advertised, "sample_limit_bytes": SAMPLE_BYTES, "record_locator": "response-byte:0..65536", "url": source.url})));
    checks.push(check("identifiers", !detected_ids.is_empty(), json!({"detected": detected_ids, "declared": source.identifier_systems, "sample_limit_bytes": SAMPLE_BYTES, "url": source.url})));
    checks.push(known_sources(page.record.final_url.as_deref().unwrap_or(&source.url)));
    let retained = snapshots.iter().filter(|s| s["retention"] == "retained").count();
    let mut snapshot_check = check(
        "source_snapshots",
        retained == snapshots.len(),
        json!({"records": snapshots}),
    );
    snapshot_check.set_message(crate::copy::msg(
        "contribute.check.snapshots",
        json!({"retained": retained, "total": snapshots.len()}),
    ));
    checks.push(snapshot_check);
    let duplicates: Vec<_> = others
        .iter()
        .filter(|c| c.id != id && c.state != State::Rejected)
        .filter(|c| {
            c.submission
                .data_source
                .as_ref()
                .is_some_and(|s| same_resource(&source.url, &s.url))
        })
        .map(|c| c.id.clone())
        .collect();
    let mut duplicate = check(
        "duplicate_source",
        !duplicates.is_empty(),
        json!({"contributions": duplicates}),
    );
    duplicate.status = if duplicates.is_empty() {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    duplicate.set_message(crate::copy::msg(
        if duplicates.is_empty() {
            "contribute.check.duplicate_source_unmatched"
        } else {
            "contribute.check.duplicate_source_matched"
        },
        json!({}),
    ));
    checks.push(duplicate);
    CheckReport {
        checked_at: now_rfc3339(),
        agent: crate::AGENT.into(),
        checks,
        fetched,
        ..Default::default()
    }
}

/// Candidate queue envelope; no records are admitted to the graph by this export.
pub fn export(contributions: &[Contribution], histories: &[Value]) -> Value {
    let generated_at = now_rfc3339();
    let export_id = crate::util::new_id();
    let export_activity = format!("activity:discovery-export/{export_id}");
    let mut accepted: Vec<_> = contributions
        .iter()
        .filter(|c| c.kind == ContributionKind::DataSource && c.state == State::Accepted)
        .collect();
    accepted.sort_by(|a, b| a.id.cmp(&b.id));
    let candidates: Vec<_> = accepted.into_iter().filter_map(|c| {
        let source = c.submission.data_source.as_ref()?;
        let public = c.public();
        let bytes = serde_json::to_vec(&public).expect("public contribution serializes");
        Some(json!({
            "@id": format!("urn:atlas:discovery-candidate:{}:v{}:{export_id}", c.id, c.version),
            "@type": "prov:Entity",
            "id": format!("contrib:{}", c.id), "url": source.url,
            "resource_kind": source.resource_kind, "resource_kind_other": source.resource_kind_other, "licence": source.licence,
            "spdx_id": source.spdx_id, "identifier_systems": source.identifier_systems, "identifier_systems_other": source.identifier_systems_other,
            "description": source.description, "consent": source.consent,
            "status": "accepted_for_discovery", "graph_import_allowed": false,
            "proposed_by": {"agent": c.contributor_agent(), "organisation": c.contributor.organisation},
            "accepted_by": c.review.as_ref().map(|r| &r.reviewer),
            "accepted_at": c.review.as_ref().map(|r| &r.at),
            "review_reason": c.review.as_ref().map(|r| &r.reason),
            "checks": c.checks,
            "source": {"url": format!("/api/contribute/{}", c.id), "retrieved_at": c.updated_at, "version": c.version.to_string(), "sha256": format!("{:x}", Sha256::digest(&bytes)), "record_locator": format!("contrib:{}/v{}", c.id, c.version)},
            "submission": public,
            "prov:wasDerivedFrom": {"@id": c.entity()},
            "prov:qualifiedDerivation": {"@type": "prov:Derivation", "prov:entity": {"@id": c.entity()}, "prov:hadActivity": {"@id": export_activity}},
            "prov:wasGeneratedBy": {"@id": export_activity},
            "prov:generatedAtTime": {"@value": generated_at, "@type": "xsd:dateTime"}
        }))
    }).collect();
    let bytes = serde_json::to_vec(&candidates).expect("candidates serialize");
    let mut provenance = histories.to_vec();
    provenance.push(json!({"@context": {"prov": crate::prov::CONTEXT, "xsd": "http://www.w3.org/2001/XMLSchema#"},
        "@graph": [{"@id": export_activity, "@type": "prov:Activity", "prov:wasAssociatedWith": {"@id": crate::model::Agent::software().id},
            "prov:startedAtTime": {"@value": generated_at, "@type": "xsd:dateTime"},
            "prov:endedAtTime": {"@value": now_rfc3339(), "@type": "xsd:dateTime"},
            "prov:used": candidates.iter().map(|c| c["prov:wasDerivedFrom"].clone()).collect::<Vec<_>>(),
            "prov:qualifiedUsage": candidates.iter().map(|c| json!({"@type": "prov:Usage", "prov:entity": c["prov:wasDerivedFrom"], "prov:hadRole": {"@id": "urn:atlas:role:accepted-source-proposal"}})).collect::<Vec<_>>()},
            {"@id": crate::model::Agent::software().id, "@type": "prov:SoftwareAgent"}]}));
    json!({"@context": {"prov": crate::prov::CONTEXT, "xsd": "http://www.w3.org/2001/XMLSchema#"},
        "schema": "atlas.discovery.candidates", "version": 1, "generated_at": generated_at,
        "sha256": format!("{:x}", Sha256::digest(&bytes)), "sha256_of": "compact JSON candidates array",
        "candidates": candidates, "provenance": provenance})
}

pub fn write(export: &Value, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| ContribError::Internal(e.to_string()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(export).map_err(|e| ContribError::Internal(e.to_string()))?;
    std::fs::write(&tmp, bytes).map_err(|e| ContribError::Internal(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| ContribError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, Decision, ReviewInput};
    use crate::{Contrib, ContribConfig, FetchPolicy, HttpFetcher, NoGraph};
    use std::sync::Arc;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn submission(url: &str) -> Submission {
        serde_json::from_value(json!({
            "kind": "data_source", "contributor": {"name": "Example researcher", "organisation": "Example laboratory", "contact": "review-only@example.invalid"},
            "data_source": {"resource_kind": "dataset", "url": url, "licence": "Submitter claims CC-BY-4.0; not yet verified", "spdx_id": "CC-BY-4.0", "identifier_systems": ["MONDO", "HGNC", "HPO", "NCT", "other".to_ascii_uppercase()], "identifier_systems_other": "Placeholder identifier system", "description": "Placeholder dataset used only for tests.", "consent": true}
        })).unwrap()
    }

    fn service(config: ContribConfig) -> Contrib {
        Contrib::new(config, Arc::new(NoGraph))
            .unwrap()
            .with_fetcher(Arc::new(HttpFetcher::new(FetchPolicy {
                allow_private: true,
                ..Default::default()
            })))
    }

    fn review(decision: Decision) -> ReviewInput {
        ReviewInput {
            decision,
            reason: "Test-only reviewer decision.".into(),
        }
    }
    fn reviewer() -> Agent {
        Agent::person("agent:reviewer/example", Some("Example reviewer".into()))
    }

    #[tokio::test]
    async fn source_workflow_exports_only_accepted_with_provenance_and_no_private_contact() {
        let server = MockServer::start().await;
        Mock::given(path("/data.json"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "application/json")
                .set_body_json(json!({"license": "https://creativecommons.org/licenses/by/4.0/", "ids": ["MONDO:0000001", "HGNC:1", "HP:0000001", "NCT00000001"]})))
            .mount(&server).await;
        let temp = std::env::temp_dir().join(format!("atlas-contrib-{}", crate::util::new_id()));
        let export_path = temp.join("discovery-candidates.json");
        let config = ContribConfig {
            discovery_candidates_path: Some(export_path.clone()),
            ..ContribConfig::for_tests()
        };
        let s = service(config);
        let c = s
            .submit(submission(&format!("{}/data.json", server.uri())), None)
            .unwrap();
        assert_eq!(c.state, State::Submitted);
        assert!(s.review(&c.id, &review(Decision::Accept), reviewer()).is_err());
        assert!(
            s.discovery_candidates().unwrap()["candidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let c = s.run_checks(&c.id).await.unwrap();
        let r = c.checks.as_ref().unwrap();
        assert_eq!(r.find("licence").unwrap().status, CheckStatus::Pass);
        assert_eq!(r.find("formats").unwrap().detail["detected"], json!(["JSON"]));
        assert_eq!(
            r.find("identifiers").unwrap().detail["detected"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(c.public()["contributor"].get("contact"), None);
        let accepted = s.review(&c.id, &review(Decision::Accept), reviewer()).unwrap();
        assert_eq!(accepted.state, State::Accepted);
        assert!(s.run_checks(&c.id).await.is_err());
        assert!(s.review(&c.id, &review(Decision::Reject), reviewer()).is_err());
        let output: Value = serde_json::from_slice(&std::fs::read(&export_path).unwrap()).unwrap();
        assert_eq!(output["schema"], "atlas.discovery.candidates");
        assert_eq!(output["candidates"].as_array().unwrap().len(), 1);
        let candidate = &output["candidates"][0];
        assert_eq!(candidate["graph_import_allowed"], false);
        assert_eq!(candidate["accepted_by"]["id"], reviewer().id);
        assert_eq!(candidate["source"]["version"], "3");
        let public_bytes = serde_json::to_vec(&candidate["submission"]).unwrap();
        assert_eq!(
            candidate["source"]["sha256"],
            format!("{:x}", Sha256::digest(&public_bytes))
        );
        let entries = serde_json::to_vec(&output["candidates"]).unwrap();
        assert_eq!(output["sha256"], format!("{:x}", Sha256::digest(&entries)));
        let serialized = output.to_string();
        assert!(!serialized.contains("review-only@example.invalid"));
        for term in [
            "prov:Person",
            "submission",
            "auto_check",
            "review",
            "prov:qualifiedUsage",
            "prov:qualifiedDerivation",
            "record_locator",
            "sha256",
        ] {
            assert!(serialized.contains(term), "{term}");
        }
        let overlay = s.overlay().unwrap();
        assert!(overlay.nodes.is_empty() && overlay.edges.is_empty() && overlay.annotations.is_empty());
        // Rebuilding is idempotent in membership after a previous export or file-write failure.
        s.write_discovery_candidates().unwrap();
        assert_eq!(
            s.discovery_candidates().unwrap()["candidates"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_file(export_path).unwrap();
        std::fs::remove_dir(temp).unwrap();
    }

    #[tokio::test]
    async fn mock_http_checks_licence_missing_linked_license_and_formats() {
        let server = MockServer::start().await;
        let cases = [
            ("/plain.csv", "text/csv", "id,label\nMONDO:0000001,Example\n", "CSV"),
            (
                "/mappings.tsv",
                "text/tab-separated-values",
                "subject_id\tpredicate_id\tobject_id\nOMIM:1\tskos:exactMatch\tORPHA:1\n",
                "SSSOM",
            ),
            (
                "/graph.ttl",
                "text/turtle",
                "@prefix ex: <https://example.invalid/> .\nex:x ex:p ex:y .",
                "RDF",
            ),
            (
                "/openapi.json",
                "application/json",
                r#"{"openapi":"3.1.0","info":{"title":"Placeholder API"}}"#,
                "OpenAPI",
            ),
        ];
        let s = service(ContribConfig::for_tests());
        for (url, ct, body, format) in cases {
            Mock::given(path(url))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", ct)
                        .set_body_string(body),
                )
                .mount(&server)
                .await;
            let c = s.submit(submission(&format!("{}{url}", server.uri())), None).unwrap();
            let c = s.run_checks(&c.id).await.unwrap();
            let r = c.checks.as_ref().unwrap();
            assert_eq!(r.find("licence").unwrap().status, CheckStatus::Warn);
            assert!(
                r.find("formats").unwrap().detail["detected"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(format)),
                "{format}"
            );
            s.review(&c.id, &review(Decision::Reject), reviewer()).unwrap();
        }
        Mock::given(path("/landing"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string(
                        "<html><a href='/LICENSE'>License</a><a href='/files/data.csv'>Download CSV</a></html>",
                    ),
            )
            .mount(&server)
            .await;
        Mock::given(path("/LICENSE"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/plain")
                    .set_body_string("MIT License\nPermission is hereby granted for this placeholder."),
            )
            .mount(&server)
            .await;
        let c = s
            .submit(submission(&format!("{}/landing", server.uri())), None)
            .unwrap();
        let c = s.run_checks(&c.id).await.unwrap();
        assert_eq!(
            c.checks.as_ref().unwrap().find("licence").unwrap().status,
            CheckStatus::Pass
        );
        assert_eq!(c.checks.as_ref().unwrap().fetched.len(), 2);
        assert_eq!(
            c.checks.as_ref().unwrap().find("formats").unwrap().detail["advertised"][0]["format"],
            "CSV"
        );
        assert_eq!(
            c.checks.as_ref().unwrap().find("formats").unwrap().detail["advertised"][0]["download_verified"],
            false
        );
        assert!(
            c.checks
                .as_ref()
                .unwrap()
                .find("licence")
                .unwrap()
                .detail
                .to_string()
                .contains("/LICENSE")
        );
    }

    #[test]
    fn schema_org_datacite_spdx_and_samples_are_bounded() {
        for body in [
            r#"<script type="application/ld+json">{"@context":"https://schema.org","license":"CC0-1.0"}</script>"#,
            r#"{"data":{"attributes":{"rightsList":[{"rights":"CC BY 4.0","rightsUri":"https://creativecommons.org/licenses/by/4.0/"}]}}}"#,
            "SPDX-License-Identifier: MIT",
        ] {
            assert!(!licence_evidence(body).is_empty());
        }
        let body = format!("{}MIT License MONDO:123", "x".repeat(SAMPLE_BYTES));
        assert!(licence_evidence(bounded(&body)).is_empty());
        assert!(identifiers(bounded(&body)).is_empty());
        assert_eq!(identifiers("notMONDO:1 MONDO:2 HP:3 NCT12345678 NCT12").len(), 3);
        let mut input = submission("https://example.org/data");
        input.data_source.as_mut().unwrap().consent = false;
        assert!(input.clean().is_ok());
        let mut input = submission("https://example.org/data");
        input.contributor.contact = Some("bad email".into());
        assert!(input.clean().is_err());
        assert!(submission("file:///etc/passwd").clean().is_err());
        assert!(licence_evidence(r#"{"license":false,"rights":[]}"#).is_empty());
        let mut input = submission("https://example.org/data");
        input.contributor.name = None;
        input.contributor.organisation = None;
        assert!(input.clean().is_ok());
    }

    #[tokio::test]
    async fn private_source_blocks_acceptance_without_mock_bypass() {
        let s = Contrib::new(ContribConfig::for_tests(), Arc::new(NoGraph))
            .unwrap()
            .with_fetcher(Arc::new(HttpFetcher::default()));
        for url in [
            "http://127.0.0.1/data",
            "http://169.254.169.254/data",
            "http://[::1]/data",
        ] {
            let c = s.submit(submission(url), None).unwrap();
            let c = s.run_checks(&c.id).await.unwrap();
            assert_eq!(
                c.checks.as_ref().unwrap().find("resource_url").unwrap().code,
                "url_blocked"
            );
            assert!(s.review(&c.id, &review(Decision::Accept), reviewer()).is_err());
            s.review(&c.id, &review(Decision::Reject), reviewer()).unwrap();
        }
        assert!(
            s.discovery_candidates().unwrap()["candidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn known_source_url_matching_is_conservative() {
        assert!(same_resource(
            "https://example.org/data?version=2",
            "https://example.org/data"
        ));
        assert!(same_resource("https://example.org/data", "https://example.org/"));
        assert!(!same_resource("https://unrelated.example/data", "https://example.org/"));
        assert!(!same_resource(
            "https://github.com/example/new",
            "https://github.com/example/known"
        ));
        assert_eq!(
            known_sources_from("https://clinicaltrials.gov/study/NCT00000001", None).status,
            CheckStatus::Warn
        );
        assert!(
            !known_sources_from("https://clinicaltrials.gov/study/NCT00000001", None).detail["inputs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn immutable_raw_snapshots_replay_and_optional_cache_flags_known_sources() {
        let server = MockServer::start().await;
        Mock::given(path("/snapshot.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(r#"{"license":"MIT","id":"MONDO:123"}"#),
            )
            .mount(&server)
            .await;
        let page = HttpFetcher::new(FetchPolicy {
            allow_private: true,
            ..Default::default()
        })
        .fetch(&format!("{}/snapshot.json", server.uri()))
        .await;
        let root = std::env::temp_dir().join(format!("atlas-contrib-snapshots-{}", crate::util::new_id()));
        let snapshot = retain_response(&page, Some(&root));
        assert_eq!(snapshot["retention"], "retained");
        let file = root.join(snapshot["snapshot"].as_str().unwrap());
        let raw = std::fs::read(&file).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&raw)),
            page.record.sha256.as_ref().unwrap().as_str()
        );
        assert_eq!(retain_response(&page, Some(&root))["retention"], "retained");
        let kg = root.join("cache/kg-scan");
        std::fs::create_dir(&kg).unwrap();
        let resources = kg.join("resources.json");
        std::fs::write(
            &resources,
            br#"{"schema":"kg_scan.resources","version":1,"records":[{"url":"https://example.invalid/dataset"}]}"#,
        )
        .unwrap();
        let known = known_sources_from("https://example.invalid/dataset", Some(&root));
        assert_eq!(known.status, CheckStatus::Warn);
        assert_eq!(known.detail["optional_cache"], "loaded");
        assert!(known.detail["inputs"].as_array().unwrap().len() >= 2);
        std::fs::write(&resources, b"invalid JSON").unwrap();
        assert!(
            known_sources_from("https://unknown.invalid/dataset", Some(&root)).detail["optional_cache"]
                .as_str()
                .unwrap()
                .contains("incomplete")
        );
        std::fs::remove_file(resources).unwrap();
        std::fs::remove_dir(kg).unwrap();
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(root.join("cache/contrib-datasource/http")).unwrap();
        std::fs::remove_dir(root.join("cache/contrib-datasource")).unwrap();
        std::fs::remove_dir(root.join("cache")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
