//! Gene2Phenotype (G2P) gene-disease models for the mechanism layer: the rows parsed by
//! [`crate::g2p`] (the one G2P parser of the workspace), plus the variant effect as a closed set and
//! the atlas conditions each model resolves to. Port of `g2p_by_disease` in `mechanism.py`.

use std::path::Path;

use atlas_core::Atlas;
pub use atlas_core::mechanism::g2p::{ESTABLISHED, Effect, G2pRecord};
use atlas_core::provenance::EntityIdx;

use crate::error::IngestError;

#[derive(Debug, Default)]
pub struct G2pCounts {
    pub read: u64,
    pub resolved: u64,
    pub unresolved: u64,
    pub own_node: u64,
}

pub fn read(path: &Path, entity: EntityIdx, atlas: &Atlas) -> Result<(Vec<G2pRecord>, G2pCounts), IngestError> {
    let active = |id: &str| {
        atlas
            .disease_idx(id)
            .map(|i| atlas.disease_at(i))
            .is_some_and(|d| d.is_active() && d.id == id)
    };
    let mut n = G2pCounts::default();
    let mut out = Vec::new();
    for r in crate::g2p::read_g2p(path, entity)? {
        n.read += 1;
        let ids: Vec<String> = [r.disease_mondo.clone(), r.disease_omim.clone()]
            .into_iter()
            .flatten()
            .collect();
        let mut diseases: Vec<String> = ids
            .iter()
            .map(|i| atlas.identity.resolve(i))
            .filter(|t| active(t))
            .collect();
        diseases.sort();
        diseases.dedup();
        let own = format!("G2P:{}", r.id);
        let node = active(&own).then_some(own);
        n.own_node += u64::from(node.is_some());
        if diseases.is_empty() {
            n.unresolved += 1;
        } else {
            n.resolved += 1;
        }
        out.push(G2pRecord {
            mechanism: Effect::from_g2p(&r.molecular_mechanism),
            g2p_id: r.id,
            symbol: r.symbol,
            hgnc_id: r.hgnc,
            disease_name: r.disease_name,
            disease_ids: ids,
            diseases,
            node,
            allelic_requirement: r.allelic_requirement,
            confidence: r.confidence,
            mechanism_raw: r.molecular_mechanism,
            mechanism_support: r.molecular_mechanism_support,
            variant_consequence: r.variant_consequence,
            publications: r.publications,
            panels: r.panels,
            reviewed: r.last_review,
            record: r.record,
        });
    }
    Ok((out, n))
}
