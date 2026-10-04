//! Condition–gene edges: one edge per gene symbol with every source link behind it, the gene node
//! id it points to, its edge id (`/api/provenance/{id}`), and the causal-gene rule.

use atlas_core::evidence::GeneLink;
use atlas_core::node::edge_id;
use atlas_core::{Atlas, DiseaseIdx, TermIdx};

pub const GENE_RELATION: &str = "has_associated_gene";
pub const PHENOTYPE_RELATION: &str = "has_phenotype";
pub const ABSENT_PHENOTYPE_RELATION: &str = "lacks_phenotype";

/// A condition with more causal genes is a clinical umbrella, not one gene's community (trials.py).
pub const MAX_CONDITION_GENES: usize = 3;

/// Relation of a phenotype edge: present, or explicitly absent (`NOT`).
pub fn phenotype_relation(absent: bool) -> &'static str {
    if absent {
        ABSENT_PHENOTYPE_RELATION
    } else {
        PHENOTYPE_RELATION
    }
}

/// Gene links count as exact for a condition with at least one and at most
/// [`MAX_CONDITION_GENES`] causal genes.
pub fn gene_exact(causal_genes: usize) -> bool {
    causal_genes > 0 && causal_genes <= MAX_CONDITION_GENES
}

/// Gene node id of a link: the gene table entry's id, else HGNC, else NCBI Gene, else the symbol.
pub fn gene_id(atlas: &Atlas, link: &GeneLink) -> String {
    match atlas.gene(&link.symbol) {
        Some(g) => atlas.gene_at(g).id().to_owned(),
        None => link
            .hgnc
            .clone()
            .or_else(|| link.ncbi_gene.clone())
            .unwrap_or_else(|| link.symbol.clone()),
    }
}

pub fn gene_edge_id(disease_id: &str, gene_id: &str) -> String {
    edge_id(disease_id, GENE_RELATION, gene_id)
}

/// Edge id of a present phenotype of condition `d`.
pub fn phenotype_edge_id(atlas: &Atlas, d: DiseaseIdx, term: TermIdx) -> String {
    edge_id(&atlas.disease_at(d).id, PHENOTYPE_RELATION, &atlas.hpo.term(term).id)
}

/// One condition–gene edge: all source links for one symbol.
#[derive(Clone, Debug)]
pub struct GeneEdge<'a> {
    pub symbol: &'a str,
    pub gene_id: String,
    pub edge_id: String,
    /// The source links, in source order (never empty).
    pub links: Vec<&'a GeneLink>,
}

impl GeneEdge<'_> {
    /// The causal-gene rule: some source link says the gene causes the condition.
    pub fn is_causal(&self) -> bool {
        self.links.iter().any(|l| l.is_causal())
    }
}

/// Gene edges of `links` (a condition's gene links), one per symbol in first-seen order.
pub fn gene_edges<'a>(atlas: &Atlas, disease_id: &str, links: &'a [GeneLink]) -> Vec<GeneEdge<'a>> {
    let mut out: Vec<GeneEdge<'a>> = Vec::new();
    for link in links {
        if let Some(e) = out.iter_mut().find(|e| e.symbol == link.symbol) {
            e.links.push(link);
            continue;
        }
        let gene_id = gene_id(atlas, link);
        out.push(GeneEdge {
            symbol: &link.symbol,
            edge_id: gene_edge_id(disease_id, &gene_id),
            gene_id,
            links: vec![link],
        });
    }
    out
}

/// Gene edges of condition `d`.
pub fn condition_gene_edges(atlas: &Atlas, d: DiseaseIdx) -> Vec<GeneEdge<'_>> {
    let disease = atlas.disease_at(d);
    gene_edges(atlas, &disease.id, &disease.genes)
}

/// Causal gene edges of condition `d`.
pub fn causal_gene_edges(atlas: &Atlas, d: DiseaseIdx) -> Vec<GeneEdge<'_>> {
    condition_gene_edges(atlas, d)
        .into_iter()
        .filter(GeneEdge::is_causal)
        .collect()
}
