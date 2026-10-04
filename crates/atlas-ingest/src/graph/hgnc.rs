//! HGNC aliases and previous symbols of atlas genes, for search (`Munc18-1` → STXBP1).
//! Reads `raw/hgnc_complete_set.txt`; every alias entry points at its TSV line.

use std::io::{BufRead, BufReader};
use std::path::Path;

use atlas_core::graph::{Coverage, GeneAlias, RecordHash, SourceRecord, activity};
use atlas_core::provenance::{Locator, SourceEntity};

use super::builder::Builder;
use super::cache;
use crate::error::IngestError;
use crate::sources;

pub const FILE: &str = "hgnc_complete_set.txt";

fn list(field: &str) -> Vec<String> {
    field
        .trim_matches('"')
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn ingest(b: &mut Builder<'_>, raw: &Path) -> Result<(), IngestError> {
    let path = raw.join(FILE);
    let mut coverage = Coverage {
        source: "hgnc".into(),
        label: "HGNC gene symbols and aliases".into(),
        status: "absent".into(),
        files: vec![format!("raw/{FILE}")],
        scope: "aliases and previous symbols of atlas genes".into(),
        ..Coverage::default()
    };
    if !path.exists() {
        b.data.coverage.push(coverage);
        return Ok(());
    }
    let meta = std::fs::metadata(&path).map_err(IngestError::io(&path))?;
    let entity = b.entity(SourceEntity {
        id: format!("source:{FILE}"),
        url: "https://storage.googleapis.com/public-download-files/hgnc/tsv/tsv/hgnc_complete_set.txt".into(),
        file: format!("raw/{FILE}"),
        version: None,
        retrieved_at: meta.modified().ok().map(sources::rfc3339),
        sha256: Some(sources::sha256(&path)?),
        bytes: meta.len(),
        licence: Some("HGNC (CC0)".into()),
    });
    let act = b.start(
        activity::INGEST_HGNC,
        "HGNC aliases and previous symbols of atlas genes",
        &[entity],
    );
    let file = std::fs::File::open(&path).map_err(IngestError::io(&path))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).split(b'\n');
    let header = match lines.next() {
        Some(h) => String::from_utf8_lossy(&h.map_err(IngestError::io(&path))?)
            .trim_end()
            .to_owned(),
        None => String::new(),
    };
    let col = |name: &str| header.split('\t').position(|c| c == name);
    let (Some(id_c), Some(sym_c), Some(name_c), Some(alias_c), Some(prev_c)) = (
        col("hgnc_id"),
        col("symbol"),
        col("name"),
        col("alias_symbol"),
        col("prev_symbol"),
    ) else {
        coverage.status = "rejected: missing columns".into();
        b.data.coverage.push(coverage);
        return Ok(());
    };
    let (mut read, mut kept) = (0usize, 0usize);
    for (i, line) in lines.enumerate() {
        let mut bytes = line.map_err(IngestError::io(&path))?;
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        read += 1;
        let text = String::from_utf8_lossy(&bytes);
        let f: Vec<&str> = text.split('\t').collect();
        let get = |c: usize| f.get(c).copied().unwrap_or("");
        let (hgnc, symbol) = (get(id_c), get(sym_c));
        let known = b.atlas.gene(symbol).or_else(|| b.atlas.gene(hgnc)).is_some();
        if !known {
            continue;
        }
        kept += 1;
        let rec = b.record(SourceRecord {
            entity,
            locator: Locator::Line(i as u32 + 2),
            id: hgnc.to_owned(),
            url: Some(format!(
                "https://www.genenames.org/data/gene-symbol-report/#!/hgnc_id/{hgnc}"
            )),
            fetched_at: None,
            hash: RecordHash::TsvLine,
            sha256: cache::sha256(&bytes),
        });
        b.data.gene_aliases.push(GeneAlias {
            hgnc: hgnc.to_owned(),
            symbol: symbol.to_owned(),
            name: get(name_c).trim_matches('"').to_owned(),
            aliases: list(get(alias_c)),
            previous: list(get(prev_c)),
            record: rec,
        });
    }
    b.finish(act, &[("rows", read), ("kept:atlas-genes", kept)]);
    coverage.status = "loaded".into();
    coverage.records = kept as u64;
    coverage.retrieved_at = meta.modified().ok().map(sources::rfc3339);
    b.data.coverage.push(coverage);
    Ok(())
}
