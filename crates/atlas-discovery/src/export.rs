//! Bundle validation, PROV-O JSON-LD, writing, the ingest plan and review decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::*;

/// Shape checks before any output is considered a valid ingest candidate.
pub fn validate(b: &Bundle) -> Result<()> {
    let nodes: BTreeSet<_> = b.nodes.iter().map(|n| &n.id).collect();
    ensure!(nodes.len() == b.nodes.len(), "duplicate graph node");
    for edge in &b.edges {
        ensure!(nodes.contains(&edge.from) && nodes.contains(&edge.to), "dangling edge");
        ensure!(!edge.evidence.is_empty(), "unsourced edge");
        ensure!(edge.relation != "skos:exactMatch", "asset identity merge is forbidden");
    }
    for r in &b.records {
        ensure!(
            (r.locator.entity.0 as usize) < b.provenance.entities.len(),
            "dangling provenance"
        );
        if let Some(a) = &r.alignment {
            ensure!(
                a.decision != Decision::Exact || a.targets.len() == 1,
                "exact mapping must have one target"
            );
        }
    }
    let exact: BTreeSet<_> = b
        .records
        .iter()
        .filter_map(|r| r.alignment.as_ref())
        .filter(|a| a.decision == Decision::Exact)
        .map(|a| (&a.mention.id, &a.targets[0]))
        .collect();
    ensure!(exact.len() == b.mappings.len(), "mapping cardinality");
    for m in &b.mappings {
        ensure!(
            m.predicate_id == "skos:exactMatch" && exact.contains(&(&m.subject_id, &m.object_id)),
            "unapproved mapping"
        );
    }
    Ok(())
}

