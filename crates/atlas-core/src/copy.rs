//! Catalog messages pending web integration in copy-audit deliverable 2.
use serde_json::{Value, json};

fn value(p: &Value, key: &str) -> String {
    match &p[key] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

pub fn msg(key: &str, p: Value) -> Value {
    let fallback = text(key, &p);
    json!({"key": key, "params": p, "fallback": fallback})
}

pub fn text(key: &str, p: &Value) -> String {
    match key {
        "integrity.contract.source-header-checksums-match" => "Every loaded source passes its cache-header checksum checks.".into(),
        "integrity.problem.header_checksums" => format!("{n} cache-header checksum checks failed.", n = value(p, "n")),
        "integrity.contract.identity-merge-has-rule-evidence" => "every SSSOM alias resolves to a merge activity and hash-bound versioned mapping rows".into(),
        "integrity.problem.identity_activity" => "missing identity merge activity".into(),
        "integrity.problem.identity_derivation" => "missing member derivation/mapping rows".into(),
        "integrity.problem.identity_rule" => "unversioned rule or incomplete evidence".into(),
        "integrity.problem.record_index" => format!("record {r} out of range", r = value(p, "r")),
        "integrity.problem.record_entity" => format!("record {arg0} names an unknown entity", arg0 = value(p, "arg0")),
        "integrity.problem.record_checksum" => format!("record {arg0} has no sha256", arg0 = value(p, "arg0")),
        "integrity.problem.endpoint_from" => format!("dangling from {arg0}", arg0 = value(p, "arg0")),
        "integrity.problem.endpoint_to" => format!("dangling to {arg0}", arg0 = value(p, "arg0")),
        "integrity.problem.endpoint_kind" => format!("{arg0} expects {from} → {to}, found {f} → {t}", arg0 = value(p, "arg0"), from = value(p, "from"), to = value(p, "to"), f = value(p, "f"), t = value(p, "t")),
        "integrity.problem.duplicate_edge" => format!("{n} edges share this id", n = value(p, "n")),
        "integrity.problem.entity_licence" => format!("entity {arg0} has no licence entry", arg0 = value(p, "arg0")),
        "integrity.problem.person_orcids" => format!("{arg0} ORCIDs: {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        "integrity.problem.orcid_people" => format!("ORCID on {arg0} person nodes: {arg1}", arg0 = value(p, "arg0"), arg1 = value(p, "arg1")),
        "integrity.problem.different_orcids" => format!("different ORCIDs {a} vs {b}", a = value(p, "a"), b = value(p, "b")),
        "integrity.problem.no_record" => "no source record".into(),
        "integrity.problem.unknown_activity" => "unknown activity".into(),
        "integrity.problem.no_checksum" => "no sha256".into(),
        "integrity.problem.no_access" => "no access route".into(),
        "integrity.problem.self_link" => "self link".into(),
        "integrity.contract.edge-endpoint-resolves" => "every edge's from and to resolve to a node of the kind its relation allows (no dangling edges)".into(),
        "integrity.contract.edge-has-provenance" => "every edge has ≥1 source record (entity + locator + sha256) and a generating activity".into(),
        "integrity.contract.node-has-provenance" => "every study, grant, paper, person, organisation and asset has ≥1 source record with a sha256".into(),
        "integrity.contract.edge-id-unique" => "edge ids (from|relation|to) are unique; parallel evidence is merged into one edge".into(),
        "integrity.contract.record-entity-has-checksum" => "every source file behind a record has a recorded sha256".into(),
        "integrity.contract.record-has-licence" => "every source file behind a record states a licence with a class (open / share_alike / non_commercial / unknown)".into(),
        "integrity.contract.asset-has-access-route" => "every asset says who holds it or how to get it (an access route with a URL or a holder)".into(),
        "integrity.contract.candidate-not-merged" => "a candidate_same_as pair never shares one node (candidates are links, not merges; D30)".into(),
        _ => panic!("unknown copy key: {key}"),
    }
}
