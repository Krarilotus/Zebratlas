//! Journey logic shared by the JSON API (atlas-server) and the conversation (atlas-ask).
//!
//! One owner for: the condition–gene edges and their ids, the causal-gene rule
//! ([`atlas_core::evidence::GeneLink::is_causal`], applied per gene edge), when gene links count as
//! exact, connection discovery and unique-work researcher tallies, how studies are ranked,
//! shared-symptom neighbours, and the breadth-first path over the
//! atlas plus the connected graph. Pure functions over atlas-core; no user-facing text lives here,
//! so the callers keep their own wording (and message keys).

pub mod connections;
pub mod genes;
pub mod path;
pub mod score;
pub mod symptoms;

pub use genes::{
    GENE_RELATION, GeneEdge, MAX_CONDITION_GENES, PHENOTYPE_RELATION, causal_gene_edges, condition_gene_edges,
    gene_edge_id, gene_edges, gene_exact, gene_id, phenotype_edge_id, phenotype_relation,
};
pub use path::{Hop, shortest_path};
pub use score::study_score;
pub use symptoms::{SharedSymptoms, shared_symptoms};
