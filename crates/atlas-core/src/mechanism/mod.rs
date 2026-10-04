//! Mechanism layer data: HGNC genes, Reactome pathways and GO biological process (one process
//! ontology type), Gene2Phenotype mechanisms and ClinGen dosage sensitivity, with their provenance.
//!
//! Types only: the parsers and the snapshot cache are `atlas_ingest::mechanism`; the computations
//! are atlas-analytics.

pub mod clingen;
pub mod g2p;
pub mod hgnc;
pub mod process;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use clingen::{ClinGenDosage, DosageScore};
pub use g2p::{ESTABLISHED, Effect, G2pRecord};
pub use hgnc::{Hgnc, HgncGene};
pub use process::{ProcessIdx, ProcessKind, ProcessOntology};

use crate::provenance::{EntityIdx, Provenance};

/// Source file names (the `prov:Entity` `file` of each source).
pub const HGNC: &str = "hgnc_complete_set.txt";
pub const REACTOME_PATHWAYS: &str = "ReactomePathways.txt";
pub const REACTOME_RELATION: &str = "ReactomePathwaysRelation.txt";
pub const REACTOME_GENES: &str = "NCBI2Reactome.txt";
pub const G2P: &str = "allG2P_2026-09-28.csv.gz";
pub const CLINGEN: &str = "ClinGen_gene_curation_list_GRCh38.tsv";
pub const GO_OBO: &str = "go-basic.obo";
pub const GO_GAF: &str = "goa_human.gaf.gz";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MechanismData {
    /// Entities: the source files (`atlas_ingest::mechanism::SOURCES`); activities: one per parse step.
    pub provenance: Provenance,
    pub hgnc: Hgnc,
    /// Gene key: NCBI Gene id.
    pub reactome: ProcessOntology,
    /// Gene key: HGNC approved symbol.
    pub go_bp: ProcessOntology,
    pub g2p: Vec<G2pRecord>,
    pub clingen: Vec<ClinGenDosage>,
    #[serde(skip)]
    g2p_by_disease: HashMap<String, Vec<u32>>,
    #[serde(skip)]
    g2p_by_gene: HashMap<String, Vec<u32>>,
    #[serde(skip)]
    clingen_by_symbol: HashMap<String, u32>,
}

impl MechanismData {
    /// The parsed sources; call [`Self::finish`] to build the indexes.
    pub fn new(
        provenance: Provenance,
        hgnc: Hgnc,
        reactome: ProcessOntology,
        go_bp: ProcessOntology,
        g2p: Vec<G2pRecord>,
        clingen: Vec<ClinGenDosage>,
    ) -> Self {
        Self {
            provenance,
            hgnc,
            reactome,
            go_bp,
            g2p,
            clingen,
            ..Self::default()
        }
    }

    /// Rebuild derived indexes (after building or loading).
    pub fn finish(&mut self) {
        self.hgnc.finish();
        self.reactome.finish();
        self.go_bp.finish();
        self.g2p_by_disease.clear();
        self.g2p_by_gene.clear();
        for (i, r) in self.g2p.iter().enumerate() {
            for d in r.conditions() {
                self.g2p_by_disease.entry(d.clone()).or_default().push(i as u32);
            }
            self.g2p_by_gene.entry(r.symbol.clone()).or_default().push(i as u32);
        }
        self.clingen_by_symbol = self
            .clingen
            .iter()
            .enumerate()
            .map(|(i, c)| (c.symbol.clone(), i as u32))
            .collect();
    }

    /// G2P records filed under this condition (its own `G2P:` node, else the resolved MONDO/OMIM id).
    pub fn g2p_for_disease(&self, disease_id: &str) -> impl Iterator<Item = &G2pRecord> {
        self.g2p_by_disease
            .get(disease_id)
            .into_iter()
            .flatten()
            .map(|&i| &self.g2p[i as usize])
    }

    pub fn g2p_for_gene(&self, symbol: &str) -> impl Iterator<Item = &G2pRecord> {
        self.g2p_by_gene
            .get(symbol)
            .into_iter()
            .flatten()
            .map(|&i| &self.g2p[i as usize])
    }

    pub fn clingen(&self, symbol: &str) -> Option<&ClinGenDosage> {
        self.clingen_by_symbol.get(symbol).map(|&i| &self.clingen[i as usize])
    }

    pub fn processes(&self, kind: ProcessKind) -> &ProcessOntology {
        match kind {
            ProcessKind::Reactome => &self.reactome,
            ProcessKind::GoBp => &self.go_bp,
        }
    }

    /// Key under which `kind` stores a gene's annotations (NCBI Gene id / HGNC symbol).
    pub fn gene_key<'a>(&self, kind: ProcessKind, gene: &'a HgncGene) -> Option<&'a str> {
        match kind {
            ProcessKind::Reactome => gene.entrez.as_deref(),
            ProcessKind::GoBp => Some(&gene.symbol),
        }
    }

    /// Process closure of a gene by approved HGNC symbol (empty if unknown).
    pub fn gene_processes(&self, kind: ProcessKind, symbol: &str) -> Vec<ProcessIdx> {
        self.hgnc
            .get(symbol)
            .and_then(|g| self.gene_key(kind, g))
            .map(|k| self.processes(kind).gene_closure(k))
            .unwrap_or_default()
    }

    /// The `prov:Entity` records of the sources, for citing (`file#locator`).
    pub fn entity_by_file(&self, file: &str) -> Option<EntityIdx> {
        self.provenance.entity_by_file(file)
    }
}
