//! `/api/verify/{id}` and the provenance chain of connected-layer nodes and edges.
//!
//! Connected-layer items (studies, grants, papers, people, organisations and their edges) verify
//! per record: the cached record is re-read at its locator and re-hashed. Atlas items (conditions,
//! genes, phenotypes and their edges) come from whole source files, so they verify per file.

use std::collections::BTreeSet;
use std::path::Path;

use atlas_core::graph::{RecIdx, RecordWithhold, hex};
use atlas_core::node::parse_edge_id;
use atlas_core::provenance::{ActivityIdx, EntityIdx, Provenance};
use atlas_core::{Atlas, Graph};
use atlas_ingest::graph::verify;
use serde_json::{Value, json};

use crate::{nodes, prov};

fn record_json(graph: &Graph, r: RecIdx) -> Value {
    let rec = graph.record(r);
    let e = graph.provenance().entity(rec.entity);
    json!({
        "record": rec.id,
        "locator": format!("{}#{}", e.file, rec.locator),
        "prov:Entity": e.id,
        "url": rec.url,
        "fetched_at": rec.fetched_at,
        "sha256": hex(&rec.sha256),
        "hash_form": rec.hash,
    })
}

fn activity_json(prov: &Provenance, idx: ActivityIdx) -> Value {
    let a = prov.activity(idx);
    let used: Vec<&str> = a.used.iter().map(|&e| prov.entity(e).id.as_str()).collect();
    json!({
        "@id": a.id, "@type": "prov:Activity", "rdfs:label": a.label,
        "prov:startedAtTime": a.started_at, "prov:endedAtTime": a.ended_at, "prov:used": used,
        "prov:wasAssociatedWith": { "@type": "prov:SoftwareAgent", "name": a.agent.name, "version": a.agent.version, "commit": a.agent.commit },
        "parameters": a.parameters, "counts": a.counts,
    })
}

/// Entities of the records plus the activities that read them.
fn bundle(graph: &Graph, records: &[RecIdx], extra: Option<ActivityIdx>) -> (Vec<Value>, Vec<Value>) {
    let prov = graph.provenance();
    let entities: BTreeSet<u16> = records.iter().map(|&r| graph.record(r).entity.0).collect();
    let mut acts: BTreeSet<u16> = extra.map(|a| a.0).into_iter().collect();
    for (i, a) in prov.activities.iter().enumerate() {
        if a.used.iter().any(|e| entities.contains(&e.0)) && a.id.contains("ingest") {
            acts.insert(i as u16);
        }
    }
    (
        entities
            .iter()
            .map(|&e| prov::entity(prov.entity(EntityIdx(e))))
            .collect(),
        acts.iter().map(|&a| activity_json(prov, ActivityIdx(a))).collect(),
    )
}

/// PROV chain of a connected-layer node or edge.
pub fn graph_chain(atlas: &Atlas, graph: &Graph, id: &str) -> Option<Value> {
    if let Some(i) = graph.edge_by_id(id) {
        let e = graph.edge(i);
        let (entities, activities) = bundle(graph, &e.records, Some(e.activity));
        return Some(json!({
            "@id": id, "@type": "prov:Entity", "kind": "edge",
            "from": nodes::any_ref(atlas, graph, &e.from), "relation": e.relation.as_str(),
            "to": nodes::any_ref(atlas, graph, &e.to),
            "edge_kind": e.kind, "level": e.level, "reason": e.reason,
            "prov:wasGeneratedBy": graph.provenance().activity(e.activity).id,
            "prov:wasDerivedFrom": e.records.iter().map(|&r| record_json(graph, r)).collect::<Vec<_>>(),
            "entities": entities, "activities": activities,
        }));
    }
    let key = graph.node(id)?;
    let records = graph.node_records(key);
    let (entities, activities) = bundle(graph, records, None);
    let node = graph.node_ref(key);
    let mut body = json!({
        "@id": node.id, "@type": "prov:Entity", "kind": node.kind, "label": node.label,
        "prov:wasDerivedFrom": records.iter().map(|&r| record_json(graph, r)).collect::<Vec<_>>(),
        "entities": entities, "activities": activities,
    });
    if key.kind == atlas_core::node::NodeKind::Person {
        let p = graph.person(key.idx);
        let same_as: Vec<Value> = graph
            .incident(id)
            .filter(|i| i.edge.relation == atlas_core::graph::Relation::SameAs)
            .map(|i| json!({ "edge_id": i.edge.id(), "other": i.other, "reason": i.edge.reason }))
            .collect();
        body["identity"] = json!({
            "source": p.source, "matched_by": p.matched_by, "orcids": p.orcids,
            "name_variants": p.name_variants, "merge_basis": p.merge_basis, "same_as": same_as,
        });
    }
    Some(body)
}