/// Standards vocabulary is expressed as JSON-LD, including exact record-level derivations.
pub fn prov_jsonld(b: &Bundle) -> Value {
    let activity = &b.provenance.activities[0];
    let aid = &activity.id;
    let agent = "urn:atlas:software:atlas-discovery:0.1.0";
    let mut graph =
        vec![json!({"@id": agent, "@type": "prov:SoftwareAgent", "atlas:version": env!("CARGO_PKG_VERSION")})];
    for activity in &b.provenance.activities {
        graph.push(json!({"@id":activity.id,"@type":"prov:Activity","prov:wasAssociatedWith":{"@id":agent},
            "prov:used":activity.used.iter().map(|i| json!({"@id":b.provenance.entities[i.0 as usize].id})).collect::<Vec<_>>(),
            "atlas:label":activity.label,"atlas:parameters":activity.parameters,"atlas:counts":activity.counts}));
    }
    for s in &b.provenance.entities {
        graph.push(
            json!({"@id":s.id,"@type":"prov:Entity","atlas:url":s.url,"atlas:sha256":s.sha256,
            "atlas:version":s.version,"prov:generatedAtTime":s.retrieved_at}),
        );
    }
    let mut refs = BTreeMap::new();
    for r in b
        .records
        .iter()
        .map(|r| &r.locator)
        .chain(b.targets.iter().map(|t| &t.evidence))
        .chain(b.review_queue.iter().map(|r| &r.evidence))
        .chain(b.characterizations.iter().map(|c| &c.evidence))
    {
        let sid = &b.provenance.entities[r.entity.0 as usize].id;
        let rid = format!("{sid}#{}", r.locator);
        refs.insert(
            rid.clone(),
            json!({"@id":rid,"@type":"prov:Entity","atlas:locator":r.locator.to_string(),
            "prov:specializationOf":{"@id":sid}}),
        );
    }
    graph.extend(refs.into_values());
    let catalog = b.sources.sources.iter().find(|s| s.id == "panelapp-catalog").unwrap();
    graph.push(
        json!({"@id":format!("urn:atlas:discovery:{}",b.manifest_sha256),"@type":"prov:Entity",
        "prov:wasGeneratedBy":{"@id":format!("{aid}:discover")},
        "prov:wasDerivedFrom":{"@id":format!("urn:sha256:{}",catalog.sha256)},"atlas:selection":b.sources.discovery}),
    );
    for c in &b.characterizations {
        graph.push(json!({"@id":format!("urn:atlas:characterization:{}:{}",b.manifest_sha256,c.source),"@type":"prov:Entity",
            "prov:wasGeneratedBy":{"@id":format!("{aid}:characterize")},
            "prov:wasDerivedFrom":{"@id":format!("{}#{}",b.provenance.entities[c.evidence.entity.0 as usize].id,c.evidence.locator)}}));
    }
    for r in &b.review_queue {
        graph.push(json!({"@id":r.id,"@type":"prov:Entity","prov:wasGeneratedBy":{"@id":format!("{aid}:stage")},
            "prov:wasDerivedFrom":{"@id":format!("{}#{}",b.provenance.entities[r.evidence.entity.0 as usize].id,r.evidence.locator)},
            "atlas:status":r.status,"atlas:reason":r.reason}));
    }
    for n in &b.nodes {
        let references: Vec<Value> = if let Some(t) = b.targets.iter().find(|t| t.id == n.id) {
            vec![
                json!({"@id":format!("{}#{}",b.provenance.entities[t.evidence.entity.0 as usize].id,t.evidence.locator)}),
            ]
        } else {
            b.records.iter().filter(|r| r.id.starts_with(&format!("{}#snapshot-", n.id)))
                .map(|r| json!({"@id":format!("{}#{}", b.provenance.entities[r.locator.entity.0 as usize].id, r.locator.locator)}))
                .collect()
        };
        graph.push(
            json!({"@id":format!("urn:atlas:node:{}:{}",b.manifest_sha256,sha256(n.id.as_bytes())),
            "@type":"prov:Entity","atlas:node_id":n.id,"prov:wasGeneratedBy":{"@id":format!("{aid}:stage")},
            "prov:wasDerivedFrom":references}),
        );
    }
    for m in &b.mappings {
        let a = b
            .records
            .iter()
            .filter_map(|r| r.alignment.as_ref())
            .find(|a| a.mention.id == m.subject_id)
            .unwrap();
        let t = b.targets.iter().find(|t| t.id == m.object_id).unwrap();
        let refs = [&a.mention.evidence, &t.evidence]
            .map(|r| json!({"@id":format!("{}#{}", b.provenance.entities[r.entity.0 as usize].id, r.locator)}));
        graph.push(
            json!({"@id":m.mapping_evidence,"@type":"prov:Entity","prov:wasGeneratedBy":{"@id":aid},
            "prov:wasDerivedFrom":refs,"atlas:subject":m.subject_id,"atlas:object":m.object_id}),
        );
    }
    for r in &b.records {
        graph.push(json!({"@id":r.id,"@type":"prov:Entity","prov:wasGeneratedBy":{"@id":format!("{aid}:parse-filter")},
            "prov:wasDerivedFrom":{"@id":format!("{}#{}",b.provenance.entities[r.locator.entity.0 as usize].id,r.locator.locator)},
            "atlas:status":r.status,"atlas:reason":r.reason}));
    }
    for edge in &b.edges {
        let evidence = &edge.evidence[0];
        let source = b.sources.sources.iter().find(|s| s.id == evidence.source).unwrap();
        graph.push(json!({"@id":format!("urn:atlas:edge:{}:{}",b.manifest_sha256,sha256(edge.id.as_bytes())),"@type":"prov:Entity",
            "atlas:edge_id":edge.id,"prov:wasGeneratedBy":{"@id":format!("{aid}:stage")},
            "prov:wasDerivedFrom":{"@id":format!("urn:sha256:{}#{}",source.sha256,evidence.record.as_deref().unwrap_or(""))}}));
    }
    json!({"@context":{"prov":"http://www.w3.org/ns/prov#","atlas":"urn:atlas:vocab:",
        "prov:generatedAtTime":{"@type":"http://www.w3.org/2001/XMLSchema#dateTime"}},"@graph":graph})
}

