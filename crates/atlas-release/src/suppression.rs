//! Count-only adapter; the shared Withhold owns all suppression semantics.
use crate::digest;
use anyhow::{Context, Result};
use atlas_core::withhold::{KeyKind, SUPPRESSION_FILE, Withhold};
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Default, Debug, Serialize)]
pub struct Summary {
    pub present: bool,
    pub manifest_sha256: Option<String>,
    pub input_entries: usize,
    pub excluded_nodes: usize,
    pub excluded_edges: usize,
}

impl Summary {
    pub fn load(data: &Path) -> Result<Self> {
        let filter = atlas_ingest::withhold::load(data, atlas_ingest::withhold::salt_from_env())?;
        let bytes = match fs::read(data.join(SUPPRESSION_FILE)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context("reading suppression manifest"),
        };
        Ok(Self {
            present: true,
            manifest_sha256: Some(digest(&bytes)),
            input_entries: filter.suppression_entries().len(),
            ..Default::default()
        })
    }
}

pub(crate) fn blocked(filter: &Withhold, id: &str) -> bool {
    filter
        .salt()
        .key(KeyKind::Node, id)
        .is_some_and(|key| filter.keys(&[key]).is_some())
}
