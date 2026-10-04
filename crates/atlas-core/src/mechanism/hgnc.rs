//! HGNC approved genes with aliases, previous symbols and NCBI Gene ids. Parsed in
//! `atlas_ingest::mechanism::hgnc`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::provenance::RecordRef;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HgncGene {
    pub hgnc_id: String,
    pub symbol: String,
    pub name: String,
    pub locus_group: String,
    pub entrez: Option<String>,
    pub groups: Vec<String>,
    pub aliases: Vec<String>,
    pub previous: Vec<String>,
    pub record: RecordRef,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Hgnc {
    pub genes: Vec<HgncGene>,
    /// Records skipped because their status is not `Approved`.
    pub skipped_not_approved: u64,
    #[serde(skip)]
    by_symbol: HashMap<String, u32>,
    /// Previous symbol -> genes (several genes may share one).
    #[serde(skip)]
    by_previous: HashMap<String, Vec<u32>>,
}

impl Hgnc {
    pub fn finish(&mut self) {
        self.by_symbol.clear();
        self.by_previous.clear();
        for (i, g) in self.genes.iter().enumerate() {
            self.by_symbol.insert(g.symbol.clone(), i as u32);
            for p in &g.previous {
                self.by_previous.entry(p.clone()).or_default().push(i as u32);
            }
        }
    }

    /// Approved symbol only (the prototype's `genes.get(symbol)`).
    pub fn get(&self, symbol: &str) -> Option<&HgncGene> {
        self.by_symbol.get(symbol).map(|&i| &self.genes[i as usize])
    }

    /// Approved symbol, else a previous symbol that names exactly one approved gene.
    pub fn resolve(&self, symbol: &str) -> Option<&HgncGene> {
        self.get(symbol)
            .or_else(|| match self.by_previous.get(symbol).map(Vec::as_slice) {
                Some([i]) => Some(&self.genes[*i as usize]),
                _ => None,
            })
    }
}
