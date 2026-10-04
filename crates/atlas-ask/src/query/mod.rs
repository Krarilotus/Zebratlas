//! D57/D57a: linked entities → typed plan → deterministic execution; bounded SPARQL power mode.
pub mod boundary;
pub mod conversation;
pub mod engine;
pub mod guard;
pub mod plan;
pub mod prompt;
pub mod routes;
pub mod schema;
pub mod suggestions;

pub use engine::{QueryEngine, QueryResult};
pub use plan::{LinkedEntity, QueryPlan};
pub use schema::SchemaCard;

pub const BASE: &str = "https://w3id.org/rare-disease-atlas/";
pub const RA: &str = "https://w3id.org/rare-disease-atlas/vocab#";
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const PROV: &str = "http://www.w3.org/ns/prov#";
pub const DCT: &str = "http://purl.org/dc/terms/";
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Same UTF-8 percent encoding as the release exporter; never interpolate unescaped identifiers.
pub fn iri(kind: &str, id: &str) -> String {
    let mut encoded = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("<{BASE}{kind}/{encoded}>")
}

pub fn literal(s: &str) -> String {
    spargebra::term::Literal::new_simple_literal(s).to_string()
}

pub const PREFIXES: &str = "PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#>\nPREFIX prov: <http://www.w3.org/ns/prov#>\nPREFIX dcterms: <http://purl.org/dc/terms/>\nPREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\nPREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n";

impl crate::Asker {
    /// Verify exact IDs supplied by retrieval or an edited plan. Labels are graph-owned.
    pub fn link_query_entities(&self, ids: &[String]) -> Result<Vec<LinkedEntity>, String> {
        link_query_entities(&self.atlas, &self.graph, ids)
    }
    /// Search-ir passes linked graph IDs here. Resolve by exact ID, and take labels from the
    /// graph rather than the client. The connected provider/key is reused from the ask boundary.
    pub async fn understand_query(
        &self,
        engine: &QueryEngine,
        req: &conversation::QueryRequest,
        key: Option<atlas_llm::ApiKey>,
    ) -> Result<conversation::QueryAnswer, String> {
        let mut trusted = req.clone();
        trusted.linked = self.link_query_entities(&req.linked.iter().map(|e| e.id.clone()).collect::<Vec<_>>())?;
        engine.understand(&self.llm, &trusted, key).await
    }
}

/// Canonical linking also serves direct query edits, with no model dependency.
pub fn link_query_entities(
    atlas: &atlas_core::Atlas,
    graph: &atlas_core::Graph,
    ids: &[String],
) -> Result<Vec<LinkedEntity>, String> {
    use atlas_core::graph::RecordWithhold;
    if ids.len() > 32 {
        return Err("too many linked entities".into());
    }
    let ctx = crate::exec::Ctx::new(atlas, graph);
    ids.iter()
        .map(|id| {
            if let Some(idx) = graph.edge_by_id(id) {
                let edge = graph.edge(idx);
                if graph.records_withheld(&edge.records).is_some()
                    || [&edge.from, &edge.to]
                        .into_iter()
                        .any(|end| graph.node(end).is_some_and(|k| graph.node_withheld(k).is_some()))
                {
                    return Err("linked assertion withheld".into());
                }
                return Ok(LinkedEntity {
                    id: id.clone(),
                    label: format!(
                        "{} {} {}",
                        ctx.label(&edge.from),
                        edge.relation.as_str(),
                        ctx.label(&edge.to)
                    ),
                });
            }
            let (target, _) = ctx.resolve_any(id).ok_or("unknown linked graph ID")?;
            let node = ctx.target_ref(target);
            if node.id != *id {
                return Err("entity must be linked to its canonical exact graph ID first".into());
            }
            if graph.node(&node.id).is_some_and(|k| graph.node_withheld(k).is_some()) {
                return Err("linked entity withheld".into());
            }
            Ok(LinkedEntity {
                id: node.id,
                label: node.label,
            })
        })
        .collect()
}
