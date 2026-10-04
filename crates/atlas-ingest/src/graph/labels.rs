//! Multilingual names from Wikidata (`labels.wikidata` v1, CC0) as **search candidates only**
//! (SOURCES.md): a label or alias leads to the Wikidata item, its `xrefs` lead to atlas conditions
//! and genes. Nothing is merged; the SSSOM closeMatch rows are not read.

use std::path::Path;

use atlas_core::graph::{Coverage, RecordHash, SourceRecord, WikiItem, WikiName, activity};
use atlas_core::provenance::{Locator, SourceEntity};
use serde_json::Value;

use super::builder::Builder;
use super::cache;
use crate::error::IngestError;

pub const FILE: &str = "cache/labels/wikidata.json";

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let path = data.join(FILE);
    let mut coverage = Coverage {
        source: "wikidata".into(),
        label: "Wikidata multilingual names".into(),
        status: "absent".into(),
        files: vec![FILE.into()],
        scope: "labels and aliases of items with a MONDO, Orphanet, OMIM or gene-symbol id (search only)".into(),
        ..Coverage::default()
    };
    if !path.exists() {
        b.data.coverage.push(coverage);
        return Ok(());
    }
    let env = cache::read_envelope(&path, "labels.wikidata", &[1])?;
    let entity = b.entity(SourceEntity {
        id: format!("source:{FILE}"),
        url: "https://www.wikidata.org/w/api.php?action=wbgetentities".into(),
        file: FILE.into(),
        version: Some(format!("{} v{}", env.schema, env.version)),
        retrieved_at: env.header_str("retrieved_at").map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        licence: Some("CC0 (Wikidata)".into()),
    });
    let act = b.start(
        activity::INGEST_LABELS,
        "Wikidata labels and aliases as search candidates (never merged)",
        &[entity],
    );
    let (mut items, mut names, mut unresolved) = (0usize, 0usize, 0usize);
    for (i, r) in env.records.iter().enumerate() {
        let mut targets: Vec<String> = Vec::new();
        if let Some(x) = r.get("xrefs").and_then(Value::as_object) {
            for (kind, ids) in x {
                for id in ids.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    let target = if kind == "HGNC.SYMBOL" {
                        let symbol = id.rsplit(':').next().unwrap_or(id);
                        b.gene_id(symbol)
                    } else {
                        b.atlas
                            .disease_idx(id)
                            .map(|d| b.atlas.disease_at(d))
                            .filter(|d| d.is_active())
                            .map(|d| d.id.clone())
                    };
                    if let Some(t) = target
                        && !targets.contains(&t)
                    {
                        targets.push(t);
                    }
                }
            }
        }
        if targets.is_empty() {
            unresolved += 1;
            continue;
        }
        let rec = b.record(SourceRecord {
            entity,
            locator: Locator::Record(format!("records[{i}]")),
            id: r.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
            url: r
                .get("revision_url")
                .or_else(|| r.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            fetched_at: r.get("fetched_at").and_then(Value::as_str).map(str::to_owned),
            hash: RecordHash::CanonicalJson,
            sha256: cache::canonical_sha256(r),
        });
        let item = b.data.wiki_items.len() as u32;
        b.data.wiki_items.push(WikiItem {
            qid: r.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
            targets,
            record: rec,
        });
        items += 1;
        let mut add = |lang: &str, text: &str, alias: bool| {
            if !text.trim().is_empty() {
                b.data.wiki_names.push(WikiName {
                    text: text.into(),
                    lang: lang.into(),
                    alias,
                    item,
                });
                names += 1;
            }
        };
        for (lang, label) in r.get("labels").and_then(Value::as_object).into_iter().flatten() {
            if let Some(t) = label.as_str() {
                add(lang, t, false);
            }
        }
        for (lang, list) in r.get("aliases").and_then(Value::as_object).into_iter().flatten() {
            for t in list.as_array().into_iter().flatten().filter_map(Value::as_str) {
                add(lang, t, true);
            }
        }
    }
    b.finish(
        act,
        &[
            ("items", items),
            ("names", names),
            ("skipped:no-atlas-target", unresolved),
        ],
    );
    coverage.status = "loaded".into();
    coverage.records = items as u64;
    coverage.retrieved_at = env.header_str("retrieved_at").map(str::to_owned);
    coverage.header_checksums_verified = u64::from(env.header_verified);
    coverage.header_checksums_failed = u64::from(!env.header_verified);
    b.data.coverage.push(coverage);
    Ok(())
}
