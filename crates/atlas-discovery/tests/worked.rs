use atlas_discovery::{
    Bundle, build,
    evaluation::{Audit, evaluate},
    load, prov_jsonld, validate,
};
use std::collections::BTreeSet;
use std::path::Path;

fn worked() -> Bundle {
    serde_json::from_str(include_str!("../examples/worked/bundle.json")).unwrap()
}

#[test]
fn checked_in_examples_and_audit_are_consistent_offline() {
    let b = worked();
    validate(&b).unwrap();
    assert_eq!(b.records.len(), 948);
    assert_eq!(b.mappings.len(), 19);
    assert_eq!(b.nodes.len(), 21);
    assert_eq!(b.edges.len(), 19);
    assert_eq!(b.review_queue.len(), 20);
    let plan = atlas_discovery::ingest_plan(&b).unwrap();
    assert_eq!(plan["edges"].as_array().unwrap().len(), 10);
    assert_eq!(plan["nodes"].as_array().unwrap().len(), 20);
    assert_eq!(plan["blocked_sources"][0]["source"], "panelapp");
    let bytes = include_bytes!("../examples/audit.json");
    let a: Audit = serde_json::from_slice(bytes).unwrap();
    let v = evaluate(&b, &a, bytes).unwrap();
    assert_eq!(v["results"][0]["correct"], 9);
    assert_eq!(v["results"][1]["correct"], 10);
    assert_eq!(
        v,
        serde_json::from_str::<serde_json::Value>(include_str!("../examples/worked/evaluation.json")).unwrap()
    );
    // Verify actual RDF references resolve within the generated provenance graph.
    let graph = prov_jsonld(&b);
    let nodes = graph["@graph"].as_array().unwrap();
    let ids: BTreeSet<_> = nodes.iter().map(|n| n["@id"].as_str().unwrap()).collect();
    for n in nodes {
        for predicate in [
            "prov:wasDerivedFrom",
            "prov:wasGeneratedBy",
            "prov:used",
            "prov:specializationOf",
            "prov:wasAssociatedWith",
        ] {
            if let Some(v) = n.get(predicate) {
                let refs = if let Some(xs) = v.as_array() {
                    xs.clone()
                } else {
                    vec![v.clone()]
                };
                for r in refs {
                    assert!(ids.contains(r["@id"].as_str().unwrap()), "dangling {predicate}: {r}");
                }
            }
        }
    }
    let mut bad = a;
    bad.rows[0].expected_hgnc = "HGNC:INVALID_EXPECTATION_FOR_TEST".into();
    assert_eq!(
        evaluate(&b, &bad, b"mutated test audit").unwrap()["results"][0]["correct"],
        8
    );
    bad.source_hashes.insert("panelapp".into(), "changed snapshot".into());
    assert!(evaluate(&b, &bad, b"mutated test audit").is_err());
}

#[test]
#[ignore = "requires RARE_ATLAS_DATA and the original content-addressed discovery cache"]
fn raw_worked_snapshots_rebuild_and_all_locators_resolve() {
    let root = std::env::var("RARE_ATLAS_DATA").expect("set RARE_ATLAS_DATA");
    let path = Path::new(&root).join(format!("cache/discovery/manifest-{}.json", worked().manifest_sha256));
    let (m, data, hash) = load(&path).unwrap();
    assert_eq!(
        hash,
        worked().manifest_sha256,
        "the measured snapshot changed; do not silently update the gold audit"
    );
    let b = build(m, data.clone(), hash).unwrap();
    for r in b
        .records
        .iter()
        .map(|r| &r.locator)
        .chain(b.targets.iter().map(|t| &t.evidence))
        .chain(b.review_queue.iter().map(|r| &r.evidence))
    {
        assert!(
            data[r.entity.0 as usize].pointer(&r.locator.to_string()).is_some(),
            "missing locator {r:?}"
        );
    }
    assert_eq!(
        serde_json::to_value(b).unwrap(),
        serde_json::to_value(worked()).unwrap()
    );
}
