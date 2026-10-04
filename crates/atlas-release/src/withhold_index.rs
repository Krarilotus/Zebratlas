//! Private normalized index for Python release gates, parsed by the shared filter.
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

pub fn index(data: &Path) -> Result<serde_json::Value> {
    let filter = atlas_ingest::withhold::load(data, atlas_ingest::withhold::salt_from_env())?;
    let quarantine = crate::quarantine::Quarantine::load(data)?;
    let suppression = crate::suppression::Summary::load(data)?;
    let projection = atlas_ingest::identity_projection::load(data)?;
    let representatives: std::collections::BTreeMap<_, _> = projection
        .pairs
        .values()
        .flat_map(|(a, b)| [a, b])
        .map(|id| (id, projection.accepted.representative(id)))
        .collect();
    let decisions: std::collections::BTreeMap<_, _> = projection
        .decisions
        .iter()
        .map(|(decision, assertion)| {
            let (subject, _) = &projection.pairs[decision];
            (
                decision,
                serde_json::json!({"assertion": assertion,
                "representative": projection.accepted.representative(subject)}),
            )
        })
        .collect();
    let mut urls = BTreeSet::new();
    if quarantine.summary.present {
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(data.join(atlas_core::withhold::QUARANTINE_FILE))?)?;
        for row in manifest
            .get("entries")
            .or_else(|| manifest.get("records"))
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(url) = row.get("source_url").and_then(|v| v.as_str())
                && filter.source_url(url).is_some()
            {
                urls.insert(url.split(['?', '#']).next().unwrap().to_owned());
            }
        }
    }
    Ok(serde_json::json!({
        "quarantine": quarantine.summary,
        "suppression": suppression,
        "quarantine_records": filter.quarantine_records().collect::<Vec<_>>(),
        "quarantine_urls": urls,
        "suppression_keys": filter.suppression_entries().iter().flat_map(|e| e.keys.iter()).collect::<BTreeSet<_>>(),
        "salt_id": filter.salt().id(),
        "identity": {"manifest_sha256": projection.accepted.manifest_sha256,
            "representatives": representatives, "decisions": decisions},
    }))
}