/// Resolve any merged member, including an atlas condition/gene, to its complete merge lineage.
pub fn chain(atlas: &Atlas, graph: &Graph, id: &str) -> Option<Value> {
    let canonical = graph.canonical_id(atlas_ingest::graph::research::norm_curie(id));
    let mut body = graph_chain(atlas, graph, canonical).or_else(|| prov::chain(atlas, canonical))?;
    if let Some(merge) = graph.identity_merge(canonical) {
        let records: Vec<_> = merge.mappings.iter().map(|m| m.record).collect();
        if graph.records_withheld(&records).is_some()
            || graph.node(canonical).is_some_and(|k| graph.node_withheld(k).is_some())
        {
            return None;
        }
        let mut activity = serde_json::to_value(&merge.activity).ok()?;
        activity["@id"] = json!(merge.activity.id);
        activity["@type"] = json!("prov:Activity");
        activity["prov:used"] = json!(
            merge
                .activity
                .used
                .iter()
                .map(|&e| graph.provenance().entity(e).id.clone())
                .collect::<Vec<_>>()
        );
        activity["prov:startedAtTime"] = json!(merge.activity.started_at);
        activity["prov:endedAtTime"] = json!(merge.activity.ended_at);
        activity["prov:wasAssociatedWith"] = json!(merge.activity.agent);
        let mappings: Vec<_> = merge
            .mappings
            .iter()
            .map(|m| {
                let mut row = serde_json::to_value(m).unwrap();
                row["mapping_row"] = record_json(graph, m.record);
                row["mapping_set_entity"] = prov::entity(graph.provenance().entity(graph.record(m.record).entity));
                row["prov:wasDerivedFrom"] = json!({"@type": "prov:Entity", "url": m.evidence_url,
                    "locator": m.evidence_locator, "sha256": m.evidence_sha256, "hash_scope": "input_file"});
                row
            })
            .collect();
        let members: Vec<_> = merge
            .members
            .iter()
            .map(|m| {
                json!({"@id": m.id,
            "prov:wasDerivedFrom": m.derived_from.iter().map(|&r| record_json(graph, r)).collect::<Vec<_>>() })
            })
            .collect();
        body["requested_id"] = json!(id);
        body["identity_merge"] = json!({"canonical": canonical, "prov:wasGeneratedBy": merge.activity.id,
            "activity": activity, "members": members, "mapping_rows": mappings});
    }
    Some(body)
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    use atlas_core::graph::GraphData;
    use atlas_core::provenance::Locator;

    struct TestData(std::path::PathBuf);

    impl Drop for TestData {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(count: usize, mismatch: Option<usize>) -> (TestData, Atlas, Graph) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = TestData(std::env::temp_dir().join(format!("atlas-verify-{}-{unique}", std::process::id())));
        let file = dir.0.join("cache/mappings/fixture.sssom.tsv");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "MONDO:1\tfixture").unwrap();
        let digest = atlas_ingest::sources::sha256(&file).unwrap();
        let mut hash = [0; 32];
        for (i, byte) in hash.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).unwrap();
        }
        let (atlas, graph) = super::merge_tests::fixture();
        let mut data: GraphData = graph.data().clone();
        let mut record = data.records[0].clone();
        record.locator = Locator::Line(1);
        record.sha256 = hash;
        let mapping = data.identity_merges[0].mappings[0].clone();
        data.records = vec![record; count];
        data.identity_merges[0].mappings = (0..count)
            .map(|i| {
                let mut m = mapping.clone();
                m.record = i as RecIdx;
                m
            })
            .collect();
        if let Some(i) = mismatch {
            data.records[i].sha256 = [0; 32];
        }
        (dir, atlas, Graph::new(data))
    }

    #[test]
    fn an_unchecked_changed_record_never_reports_complete_verification() {
        let (dir, atlas, graph) = fixture(verify::MAX_RECORDS + 1, Some(verify::MAX_RECORDS));
        let result = verify(&dir.0, &atlas, &graph, "MONDO:1").unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["verified"], false);
        assert_eq!(result["checked"], verify::MAX_RECORDS);
        assert_eq!(result["total"], verify::MAX_RECORDS + 1);
        assert_eq!(result["not_checked"], 1);
        assert!(
            result["records"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["matches"] == true)
        );
    }

    #[test]
    fn complete_failed_and_empty_checks_remain_distinct() {
        for (count, mismatch, status, verified) in [
            (1, None, "complete", true),
            (1, Some(0), "failed", false),
            (0, None, "unavailable", false),
        ] {
            let (dir, atlas, graph) = fixture(count, mismatch);
            let result = verify(&dir.0, &atlas, &graph, "MONDO:1").unwrap();
            assert_eq!(result["status"], status);
            assert_eq!(result["verified"], verified);
            assert_eq!(result["total"], count);
        }
    }
}

