//! Reactome human pathways: names, hierarchy and lowest-level gene membership (NCBI Gene ids).
//! Port of `read_reactome` in `mechanism.py`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::error::IngestError;
use crate::mechanism::process::ProcessOntology;

pub const HUMAN: &str = "Homo sapiens";

/// Counts for the ingest activity.
#[derive(Debug, Default)]
pub struct ReactomeCounts {
    pub pathways: u64,
    pub other_species: u64,
    pub relations: u64,
    pub memberships: u64,
    pub memberships_other_species: u64,
}

fn lines(path: &Path) -> Result<impl Iterator<Item = std::io::Result<String>>, IngestError> {
    let f = File::open(path).map_err(IngestError::io(path))?;
    Ok(BufReader::with_capacity(1 << 20, f).lines())
}

/// Read `ReactomePathways.txt`, `ReactomePathwaysRelation.txt` and `NCBI2Reactome.txt`.
pub fn read(pathways: &Path, relation: &Path, genes: &Path) -> Result<(ProcessOntology, ReactomeCounts), IngestError> {
    let mut o = ProcessOntology::default();
    let mut n = ReactomeCounts::default();
    for line in lines(pathways)? {
        let line = line.map_err(IngestError::io(pathways))?;
        let mut cols = line.split('\t');
        let (Some(id), Some(name), Some(species)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        if species == HUMAN {
            let p = o.intern(id);
            o.names[p as usize] = name.to_owned();
            n.pathways += 1;
        } else {
            n.other_species += 1;
        }
    }
    for line in lines(relation)? {
        let line = line.map_err(IngestError::io(relation))?;
        let mut cols = line.split_whitespace();
        let (Some(parent), Some(child)) = (cols.next(), cols.next()) else {
            continue;
        };
        if let Some(c) = o.get(child) {
            let p = o.intern(parent);
            o.parents[c as usize].push(p);
            n.relations += 1;
        }
    }
    for line in lines(genes)? {
        let line = line.map_err(IngestError::io(genes))?;
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 6 {
            continue;
        }
        if cols[5] != HUMAN {
            n.memberships_other_species += 1;
            continue;
        }
        let p = o.intern(cols[1]);
        let entry = o.by_gene.entry(cols[0].to_owned()).or_default();
        if !entry.contains(&p) {
            entry.push(p);
        }
        n.memberships += 1;
    }
    o.finish();
    o.compute_ic();
    Ok((o, n))
}