pub fn write_bundle(b: &Bundle, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, value) in [
        ("bundle.json", serde_json::to_value(b)?),
        ("review-queue.json", serde_json::to_value(&b.review_queue)?),
        ("provenance.jsonld", prov_jsonld(b)),
        ("ingest-plan.json", ingest_plan(b)?),
    ] {
        std::fs::write(dir.join(name), serde_json::to_vec_pretty(&value)?)?;
    }
    let mut w = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .from_path(dir.join("mappings.sssom.tsv"))?;
    for m in &b.mappings {
        w.serialize(m)?;
    }
    w.flush()?;
    std::fs::write(
        dir.join("mappings.sssom.json"),
        serde_json::to_vec_pretty(&json!({
            "mapping_set_id":format!("urn:atlas:mapping-set:{}",b.manifest_sha256),
            "mapping_set_description":"Explicit source gene mentions aligned to current HGNC records; staged outputs",
            "curie_map":{"HGNC":"https://bioregistry.io/hgnc:",
                "skos":"http://www.w3.org/2004/02/skos/core#","semapv":"https://w3id.org/semapv/vocab/"},
            "license":"unspecified (upstream PanelApp licence review pending; do not redistribute source content)"
        }))?,
    )?;
    Ok(())
}

/// Machine-enforced source admission, still requiring the graph owner to resolve endpoints.
/// Review-only mechanism claims never enter this plan. PanelApp stays visible in staging.
pub fn ingest_plan(b: &Bundle) -> Result<Value> {
    validate(b)?;
    let allowed: BTreeSet<_> = b
        .characterizations
        .iter()
        .filter(|c| {
            c.publication_status == "staged_with_attribution"
                && matches!(c.licence.as_deref(), Some("CC-BY-4.0" | "CC0-1.0"))
        })
        .map(|c| &c.source)
        .collect();
    let edges: Vec<_> = b
        .edges
        .iter()
        .filter(|e| e.evidence.iter().all(|v| allowed.contains(&v.source)))
        .collect();
    let ids: BTreeSet<_> = edges.iter().flat_map(|e| [&e.from, &e.to]).collect();
    let nodes: Vec<_> = b.nodes.iter().filter(|n| ids.contains(&n.id)).collect();
    Ok(
        json!({"schema":"atlas.discovery.ingest-plan","version":1,"nodes":nodes,"edges":edges,
        "bundle_manifest_sha256":b.manifest_sha256,"provenance_file":"provenance.jsonld",
        "blocked_sources":b.characterizations.iter().filter(|c| !allowed.contains(&c.source)).collect::<Vec<_>>(),
        "contract":"Resolve gene endpoints against current atlas; reject conflicts; remap local provenance indices; never merge asset identities. This is an import plan, not a published graph."}),
    )
}

/// Review is append-only and never upgrades a name-only suggestion to an exact identity merge.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub review_id: String,
    pub reviewer: String,
    pub reviewed_at: String,
    pub disposition: String,
    pub rationale: String,
    pub evidence_url: String,
    pub bundle_sha256: String,
}
pub fn validate_review(b: &Bundle, d: &ReviewDecision, bundle_bytes: &[u8]) -> Result<()> {
    ensure!(
        b.review_queue.iter().any(|r| r.id == d.review_id),
        "unknown review item"
    );
    ensure!(sha256(bundle_bytes) == d.bundle_sha256, "review is stale");
    ensure!(
        !d.reviewer.trim().is_empty()
            && !d.reviewed_at.trim().is_empty()
            && !d.rationale.trim().is_empty()
            && d.evidence_url.starts_with("https://"),
        "incomplete review evidence"
    );
    if !matches!(d.disposition.as_str(), "reject" | "keep_pending" | "approve_typed_link") {
        bail!("review cannot authorize an identity merge; submit authoritative exact mapping through identity owner");
    }
    Ok(())
}
