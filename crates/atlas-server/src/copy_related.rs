//! Language adapters for analytics reports. Scores stay data; reader explanations use keys.
use crate::copy_extra::msg;
use atlas_analytics::{Counterexample, CounterexampleKind};
use serde_json::{Value, json};

pub fn counter_message(c: &Counterexample) -> Value {
    let kind = match c.kind {
        CounterexampleKind::SameGeneDifferentMechanism => "same_gene",
        CounterexampleKind::MechanismSplitWithinCondition => "within_condition",
        CounterexampleKind::LookalikeDifferentMechanism => "lookalike",
    };
    msg(&format!("related.counter.{kind}"), json!({"gene": c.gene}))
}

/// Rebuild explanations from structured facts, without parsing or translating source names.
pub fn decorate(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(decorate),
        Value::Object(o) => {
            if o.contains_key("mechanism") && o.contains_key("phenotype") && o.contains_key("neighbour") {
                let me = &o["mechanism"];
                let ph = &o["phenotype"];
                let mut why = Vec::new();
                if let Some(a) = me["shared_genes"].as_array() {
                    for g in a {
                        why.push(msg(
                            "related.shared_gene",
                            json!({"gene": g["symbol"], "here": g["effects_a"], "there": g["effects_b"]}),
                        ));
                    }
                }
                if me["process_score"].as_f64().unwrap_or(0.0) > 0.0 {
                    let names: Vec<_> = me["shared_processes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(3)
                        .map(|p| p["name"].clone())
                        .collect();
                    if !names.is_empty() {
                        why.push(msg("related.shared_processes", json!({"processes": names})));
                    }
                }
                let names: Vec<_> = ph["shared"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(3)
                    .map(|p| p["term"]["label"].clone())
                    .collect();
                if !names.is_empty() {
                    why.push(msg("related.shared_symptoms", json!({"symptoms": names})));
                }
                if me["known"] == false {
                    why.push(msg("related.cause_unknown", json!({})));
                }
                if let Some(a) = me["effect_conflicts"].as_array() {
                    why.extend(a.iter().map(|g| msg("related.effect_conflict", json!({"gene": g["gene"]["symbol"], "here": g["gene"]["effects_a"], "there": g["gene"]["effects_b"]}))));
                }
                o.insert(
                    "why".into(),
                    json!(why.iter().map(|m| m["fallback"].clone()).collect::<Vec<_>>()),
                );
                o.insert("why_msg".into(), json!(why));
            }
            if o.contains_key("effects_a") && o.contains_key("effects_b") && o.contains_key("symbol") {
                // Effect labels are source vocabulary, retained as data in the enclosing message.
            } else if o.contains_key("opposite") && o.contains_key("gene") && o.contains_key("why") {
                let m = msg(
                    "related.effect_conflict",
                    json!({"gene": o["gene"]["symbol"], "here": o["gene"]["effects_a"], "there": o["gene"]["effects_b"]}),
                );
                o.insert("why".into(), m["fallback"].clone());
                o.insert("why_msg".into(), m);
            } else if o.contains_key("kind") && o.contains_key("records") && o.contains_key("why") {
                let kind = match o["kind"].as_str() {
                    Some("same_gene_different_mechanism") => "same_gene",
                    Some("mechanism_split_within_condition") => "within_condition",
                    _ => "lookalike",
                };
                let m = msg(&format!("related.counter.{kind}"), json!({"gene": o["gene"]}));
                o.insert("why".into(), m["fallback"].clone());
                o.insert("why_msg".into(), m);
            }
            for (key, value) in o.iter_mut() {
                if !key.ends_with("_msg") {
                    decorate(value);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_use_structured_evidence_and_do_not_put_scores_in_prose() {
        let mut report = json!({"items": [{"neighbour": {"id": "MONDO:x", "label": "Example"},
            "why": ["old score prose"], "score": 0.42,
            "phenotype": {"shared": [{"term": {"label": "Seizures"}}]},
            "mechanism": {"known": true, "process_score": 0.42,
                "shared_genes": [{"symbol": "STXBP1", "effects_a": ["loss_of_function"], "effects_b": ["loss_of_function"]}],
                "shared_processes": [{"name": "synaptic vesicle fusion"}], "effect_conflicts": []}}]});
        decorate(&mut report);
        assert_eq!(report["items"][0]["score"], 0.42);
        assert_eq!(report["items"][0]["why_msg"][0]["params"]["gene"], "STXBP1");
        assert!(!report["items"][0]["why"].to_string().contains("0.42"));
        assert!(crate::language_contract::offenders(&report).is_empty());
    }
}
