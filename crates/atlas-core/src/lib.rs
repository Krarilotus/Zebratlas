//! Rare-disease atlas model: ids, ontology, disease identity, provenance, graph store, search index.
//!
//! No IO except snapshot (de)serialisation. Parsing lives in `atlas-ingest`, scoring in `atlas-analytics`.

pub mod atlas;
pub mod copy;
pub mod curie;
pub mod disease;
pub mod error;
pub mod evidence;
pub mod graph;
pub mod identity;
pub mod identity_policy;
pub mod identity_rules;
pub mod integrity;
pub mod mechanism;
pub mod node;
pub mod ontology;
pub mod provenance;
pub mod query_graph;
pub mod search;
pub mod snapshot;
pub mod term;
pub mod text;
pub mod units;
pub mod withhold;

pub use atlas::{Atlas, Gene, GeneIdx, Stats};
pub use disease::{Classification, Disease, DiseaseIdx, Name, PhenotypeEdge, Status};
pub use error::CoreError;
pub use graph::Graph;
pub use identity::DiseaseIdentity;
pub use ontology::{Ontology, TermIdx};
pub use provenance::Provenance;
pub use term::{Scope, Synonym, Term, Xref};
