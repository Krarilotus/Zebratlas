//! Drugs for research questions, from the Open Targets clinical associations cache
//! (`data/cache/repurposing/opentargets.json`, written by the repurpose agent): drug → target →
//! mechanism and drug → indication, both `observed` clinical-stage assertions, **not** efficacy.
//! Read once, lazily; only `status = "included"` records. Every Cure MATRIX scores are not used:
//! the cache holds only its drug and disease lists so far (no prediction records).

use std::path::Path;
use std::sync::OnceLock;

use atlas_core::node::NodeRef;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct File {
    header: Value,
    records: Vec<Record>,
}

#[derive(Clone, Deserialize)]
struct Target {
    #[serde(default, rename = "approvedSymbol")]
    symbol: Option<String>,
}

#[derive(Clone, Deserialize)]
struct Moa {
    #[serde(default, rename = "mechanismOfAction")]
    mechanism: Option<String>,
    #[serde(default, rename = "actionType")]
    action: Option<String>,
    #[serde(default)]
    targets: Vec<Target>,
}

#[derive(Clone, Deserialize)]
struct Record {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    relation: Option<String>,
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default)]
    drug_id: Option<String>,
    #[serde(default)]
    drug_name: Option<String>,
    #[serde(default)]
    association_max_clinical_stage: Option<String>,
    #[serde(default)]
    drug_maximum_clinical_stage: Option<String>,
    #[serde(default)]
    mechanisms_of_action: Vec<Moa>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    record_locator: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    retrieved_at: Option<String>,
    #[serde(default)]
    data_version: Option<String>,
}

pub struct Drugs {
    records: Vec<Record>,
    source: Value,
}

pub fn load(data: &Path) -> Option<&'static Drugs> {
    static D: OnceLock<Option<Drugs>> = OnceLock::new();
    D.get_or_init(|| {
        let bytes = std::fs::read(data.join("cache/repurposing/opentargets.json")).ok()?;
        match serde_json::from_slice::<File>(&bytes) {
            Ok(f) => Some(Drugs {
                source: json!({
                    "file": "data/cache/repurposing/opentargets.json", "source": f.header["source"],
                    "url": f.header["url"], "retrieved_at": f.header["retrieved_at"], "sha256": f.header["sha256"],
                    "interpretation": "Clinical-stage/target assertions; not proof of efficacy",
                }),
                records: f
                    .records
                    .into_iter()
                    .filter(|r| r.status.as_deref() == Some("included"))
                    .collect(),
            }),
            Err(e) => {
                eprintln!("open targets drugs unavailable: {e}");
                None
            }
        }
    })
    .as_ref()
}

use crate::copy::sentence as msg;

/// Drugs acting on a community's causal gene, or in clinical study for one of the communities.
/// `communities`: (condition, gene symbols). `query`: the asking condition (for "related condition").
pub fn for_question(d: &Drugs, communities: &[(NodeRef, Vec<String>)], query: &str, limit: usize) -> Vec<Value> {
    let mut out: Vec<(u8, Value)> = Vec::new();
    for r in &d.records {
        let drug = r
            .drug_name
            .clone()
            .unwrap_or_else(|| r.drug_id.clone().unwrap_or_default());
        let cite = format!("opentargets:{}", r.id);
        let stage = r.association_max_clinical_stage.clone().unwrap_or_default();
        let base = |kind: &str, sentence: Value, via: Value| {
            json!({
                "drug": { "id": r.drug_id, "kind": "drug", "label": drug }, "edge_kind": "observed", "basis": kind,
                "relation": r.relation, "stage": stage, "drug_max_stage": r.drug_maximum_clinical_stage,
                "mechanisms": r.mechanisms_of_action.iter().map(|m| json!({ "mechanism": m.mechanism, "action": m.action })).collect::<Vec<_>>(),
                "via": via, "sentence": sentence,
                "source": { "id": cite, "url": r.url, "locator": r.record_locator, "sha256": r.sha256,
                            "retrieved_on": r.retrieved_at, "version": r.data_version, "file": d.source["file"] },
                "note": crate::copy_extra::msg("questions.drug_evidence", json!({}))["fallback"],
                "note_msg": crate::copy_extra::msg("questions.drug_evidence", json!({})),
            })
        };
        if let Some(c) = r.condition_id.as_deref()
            && let Some((node, _)) = communities.iter().find(|(n, _)| n.id == c)
        {
            let related = node.id != query;
            let key = if related {
                "questions.drug.indication_related"
            } else {
                "questions.drug.indication"
            };
            let s = msg(
                key,
                json!({ "drug": drug, "stage": stage, "condition": node.label }),
                vec![cite.clone()],
            );
            out.push((
                if related { 0 } else { 1 },
                base("indication", s, json!({ "condition": node })),
            ));
            continue;
        }
        // The mechanism that names the community gene as its target (never another mechanism of the drug).
        let hit = communities.iter().flat_map(|(_, g)| g).find_map(|g| {
            r.mechanisms_of_action
                .iter()
                .find(|m| m.targets.iter().any(|t| t.symbol.as_deref() == Some(g.as_str())))
                .map(|m| (g, m))
        });
        if let Some((gene, moa)) = hit {
            let genes = vec![gene];
            let mech = moa.mechanism.clone().unwrap_or_default();
            let s = msg(
                "questions.drug.target",
                json!({ "drug": drug, "gene": gene, "mechanism": mech, "stage": stage }),
                vec![cite.clone()],
            );
            out.push((2, base("target", s, json!({ "genes": genes }))));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    let mut seen = std::collections::BTreeSet::new();
    out.into_iter()
        .filter(|(_, v)| seen.insert((v["drug"]["id"].to_string(), v["basis"].to_string())))
        .take(limit)
        .map(|x| x.1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::node::NodeKind;

    #[test]
    fn mechanism_is_the_one_targeting_the_gene() {
        let rec: Record = serde_json::from_value(json!({
            "id": "r1", "status": "included", "relation": "clinical_target", "drug_id": "CHEMBL1", "drug_name": "X",
            "association_max_clinical_stage": "PHASE_2",
            "mechanisms_of_action": [
                { "mechanismOfAction": "H1 antagonist", "targets": [{ "approvedSymbol": "HRH1" }] },
                { "mechanismOfAction": "NMDA antagonist", "targets": [{ "approvedSymbol": "GRIN2A" }] }
            ]
        }))
        .unwrap();
        let d = Drugs {
            records: vec![rec],
            source: json!({}),
        };
        let node = NodeRef {
            id: "MONDO:1".into(),
            kind: NodeKind::Disease,
            label: "c".into(),
        };
        let out = for_question(&d, &[(node, vec!["GRIN2A".into()])], "MONDO:1", 5);
        assert_eq!(out.len(), 1);
        assert!(out[0]["sentence"]["text"].as_str().unwrap().contains("NMDA antagonist"));
        assert_eq!(out[0]["edge_kind"], "observed");
    }
}
