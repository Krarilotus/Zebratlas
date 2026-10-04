//! Inspectable RePORTER API-derived records. A project web page is a JS shell, not evidence.
use atlas_core::graph::RecordWithhold;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::json;
use std::path::Path as FsPath;

use crate::routes::{ApiError, ApiResult, AppState, not_found};

fn unavailable(id: &str) -> ApiError {
    not_found(crate::copy_extra::msg("api.error.item_unknown", json!({"id": id})))
}

fn changed(reason: &str) -> ApiError {
    eprintln!("funding evidence invalid: {reason}");
    ApiError(
        axum::http::StatusCode::CONFLICT,
        crate::copy_extra::msg("api.error.internal", json!({})),
    )
}

pub async fn get(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let key = s
        .graph
        .node(&id)
        .filter(|k| k.kind == atlas_core::node::NodeKind::Grant)
        .ok_or_else(|| unavailable(&id))?;
    if s.graph.node_withheld(key).is_some() {
        return Err(unavailable(&id));
    }
    let grant = s.graph.grant(key.idx);
    for &r in grant.records.iter().rev() {
        let rec = s.graph.record(r);
        let ent = s.graph.provenance().entity(rec.entity);
        if !ent.file.starts_with("cache/reporter/") && !ent.file.starts_with("cache/bench-fixes/reporter/") {
            continue;
        }
        let check = atlas_ingest::graph::verify::record(&s.data, &s.graph, r);
        if !check.matches {
            return Err(changed("source record changed; rebuild and reverify"));
        }
        let env = atlas_ingest::graph::cache::read_envelope(&s.data.join(&ent.file), "reporter.grants", &[1])
            .map_err(crate::routes::internal)?;
        if !env.header_verified {
            return Err(changed("source envelope checksum failed"));
        }
        let row = env
            .records
            .iter()
            .find(|v| v["id"].as_str() == Some(&rec.id))
            .ok_or_else(|| unavailable(&id))?;
        // A saved API response is independently re-hashed and linked by the API's project id.
        // This checks cached evidence; it makes no claim of a fresh upstream request.
        let api_checks = upstream_checks(&s.data, row);
        let api_verified = !api_checks.is_empty() && api_checks.iter().all(|c| c["matches"] == true);
        return Ok(Json(json!({
            "id": id, "title": row["title"], "abstract": row["abstract"], "gene": row["gene"],
            "matched_in": row["matched_in"], "organisation": row["organization"],
            "agency": row["agency"], "fiscal_years": row["fiscal_years"],
            "project_nums": row["project_nums"], "source_url": row["url"],
            "evidence_kind": "api_derived_cached_record", "cache_verification": check,
            "upstream": {"method": "POST", "url": "https://api.reporter.nih.gov/v2/projects/search",
                "body": {"criteria": {"core_project_nums": [rec.id]}, "limit": 500}},
            "api_evidence": row["api_evidence"], "version": ent.version,
            "cached_api_responses_verified": api_verified, "api_response_checks": api_checks,
            "upstream_checked_now": false,
            "prov:wasDerivedFrom": ent.id,
        })));
    }
    Err(unavailable(&id))
}

fn upstream_checks(data: &FsPath, row: &serde_json::Value) -> Vec<serde_json::Value> {
    row["api_evidence"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let digest = e["sha256"].as_str().unwrap_or("");
            let file = e["file"].as_str().unwrap_or("").replace('\\', "/");
            let locator = e["record_locator"].as_str().unwrap_or("");
            // Accept only content-addressed response files in this producer's cache.
            let safe = digest.len() == 64
                && digest.bytes().all(|b| b.is_ascii_hexdigit())
                && file == format!("responses/{digest}.bin")
                && e["url"].as_str() == Some("https://api.reporter.nih.gov/v2/projects/search")
                && e["status"] == 200;
            let bytes = safe
                .then(|| std::fs::read(data.join("cache/bench-fixes").join(&file)).ok())
                .flatten();
            let hash_matches = bytes
                .as_ref()
                .is_some_and(|b| atlas_ingest::graph::cache::hex(&atlas_ingest::graph::cache::sha256(b)) == digest);
            let payload = bytes
                .as_ref()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
            let source_row = payload.as_ref().and_then(|p| p.pointer(locator));
            let identity_matches = source_row.is_some_and(|r| {
                r["appl_id"] == e["appl_id"]
                    && !r["appl_id"].is_null()
                    && r.get("core_project_num").or_else(|| r.get("project_num")) == Some(&row["id"])
            });
            json!({"url": e["url"], "file": file, "sha256": digest, "record_locator": locator,
            "hash_matches": hash_matches, "project_identity_matches": identity_matches,
            "matches": hash_matches && identity_matches})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evidence_errors_keep_status_and_translated_contract() {
        use axum::response::IntoResponse;
        for (error, status, key) in [
            (
                changed("checksum mismatch"),
                axum::http::StatusCode::CONFLICT,
                "api.error.internal",
            ),
            (
                unavailable("REPORTER:missing"),
                axum::http::StatusCode::NOT_FOUND,
                "api.error.item_unknown",
            ),
        ] {
            let response = error.into_response();
            assert_eq!(response.status(), status);
            let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["detail_msg"]["key"], key);
            assert_eq!(value["detail"], value["detail_msg"]["fallback"]);
            assert!(crate::language_contract::offenders(&value).is_empty());
        }
    }

    #[test]
    fn saved_api_response_checks_bytes_and_project_identity() {
        let dir = std::env::temp_dir().join(format!("atlas-reporter-evidence-{}", std::process::id()));
        let bytes = br#"{"results":[{"core_project_num":"R01FIXTURE","appl_id":123}]}"#;
        let digest = atlas_ingest::graph::cache::hex(&atlas_ingest::graph::cache::sha256(bytes));
        let file = format!("responses/{digest}.bin");
        let path = dir.join("cache/bench-fixes").join(&file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let mut row = json!({"id":"R01FIXTURE", "api_evidence":[{"file":file, "sha256":digest,
            "record_locator":"/results/0", "appl_id":123, "status":200,
            "url":"https://api.reporter.nih.gov/v2/projects/search"}]});
        assert_eq!(upstream_checks(&dir, &row)[0]["matches"], true);
        row["id"] = json!("R01OTHER");
        assert_eq!(upstream_checks(&dir, &row)[0]["matches"], false);
        row["id"] = json!("R01FIXTURE");
        row["api_evidence"][0]["file"] = json!("../outside.bin");
        assert_eq!(upstream_checks(&dir, &row)[0]["matches"], false);
        row["api_evidence"][0]["file"] = json!(file);
        std::fs::write(&path, b"changed").unwrap();
        assert_eq!(upstream_checks(&dir, &row)[0]["matches"], false);
        std::fs::remove_file(path).unwrap();
    }
}
