//! The graph store: diseases (MONDO-keyed), HPO, genes, identity, provenance and the derived indexes.
//!
//! Derived state (closures, IC, indexes, search) is rebuilt by [`Atlas::new`], so ingest and
//! snapshot loading share one construction path.

use std::collections::HashMap;

use serde::Serialize;

use crate::curie;
use crate::disease::{Disease, DiseaseIdx};
use crate::identity::DiseaseIdentity;
use crate::node::{NodeKey, NodeKind, NodeRef};
use crate::ontology::{Ontology, TermIdx};
use crate::provenance::Provenance;
use crate::search::SearchIndex;
use crate::term::Term;

/// Dense index into [`Atlas::genes`].
pub type GeneIdx = u32;

/// Gene node, keyed by symbol; links come from the active diseases' [`crate::evidence::GeneLink`]s.
#[derive(Clone, Debug, PartialEq)]
pub struct Gene {
    pub symbol: String,
    pub hgnc: Option<String>,
    pub ncbi_gene: Option<String>,
    /// Active diseases, ascending.
    pub diseases: Vec<DiseaseIdx>,
}

impl Gene {
    /// HGNC id, else NCBIGene id, else the symbol.
    pub fn id(&self) -> &str {
        self.hgnc
            .as_deref()
            .or(self.ncbi_gene.as_deref())
            .unwrap_or(&self.symbol)
    }
}

/// Size of the graph; field order and names match the Python `Atlas.stats()`.
/// Counts cover active nodes; retired ones are reported separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Stats {
    pub diseases: usize,
    pub rare_diseases: usize,
    pub diseases_with_phenotypes: usize,
    pub diseases_with_genes: usize,
    pub merged_from_multiple_sources: usize,
    pub phenotype_edges: usize,
    pub excluded_phenotype_edges: usize,
    pub genes: usize,
    pub hpo_terms: usize,
    pub identity_conflicts: usize,
    pub retired_diseases: usize,
    /// Gene-specific conditions from Gene2Phenotype without an OMIM/Orphanet/MONDO node; not in
    /// the counts above (those match the Python reference).
    #[serde(default)]
    pub newly_described: usize,
}

#[derive(Debug)]
pub struct Atlas {
    pub hpo: Ontology,
    pub identity: DiseaseIdentity,
    pub provenance: Provenance,
    diseases: Vec<Disease>,
    index: HashMap<String, DiseaseIdx>,
    genes: Vec<Gene>,
    /// Symbol, HGNC id and NCBIGene id -> gene.
    gene_index: HashMap<String, GeneIdx>,
    /// Term -> active diseases with a direct (asserted) present annotation, ascending.
    by_phenotype: Vec<Vec<DiseaseIdx>>,
    search: SearchIndex,
}

