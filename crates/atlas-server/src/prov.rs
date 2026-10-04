//! `/api/provenance/{id}`: the PROV-O chain of a node (disease, phenotype, gene) or an edge
//! (`from|relation|to`): generating activities, source records, merges, classifications, and the
//! full `prov:Entity` / `prov:Activity` records they reference (keys use PROV-O names).

use std::collections::BTreeSet;

use atlas_core::node::parse_edge_id;
use atlas_core::provenance::{Activity, ActivityIdx, EntityIdx, RecordRef, SourceEntity, activity};
use atlas_core::{Atlas, DiseaseIdx};
use serde_json::{Value, json};

use crate::views::{self, GENE_RELATION};

pub fn entity(e: &SourceEntity) -> Value {
    json!({
        "@id": e.id,
        "@type": "prov:Entity",
        "prov:atLocation": e.url,
        "file": e.file,
        "version": e.version,
        "retrieved_at": e.retrieved_at,
        "sha256": e.sha256,
        "bytes": e.bytes,
        "licence": e.licence,
    })
}

fn activity_json(atlas: &Atlas, a: &Activity) -> Value {
    let used: Vec<&str> = a.used.iter().map(|&e| atlas.provenance.entity(e).id.as_str()).collect();
    json!({
        "@id": a.id,
        "@type": "prov:Activity",
        "rdfs:label": a.label,
        "prov:startedAtTime": a.started_at,
        "prov:endedAtTime": a.ended_at,
        "prov:used": used,
        "prov:wasAssociatedWith": {
            "@type": "prov:SoftwareAgent",
            "name": a.agent.name,
            "version": a.agent.version,
            "commit": a.agent.commit,
        },
        "parameters": a.parameters,
        "counts": a.counts,
    })
}

/// Collects the entities and activities a chain mentions.
struct Chain<'a> {
    atlas: &'a Atlas,
    entities: BTreeSet<u16>,
    activities: BTreeSet<u16>,
}

impl<'a> Chain<'a> {
    fn new(atlas: &'a Atlas) -> Self {
        Self {
            atlas,
            entities: BTreeSet::new(),
            activities: BTreeSet::new(),
        }
    }

    fn activity_idx(&mut self, idx: ActivityIdx) -> &'a str {
        self.activities.insert(idx.0);
        &self.atlas.provenance.activity(idx).id
    }

    fn activity_id(&mut self, id: &str) -> Option<String> {
        let i = self.atlas.provenance.activities.iter().position(|a| a.id == id)?;
        self.activities.insert(i as u16);
        Some(id.to_owned())
    }

    /// A source record (`prov:wasDerivedFrom`), noting its entity and the activity that read it.
    fn record(&mut self, r: &RecordRef) -> Value {
        self.entities.insert(r.entity.0);
        let generated_by = self.generator(r.entity);
        json!({
            "record": self.atlas.provenance.cite(r),
            "prov:Entity": self.atlas.provenance.entity(r.entity).id,
            "prov:wasGeneratedBy": generated_by,
        })
    }

    fn generator(&mut self, e: EntityIdx) -> Option<String> {
        let id = self.atlas.provenance.generator_of(e)?.id.clone();
        self.activity_id(&id)
    }

    fn finish(self, mut body: Value) -> Value {
        let prov = &self.atlas.provenance;
        let mut entities = self.entities;
        for &a in &self.activities {
            entities.extend(prov.activity(ActivityIdx(a)).used.iter().map(|e| e.0));
        }
        body["entities"] = entities.iter().map(|&e| entity(prov.entity(EntityIdx(e)))).collect();
        body["activities"] = self
            .activities
            .iter()
            .map(|&a| activity_json(self.atlas, prov.activity(ActivityIdx(a))))
            .collect();
        body
    }
}

pub fn chain(atlas: &Atlas, id: &str) -> Option<Value> {
    if let Some((from, relation, to)) = parse_edge_id(id) {
        return edge(atlas, id, from, relation, to);
    }
    if let Some(d) = atlas.disease_idx(id) {
        return Some(disease(atlas, d));
    }
    if let Some(t) = atlas.hpo.canonical(id.trim()) {
        let mut c = Chain::new(atlas);
        let hp = atlas.provenance.entity_by_file("hp.obo")?;
        let term = &atlas.hpo.term(t).id;
        let body = json!({
            "@id": term,
            "@type": "prov:Entity",
            "kind": "phenotype",
            "prov:wasGeneratedBy": c.activity_id(activity::INGEST_HPO),
            "prov:wasDerivedFrom": [c.record(&RecordRef::record(hp, term.clone()))],
            "ic": atlas.hpo.ic(t),
            "ic_generated_by": c.activity_id(activity::COMPUTE_IC),
        });
        return Some(c.finish(body));
    }
    let g = atlas.gene(id)?;
    let gene = atlas.gene_at(g);
    let mut c = Chain::new(atlas);
    let records: Vec<Value> = gene
        .diseases
        .iter()
        .flat_map(|&d| atlas.disease_at(d).genes.iter().filter(|l| l.symbol == gene.symbol))
        .map(|l| c.record(&l.record))
        .collect();
    let body = json!({
        "@id": gene.id(),
        "@type": "prov:Entity",
        "kind": "gene",
        "label": gene.symbol,
        "prov:wasDerivedFrom": records,
    });
    Some(c.finish(body))
}

