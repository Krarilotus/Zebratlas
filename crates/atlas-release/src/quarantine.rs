//! Count-only quarantine reporting adapter over the shared withholding mechanism.
use crate::{Citation, digest};
use anyhow::{Context, Result};
use atlas_core::provenance::{Locator, Provenance};
use atlas_core::withhold::{QUARANTINE_FILE, Withhold};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Default, Debug, Serialize)]
pub struct QuarantineSummary {
    pub present: bool,
    pub manifest_sha256: Option<String>,
    pub input_entries: usize,
    pub listed_records: usize,
    pub records_by_cache: BTreeMap<String, usize>,
    pub excluded_nodes: usize,
    pub excluded_edges: usize,
    pub excluded_mapping_rows: usize,
}

#[derive(Default, Debug)]
pub struct Quarantine {
    filter: Option<Withhold>,
    pub summary: QuarantineSummary,
}

impl Quarantine {
    pub fn load(data: &Path) -> Result<Self> {
        let path = data.join(QUARANTINE_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context("reading quarantine manifest"),
        };
        let filter = atlas_ingest::withhold::load(data, atlas_ingest::withhold::salt_from_env())?;
        let mut summary = QuarantineSummary {
            present: true,
            manifest_sha256: Some(digest(&bytes)),
            input_entries: filter.quarantine_len(),
            ..Default::default()
        };
        for (file, _) in filter.quarantine_records() {
            *summary.records_by_cache.entry(file.to_owned()).or_default() += 1;
            summary.listed_records += 1;
        }
        Ok(Self {
            filter: Some(filter),
            summary,
        })
    }

    pub(crate) fn blocks(&self, refs: &[Citation], files: &BTreeMap<String, String>) -> bool {
        let Some(filter) = &self.filter else { return false };
        refs.iter().any(|r| {
            files.get(&r.source_entity).is_some_and(|file| {
                filter
                    .record(file, &Locator::Record(r.record_locator.clone()))
                    .is_some()
            }) || r
                .record_url
                .as_deref()
                .is_some_and(|url| filter.source_url(url).is_some())
                || filter.source_url(&r.source_url).is_some()
        })
    }
}

pub(crate) fn source_files(atlas: &Provenance, graph: &Provenance) -> BTreeMap<String, String> {
    [("atlas", atlas), ("graph", graph)]
        .into_iter()
        .flat_map(|(ns, prov)| {
            prov.entities
                .iter()
                .map(move |e| (format!("{ns}:{}", e.id), e.file.replace('\\', "/")))
        })
        .collect()
}
