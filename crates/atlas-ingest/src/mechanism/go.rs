//! GO biological process: `go-basic.obo` (is_a + part_of within BP) and human GAF annotations.
//!
//! Annotation rule (documented in docs/research/mechanism-validation.md):
//! - aspect P, taxon 9606, qualifier without `NOT`;
//! - evidence code not `IEA` (electronic, uncurated) and not `ND` (no data: root-only placeholder);
//!   phylogenetic `IBA` and author/curator statements (`TAS`, `NAS`, `IC`) stay;
//! - gene = HGNC approved symbol from GAF column 3, else a unique HGNC previous symbol;
//! - GO id through `alt_id`; obsolete terms are skipped (counted).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use flate2::read::MultiGzDecoder;

use crate::error::IngestError;
use crate::mechanism::hgnc::Hgnc;
use crate::mechanism::process::ProcessOntology;

pub const BIOLOGICAL_PROCESS: &str = "GO:0008150";
pub const EXCLUDED_EVIDENCE: [&str; 2] = ["IEA", "ND"];

#[derive(Debug, Default)]
pub struct GoCounts {
    pub bp_terms: u64,
    pub obsolete_terms: u64,
    pub other_namespace_terms: u64,
    pub annotations_read: u64,
    pub kept: u64,
    pub skipped_other_aspect: u64,
    pub skipped_not: u64,
    pub skipped_evidence: u64,
    pub skipped_taxon: u64,
    pub skipped_unknown_gene: u64,
    pub skipped_obsolete_or_unknown_term: u64,
    pub data_version: Option<String>,
    pub gaf_date: Option<String>,
}

#[derive(Default)]
struct Stanza {
    id: String,
    name: String,
    namespace: String,
    parents: Vec<String>,
    regulates: Vec<String>,
    alt_ids: Vec<String>,
    subsets: Vec<String>,
    obsolete: bool,
}

/// BP terms only; `alt` maps alternative ids to primary ids.
pub fn read_obo(path: &Path, counts: &mut GoCounts) -> Result<(ProcessOntology, HashMap<String, String>), IngestError> {
    let f = File::open(path).map_err(IngestError::io(path))?;
    let mut stanzas: Vec<Stanza> = Vec::new();
    let mut cur: Option<Stanza> = None;
    let mut in_term = false;
    for line in BufReader::with_capacity(1 << 20, f).lines() {
        let line = line.map_err(IngestError::io(path))?;
        if line.starts_with('[') {
            stanzas.extend(cur.take());
            in_term = line == "[Term]";
            if in_term {
                cur = Some(Stanza::default());
            }
            continue;
        }
        if !in_term {
            if let Some(v) = line.strip_prefix("data-version: ") {
                counts.data_version = Some(v.to_owned());
            }
            continue;
        }
        let Some(s) = cur.as_mut() else { continue };
        let Some((tag, value)) = line.split_once(": ") else {
            continue;
        };
        let first = value.split(' ').next().unwrap_or(value);
        match tag {
            "id" => s.id = value.to_owned(),
            "name" => s.name = value.to_owned(),
            "namespace" => s.namespace = value.to_owned(),
            "is_a" => s.parents.push(first.to_owned()),
            "relationship" => {
                let mut it = value.split(' ');
                match (it.next(), it.next()) {
                    (Some("part_of"), Some(target)) => s.parents.push(target.to_owned()),
                    (Some("regulates" | "positively_regulates" | "negatively_regulates"), Some(target)) => {
                        s.regulates.push(target.to_owned())
                    }
                    _ => {}
                }
            }
            "alt_id" => s.alt_ids.push(value.to_owned()),
            "subset" => s.subsets.push(first.to_owned()),
            "is_obsolete" => s.obsolete = value == "true",
            _ => {}
        }
    }
    stanzas.extend(cur.take());

    let mut o = ProcessOntology::default();
    let mut alt = HashMap::new();
    let bp: Vec<&Stanza> = stanzas
        .iter()
        .filter(|s| {
            if s.obsolete {
                counts.obsolete_terms += 1;
                false
            } else if s.namespace != "biological_process" {
                counts.other_namespace_terms += 1;
                false
            } else {
                true
            }
        })
        .collect();
    for s in &bp {
        let i = o.intern(&s.id);
        o.names[i as usize] = s.name.clone();
        for a in &s.alt_ids {
            alt.insert(a.clone(), s.id.clone());
        }
        for sub in &s.subsets {
            o.subsets.entry(sub.clone()).or_default().push(i);
        }
    }
    counts.bp_terms = bp.len() as u64;
    o.regulates = vec![Vec::new(); o.len()];
    for s in &bp {
        let i = o.get(&s.id).expect("interned");
        // part_of targets outside BP are not interned: the closure stays within BP
        let parents: Vec<u32> = s.parents.iter().filter_map(|p| o.get(p)).collect();
        o.parents[i as usize] = parents;
        o.regulates[i as usize] = s.regulates.iter().filter_map(|p| o.get(p)).collect();
    }
    Ok((o, alt))
}

/// Add GAF annotations (gene key = HGNC approved symbol), then compute closures and IC.
pub fn read_gaf(
    path: &Path,
    o: &mut ProcessOntology,
    alt: &HashMap<String, String>,
    hgnc: &Hgnc,
    counts: &mut GoCounts,
) -> Result<(), IngestError> {
    let f = File::open(path).map_err(IngestError::io(path))?;
    let reader = BufReader::with_capacity(1 << 20, MultiGzDecoder::new(f));
    for line in reader.lines() {
        let line = line.map_err(IngestError::io(path))?;
        if let Some(h) = line.strip_prefix('!') {
            if counts.gaf_date.is_none()
                && let Some(d) = h.strip_prefix("date-generated: ")
            {
                counts.gaf_date = Some(d.trim().to_owned());
            }
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 13 {
            continue;
        }
        counts.annotations_read += 1;
        if cols[8] != "P" {
            counts.skipped_other_aspect += 1;
            continue;
        }
        if cols[3].split('|').any(|q| q == "NOT") {
            counts.skipped_not += 1;
            continue;
        }
        if EXCLUDED_EVIDENCE.contains(&cols[6]) {
            counts.skipped_evidence += 1;
            continue;
        }
        if !cols[12].split('|').any(|t| t == "taxon:9606") {
            counts.skipped_taxon += 1;
            continue;
        }
        let Some(gene) = hgnc.resolve(cols[2]) else {
            counts.skipped_unknown_gene += 1;
            continue;
        };
        let term = alt.get(cols[4]).map_or(cols[4], String::as_str);
        let Some(p) = o.get(term) else {
            counts.skipped_obsolete_or_unknown_term += 1;
            continue;
        };
        let entry = o.by_gene.entry(gene.symbol.clone()).or_default();
        if !entry.contains(&p) {
            entry.push(p);
        }
        counts.kept += 1;
    }
    o.finish();
    o.compute_ic();
    Ok(())
}