fn disease(atlas: &Atlas, idx: DiseaseIdx) -> Value {
    let d = atlas.disease_at(idx);
    let mut c = Chain::new(atlas);
    let generated_by = c.activity_idx(d.generated_by);
    let derived: Vec<Value> = d.derived_from.iter().map(|r| c.record(r)).collect();
    let merges: Vec<Value> = d
        .source_ids
        .iter()
        .map(|s| match atlas.identity.merged_by(s) {
            Some(m) => json!({
                "source_id": s,
                "merged_by": m.basis.as_str(),
                "mapping_basis": m.basis.mapping_basis().as_str(),
                "prov:wasGeneratedBy": c.activity_id(m.basis.activity_id()),
                "asserted_in": m.asserted_in,
            }),
            None => json!({ "source_id": s, "merged_by": null, "note": "resolved via MONDO replaced_by or id normalisation" }),
        })
        .collect();
    let classifications: Vec<Value> = d
        .classifications
        .iter()
        .map(|cl| {
            json!({
                "property": cl.property,
                "value": cl.value,
                "reason": cl.reason,
                "prov:wasGeneratedBy": c.activity_idx(cl.activity),
                "record": cl.record.as_ref().map(|r| atlas.provenance.cite(r)),
            })
        })
        .collect();
    let ids: Vec<&str> = std::iter::once(d.id.as_str())
        .chain(d.source_ids.iter().map(String::as_str))
        .collect();
    let mut conflicts: Vec<Value> = Vec::new();
    for conflict in atlas.identity.conflicts().iter().filter(|x| {
        ids.iter()
            .any(|i| *i == x.source_id || x.candidates.iter().any(|y| y == i))
    }) {
        conflicts.push(serde_json::to_value(conflict).expect("conflict serialises"));
    }
    let body = json!({
        "@id": d.id,
        "@type": "prov:Entity",
        "kind": "disease",
        "status": d.status,
        "prov:wasGeneratedBy": generated_by,
        "prov:wasDerivedFrom": derived,
        "merges": merges,
        // D30.1: a name match nominates, never merges
        "candidate_same_as": atlas.identity.candidate(&d.id).map(|x| json!({
            "target": x.target, "target_label": x.target_label, "matched_labels": x.matched_labels,
            "guard": x.guard, "mapping_basis": "candidate",
            "prov:wasGeneratedBy": c.activity_id(atlas_core::provenance::activity::IDENTITY_LABEL),
        })),
        "classifications": classifications,
        "identity_conflicts": conflicts,
    });
    c.finish(body)
}

/// Source records of an atlas edge (condition–phenotype or condition–gene), in source order.
pub fn edge_records<'a>(atlas: &'a Atlas, from: &str, relation: &str, to: &str) -> Option<Vec<&'a RecordRef>> {
    let d = atlas.disease_at(atlas.disease_idx(from)?);
    let records: Vec<&RecordRef> = match relation {
        r if r == views::phenotype_relation(false) || r == views::phenotype_relation(true) => {
            let t = atlas.hpo.canonical(to)?;
            let absent = r == views::phenotype_relation(true);
            let e = if absent {
                d.excluded_phenotype(t)
            } else {
                d.phenotype(t)
            }?;
            e.annotations.iter().map(|a| &a.record).collect()
        }
        GENE_RELATION => d
            .genes
            .iter()
            .filter(|g| g.symbol == to || views::gene_ref(atlas, g).id == to)
            .map(|g| &g.record)
            .collect(),
        _ => return None,
    };
    Some(records).filter(|r| !r.is_empty())
}

fn edge(atlas: &Atlas, id: &str, from: &str, relation: &str, to: &str) -> Option<Value> {
    let d = atlas.disease_at(atlas.disease_idx(from)?);
    let records = edge_records(atlas, from, relation, to)?;
    let mut c = Chain::new(atlas);
    let derived: Vec<Value> = records.into_iter().map(|r| c.record(r)).collect();
    let generated_by: BTreeSet<String> = derived
        .iter()
        .filter_map(|r| r["prov:wasGeneratedBy"].as_str().map(str::to_owned))
        .collect();
    let body = json!({
        "@id": id,
        "@type": "prov:Entity",
        "kind": "edge",
        "from": d.id,
        "relation": relation,
        "to": to,
        "prov:wasGeneratedBy": generated_by,
        "prov:wasDerivedFrom": derived,
    });
    Some(c.finish(body))
}
