//! Official study contacts (`contacts.ctgov` v1, SOURCES.md): the central contact a sponsor publishes
//! on ClinicalTrials.gov (the official route, D13), overall officials and per-site status. Per-site
//! personal contacts are not in the cache and never shown.

use std::path::Path;

use atlas_core::graph::{Contact, Coverage, Official, RecordHash, Site, SourceRecord, StudyContacts, activity};
use atlas_core::provenance::{Locator, SourceEntity};
use serde_json::Value;

use super::builder::Builder;
use super::cache;
use crate::error::IngestError;

pub const FILE: &str = "cache/contacts/ctgov.json";

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_owned()
}

fn opt(v: &Value, k: &str) -> Option<String> {
    Some(s(v, k)).filter(|x| !x.is_empty())
}

fn items<'v>(v: &'v Value, k: &str) -> impl Iterator<Item = &'v Value> {
    v.get(k).and_then(Value::as_array).into_iter().flatten()
}

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let path = data.join(FILE);
    let mut coverage = Coverage {
        source: "contacts".into(),
        label: "ClinicalTrials.gov study contacts".into(),
        status: "absent".into(),
        files: vec![FILE.into()],
        scope: "central contacts, officials and sites of studies linked to the slice genes".into(),
        ..Coverage::default()
    };
    if !path.exists() {
        b.data.coverage.push(coverage);
        return Ok(());
    }
    let env = cache::read_envelope(&path, "contacts.ctgov", &[1])?;
    let entity = b.entity(SourceEntity {
        id: format!("source:{FILE}"),
        url: env
            .header
            .get("query")
            .and_then(|q| q.get("api"))
            .and_then(Value::as_str)
            .unwrap_or("https://clinicaltrials.gov/api/v2/studies")
            .into(),
        file: FILE.into(),
        version: Some(format!("{} v{}", env.schema, env.version)),
        retrieved_at: env.header_str("retrieved_at").map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        licence: Some("ClinicalTrials.gov terms of use (public registry fields)".into()),
    });
    let act = b.start(
        activity::INGEST_CONTACTS,
        "Official study contacts from ClinicalTrials.gov",
        &[entity],
    );
    let (mut n, mut unknown) = (0usize, 0usize);
    for (i, r) in env.records.iter().enumerate() {
        let nct = s(r, "id");
        if b.node(&nct).is_none() {
            unknown += 1;
        }
        let rec = b.record(SourceRecord {
            entity,
            locator: Locator::Record(format!("records[{i}]")),
            id: nct.clone(),
            url: opt(r, "url"),
            fetched_at: opt(r, "fetched_at"),
            hash: RecordHash::CanonicalJson,
            sha256: cache::canonical_sha256(r),
        });
        b.data.contacts.push(StudyContacts {
            study: nct,
            central: items(r, "central_contacts")
                .map(|c| Contact {
                    name: s(c, "name"),
                    role: s(c, "role"),
                    phone: opt(c, "phone").map(|p| match opt(c, "phoneExt") {
                        Some(ext) => format!("{p} ext. {ext}"),
                        None => p,
                    }),
                    email: opt(c, "email"),
                })
                .collect(),
            officials: items(r, "overall_officials")
                .map(|o| Official {
                    name: s(o, "name"),
                    affiliation: s(o, "affiliation"),
                    role: s(o, "role"),
                })
                .collect(),
            sites: items(r, "locations")
                .map(|l| Site {
                    facility: s(l, "facility"),
                    city: s(l, "city"),
                    country: s(l, "country"),
                    status: opt(l, "status"),
                })
                .collect(),
            last_update: s(r, "last_update"),
            record: rec,
        });
        n += 1;
    }
    b.finish(act, &[("studies", n), ("studies-not-in-graph", unknown)]);
    coverage.status = "loaded".into();
    coverage.records = n as u64;
    coverage.retrieved_at = env.header_str("retrieved_at").map(str::to_owned);
    coverage.header_checksums_verified = u64::from(env.header_verified);
    coverage.header_checksums_failed = u64::from(!env.header_verified);
    b.data.coverage.push(coverage);
    Ok(())
}