/// Re-read and re-hash cached bytes; `None` for an unknown id.
pub fn verify(data: &Path, atlas: &Atlas, graph: &Graph, id: &str) -> Option<Value> {
    let id = graph.canonical_id(id);
    let graph_records: Option<(&str, Vec<RecIdx>)> = match graph.edge_by_id(id) {
        Some(i) => Some(("edge", graph.edge(i).records.clone())),
        None => graph.node(id).map(|k| ("node", graph.node_records(k).to_vec())),
    };
    let graph_records = graph_records.or_else(|| graph.identity_merge(id).map(|_| ("node", Vec::new())));
    if let Some((kind, mut records)) = graph_records {
        if let Some(merge) = graph.identity_merge(id) {
            records.extend(merge.mappings.iter().map(|m| m.record));
            records.sort_unstable();
            records.dedup();
        }
        let checked: Vec<_> = records
            .iter()
            .take(verify::MAX_RECORDS)
            .map(|&r| verify::record(data, graph, r))
            .collect();
        let not_checked = records.len().saturating_sub(checked.len());
        let status = if checked.is_empty() {
            "unavailable"
        } else if checked.iter().any(|c| !c.matches) {
            "failed"
        } else if not_checked > 0 {
            "partial"
        } else {
            "complete"
        };
        let summary_msg = match status {
            "complete" => verification_message("record", true),
            "partial" => crate::copy_extra::msg("verify.record.partial", json!({})),
            "failed" => verification_message("record", false),
            _ => crate::copy_extra::msg("verify.record.unavailable", json!({})),
        };
        let live: Vec<Value> = checked
            .iter()
            .filter_map(|c| c.url.as_ref().map(|u| json!({ "record": c.record_id, "url": u })))
            .collect();
        return Some(json!({
            "id": id, "kind": kind, "method": "record", "verified": status == "complete",
            "status": status, "summary": summary_msg["fallback"], "summary_msg": summary_msg,
            "checked": checked.len(), "total": records.len(), "records": checked.iter().map(verification_record).collect::<Vec<_>>(), "not_checked": not_checked,
            "check_live": live,
        }));
    }
    // atlas items: the files they were built from
    let mut entities: BTreeSet<u16> = BTreeSet::new();
    let mut locators: Vec<String> = Vec::new();
    let mut add = |r: &atlas_core::provenance::RecordRef| {
        entities.insert(r.entity.0);
        if locators.len() < 20 {
            locators.push(atlas.provenance.cite(r));
        }
    };
    let kind;
    if let Some((from, relation, to)) = parse_edge_id(id) {
        let d = atlas.disease_at(atlas.disease_idx(from)?);
        kind = "edge";
        match relation {
            "has_phenotype" | "lacks_phenotype" => {
                let t = atlas.hpo.canonical(to)?;
                let e = if relation == "lacks_phenotype" {
                    d.excluded_phenotype(t)
                } else {
                    d.phenotype(t)
                }?;
                e.annotations.iter().for_each(|a| add(&a.record));
            }
            crate::views::GENE_RELATION => d
                .genes
                .iter()
                .filter(|g| g.symbol == to || crate::views::gene_ref(atlas, g).id == to)
                .for_each(|g| add(&g.record)),
            _ => return None,
        }
    } else if let Some(d) = atlas.disease_idx(id) {
        kind = "node";
        atlas.disease_at(d).derived_from.iter().for_each(&mut add);
    } else if let Some(g) = atlas.gene(id) {
        kind = "node";
        let gene = atlas.gene_at(g);
        for &d in &gene.diseases {
            atlas
                .disease_at(d)
                .genes
                .iter()
                .filter(|l| l.symbol == gene.symbol)
                .for_each(|l| add(&l.record));
        }
    } else if atlas.hpo.canonical(id.trim()).is_some() {
        kind = "node";
        let hp = atlas.provenance.entity_by_file(atlas_ingest::sources::HP_OBO)?;
        entities.insert(hp.0);
        locators.push(format!("{}#{}", atlas_ingest::sources::HP_OBO, id.trim()));
    } else {
        return None;
    }
    if locators.is_empty() {
        return None;
    }
    let files: Vec<_> = entities
        .iter()
        .map(|&e| verify::file(data, atlas.provenance.entity(EntityIdx(e))))
        .collect();
    let ok = !files.is_empty() && files.iter().all(|f| f.matches);
    let live: Vec<Value> = files
        .iter()
        .map(|f| json!({ "source": f.source, "url": f.url }))
        .collect();
    Some(json!({
        "id": id, "kind": kind, "method": "file", "verified": ok,
        "status": if ok { "complete" } else if files.is_empty() { "unavailable" } else { "failed" },
        "checked": files.len(), "total": files.len(), "not_checked": 0,
        "summary": verification_message("file", ok)["fallback"], "summary_msg": verification_message("file", ok),
        "records": locators, "files": files.iter().map(verification_record).collect::<Vec<_>>(), "check_live": live,
    }))
}