impl Atlas {
    /// Assemble the store; computes HPO closures, IC over active annotated diseases, indexes, search.
    pub fn new(
        hpo_terms: Vec<Term>,
        identity: DiseaseIdentity,
        provenance: Provenance,
        diseases: Vec<Disease>,
    ) -> Self {
        let mut hpo = Ontology::new(hpo_terms);
        hpo.compute_ic(
            diseases
                .iter()
                .filter(|d| d.is_active() && !d.phenotypes.is_empty())
                .map(|d| d.phenotypes.iter().map(|e| e.term)),
        );
        let index = diseases
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.clone(), i as DiseaseIdx))
            .collect();

        let mut genes: Vec<Gene> = Vec::new();
        let mut by_symbol: HashMap<String, GeneIdx> = HashMap::new();
        let mut by_phenotype = vec![Vec::new(); hpo.len()];
        for (i, d) in diseases.iter().enumerate().filter(|(_, d)| d.is_active()) {
            let i = i as DiseaseIdx;
            for link in &d.genes {
                let g = *by_symbol.entry(link.symbol.clone()).or_insert_with(|| {
                    let gene = Gene {
                        symbol: link.symbol.clone(),
                        hgnc: None,
                        ncbi_gene: None,
                        diseases: Vec::new(),
                    };
                    genes.push(gene);
                    (genes.len() - 1) as GeneIdx
                });
                let gene = &mut genes[g as usize];
                gene.hgnc = gene.hgnc.take().or_else(|| link.hgnc.clone());
                gene.ncbi_gene = gene.ncbi_gene.take().or_else(|| link.ncbi_gene.clone());
                if gene.diseases.last() != Some(&i) {
                    gene.diseases.push(i);
                }
            }
            for e in &d.phenotypes {
                by_phenotype[e.term as usize].push(i);
            }
        }
        let mut gene_index = by_symbol;
        for (g, gene) in genes.iter().enumerate() {
            for id in [&gene.hgnc, &gene.ncbi_gene].into_iter().flatten() {
                gene_index.entry(id.clone()).or_insert(g as GeneIdx);
            }
        }
        let search = SearchIndex::build(&hpo, &diseases, &genes);
        Self {
            hpo,
            identity,
            provenance,
            diseases,
            index,
            genes,
            gene_index,
            by_phenotype,
            search,
        }
    }

    /// All diseases, retired included, in build order.
    pub fn diseases(&self) -> &[Disease] {
        &self.diseases
    }

    /// Active diseases with their indexes.
    pub fn active(&self) -> impl Iterator<Item = (DiseaseIdx, &Disease)> {
        self.diseases
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_active())
            .map(|(i, d)| (i as DiseaseIdx, d))
    }

    pub fn disease_at(&self, idx: DiseaseIdx) -> &Disease {
        &self.diseases[idx as usize]
    }

    /// Node for any id: the node with exactly this id (also retired ones), else its canonical node.
    pub fn disease_idx(&self, id: &str) -> Option<DiseaseIdx> {
        let id = curie::normalize_query(id).unwrap_or_else(|| id.trim().to_owned());
        let normal = curie::normalize(&id);
        self.index
            .get(&normal)
            .or_else(|| self.index.get(&self.identity.resolve(&normal)))
            .copied()
    }

    pub fn disease(&self, id: &str) -> Option<&Disease> {
        self.disease_idx(id).map(|i| self.disease_at(i))
    }

    pub fn genes(&self) -> &[Gene] {
        &self.genes
    }

    pub fn gene_at(&self, idx: GeneIdx) -> &Gene {
        &self.genes[idx as usize]
    }

    /// Gene by symbol, HGNC or NCBIGene id.
    pub fn gene(&self, key: &str) -> Option<GeneIdx> {
        self.gene_index.get(key.trim()).copied()
    }

    /// Active diseases directly annotated with `term` (present).
    pub fn by_phenotype(&self, term: TermIdx) -> &[DiseaseIdx] {
        &self.by_phenotype[term as usize]
    }

    pub fn search(&self) -> &SearchIndex {
        &self.search
    }

    pub fn node_ref(&self, key: NodeKey) -> NodeRef {
        let (id, label) = match key.kind {
            NodeKind::Disease => {
                let d = self.disease_at(key.idx);
                (d.id.clone(), d.name.clone())
            }
            NodeKind::Phenotype => {
                let t = self.hpo.term(key.idx);
                (t.id.clone(), t.name.clone())
            }
            NodeKind::Gene => {
                let g = self.gene_at(key.idx);
                (g.id().to_owned(), g.symbol.clone())
            }
            other => unreachable!("no {other:?} table in atlas-core yet"),
        };
        NodeRef {
            id,
            kind: key.kind,
            label,
        }
    }

    pub fn disease_ref(&self, idx: DiseaseIdx) -> NodeRef {
        self.node_ref(NodeKey {
            kind: NodeKind::Disease,
            idx,
        })
    }

    pub fn term_ref(&self, term: TermIdx) -> NodeRef {
        self.node_ref(NodeKey {
            kind: NodeKind::Phenotype,
            idx: term,
        })
    }

    pub fn stats(&self) -> Stats {
        let all_active = self.diseases.iter().filter(|d| d.is_active());
        let (new, active): (Vec<&Disease>, Vec<&Disease>) = all_active.partition(|d| d.is_newly_described());
        let count = |f: &dyn Fn(&Disease) -> bool| active.iter().filter(|d| f(d)).count();
        let genes = self
            .genes
            .iter()
            .filter(|g| g.diseases.iter().any(|&d| !self.disease_at(d).is_newly_described()))
            .count();
        Stats {
            diseases: active.len(),
            rare_diseases: count(&|d| d.rare),
            diseases_with_phenotypes: count(&|d| !d.phenotypes.is_empty()),
            diseases_with_genes: count(&|d| !d.genes.is_empty()),
            merged_from_multiple_sources: count(&|d| d.source_ids.len() > 1),
            phenotype_edges: active.iter().map(|d| d.phenotypes.len()).sum(),
            excluded_phenotype_edges: active.iter().map(|d| d.excluded.len()).sum(),
            genes,
            hpo_terms: self.hpo.len(),
            identity_conflicts: self.identity.conflicts().len(),
            retired_diseases: self.diseases.len() - active.len() - new.len(),
            newly_described: new.len(),
        }
    }

    /// Parts that a snapshot stores; everything else is derived.
    pub fn parts(&self) -> (&[Term], &DiseaseIdentity, &Provenance, &[Disease]) {
        (self.hpo.terms(), &self.identity, &self.provenance, &self.diseases)
    }
}