fn verification_message(method: &str, ok: bool) -> Value {
    crate::copy_extra::msg(
        &format!("verify.{method}.{}", if ok { "matched" } else { "changed" }),
        json!({}),
    )
}

fn verification_record(record: &impl serde::Serialize) -> Value {
    let mut v = serde_json::to_value(record).expect("verification serializes");
    if v["error"].is_string() {
        let msg = crate::copy_extra::msg("verify.unreadable", json!({}));
        v["error"] = msg["fallback"].clone();
        v["error_msg"] = msg;
    }
    v
}

pub fn integrity(atlas: &Atlas, graph: &Graph) -> Value {
    let report = atlas_core::integrity::check(atlas, graph);
    let mut v = serde_json::to_value(report).expect("integrity report serializes");
    if let Some(coverage) = v["coverage"].as_array_mut() {
        for c in coverage {
            c["scope_msg"] = crate::copy::scope(
                c["source"].as_str().unwrap_or_default(),
                c["scope"].as_str().unwrap_or_default(),
            );
        }
    }
    v
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use atlas_core::graph::{GraphData, IdentityMapping, IdentityMember, IdentityMerge, RecordHash, SourceRecord};
    use atlas_core::provenance::{Activity, Agent, Locator, Provenance, RecordRef, SourceEntity};

    pub(super) fn fixture() -> (Atlas, Graph) {
        let mut prov = Provenance::default();
        let entity = prov.add_entity(SourceEntity {
            id: "source:mondo".into(),
            file: "mondo.obo".into(),
            sha256: Some("a".repeat(64)),
            ..Default::default()
        });
        let act = prov.add_activity(Activity {
            id: "activity:ingest-mondo".into(),
            used: vec![entity],
            ..Default::default()
        });
        let mut d = atlas_core::Disease::new("MONDO:1", act);
        d.name = "fixture disease".into();
        d.derive_from(RecordRef::record(entity, "MONDO:1"));
        let atlas = Atlas::new(
            vec![atlas_core::Term::new("HP:0000001")],
            atlas_core::DiseaseIdentity::default(),
            prov,
            vec![d],
        );
        let mut data = GraphData::default();
        let e = data.provenance.add_entity(SourceEntity {
            id: "source:mapping".into(),
            file: "cache/mappings/fixture.sssom.tsv".into(),
            sha256: Some("b".repeat(64)),
            ..Default::default()
        });
        data.records.push(SourceRecord {
            entity: e,
            locator: Locator::Line(2),
            id: "MONDO:1".into(),
            url: Some("https://example.org/source".into()),
            fetched_at: None,
            hash: RecordHash::TsvLine,
            sha256: [1; 32],
        });
        data.aliases.push(("DOID:1".into(), "MONDO:1".into()));
        data.identity_merges.push(IdentityMerge {
            canonical: "MONDO:1".into(),
            activity: Activity {
                id: "activity:identity-merge:fixture".into(),
                label: "identity merge".into(),
                used: vec![e],
                started_at: Some("2026-10-04T00:00:00Z".into()),
                agent: Agent {
                    name: "fixture-engine".into(),
                    version: "1.0.0".into(),
                    commit: None,
                },
                ..Default::default()
            },
            members: vec![
                IdentityMember {
                    id: "MONDO:1".into(),
                    derived_from: vec![0],
                },
                IdentityMember {
                    id: "DOID:1".into(),
                    derived_from: vec![0],
                },
            ],
            mappings: vec![IdentityMapping {
                subject: "MONDO:1".into(),
                object: "DOID:1".into(),
                mapping_set_id: "https://example.org/mapping".into(),
                mapping_set_version: "fixture-v1".into(),
                record: 0,
                rule_id: "R-DIS-01".into(),
                rule_version: "1.0.0".into(),
                evidence_url: "https://example.org/source".into(),
                evidence_sha256: "c".repeat(64),
                evidence_locator: "record fixture".into(),
                assertion_id: String::new(),
                decision_id: String::new(),
                gate_manifest_sha256: String::new(),
                mapping_tool: "fixture-align 1.0.0".into(),
            }],
        });
        (atlas, Graph::new(data))
    }

    #[test]
    fn atlas_canonical_and_any_merged_alias_show_the_same_rule_bound_lineage() {
        let (atlas, graph) = fixture();
        let canonical = chain(&atlas, &graph, "MONDO:1").unwrap();
        let alias = chain(&atlas, &graph, "DOID:1").unwrap();
        assert_eq!(canonical["identity_merge"], alias["identity_merge"]);
        assert_eq!(alias["identity_merge"]["mapping_rows"][0]["rule_id"], "R-DIS-01");
        assert_eq!(
            alias["identity_merge"]["mapping_rows"][0]["mapping_row"]["sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(alias["identity_merge"]["members"].as_array().unwrap().len(), 2);
        let export: String =
            crate::export::Export::new(&atlas, &graph, atlas.disease_idx("MONDO:1").unwrap()).collect();
        assert!(export.contains("ra:ruleId \"R-DIS-01\""));
        assert!(export.contains("prov:wasGeneratedBy act:activity%3Aidentity-merge%3Afixture"));
        assert!(export.contains("prov:wasDerivedFrom rec:0"));
    }
}
