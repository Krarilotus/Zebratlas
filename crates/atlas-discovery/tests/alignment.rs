use atlas_core::node::NodeKind;
use atlas_core::provenance::{EntityIdx, RecordRef};
use atlas_discovery::*;
use serde_json::{Value, json};

fn evidence() -> RecordRef {
    RecordRef::record(EntityIdx(0), "/synthetic-test")
}
fn target(id: &str, label: &str) -> Target {
    Target {
        id: id.into(),
        label: label.into(),
        kind: NodeKind::Gene,
        active: true,
        evidence: evidence(),
    }
}
fn mention(ids: &[&str], label: &str) -> Mention {
    Mention {
        id: "urn:test:mention".into(),
        label: label.into(),
        kind: NodeKind::Gene,
        explicit_ids: ids.iter().map(|s| (*s).into()).collect(),
        evidence: evidence(),
    }
}

#[test]
fn explicit_identifier_normalizes_through_core() {
    let a = align(mention(&[" hgnc:1 "], "TEST_A"), &[target("HGNC:1", "TEST_A")]);
    assert_eq!(a.decision, Decision::Exact);
}
#[test]
fn label_only_never_merges_even_if_unique() {
    let a = align(mention(&[], "TEST_A"), &[target("HGNC:1", "TEST_A")]);
    assert_eq!(a.decision, Decision::Review);
    assert_eq!(a.targets, ["HGNC:1"]);
}
#[test]
fn conflicting_unknown_retired_and_wrong_kind_ids_veto_alignment() {
    let ts = [target("HGNC:1", "TEST_A"), target("HGNC:2", "TEST_B")];
    for ids in [vec!["HGNC:1", "HGNC:2"], vec!["HGNC:1", "HGNC:999"], vec!["malformed"]] {
        assert_eq!(align(mention(&ids, "TEST_A"), &ts).decision, Decision::Review);
    }
    let mut t = ts[0].clone();
    t.active = false;
    assert_eq!(
        align(mention(&["HGNC:1"], "TEST_A"), &[t.clone()]).decision,
        Decision::Review
    );
    t.active = true;
    t.kind = NodeKind::Disease;
    assert_eq!(align(mention(&["HGNC:1"], "TEST_A"), &[t]).decision, Decision::Review);
}
#[test]
fn stale_or_ambiguous_names_need_review() {
    assert_eq!(
        align(mention(&["HGNC:1"], "OLD_SYMBOL"), &[target("HGNC:1", "TEST_A")]).decision,
        Decision::Review
    );
    let a = align(
        mention(&[], "TEST_A"),
        &[target("HGNC:1", "TEST_A"), target("HGNC:2", "TEST_A")],
    );
    assert_eq!(a.decision, Decision::Review);
    assert_eq!(a.targets.len(), 2);
}

fn fixture() -> (Manifest, Vec<Value>) {
    // Entirely synthetic, deliberately unlike real HGNC records; no accuracy claim uses it.
    let sources: Vec<Source> = ["panelapp-catalog", "panelapp", "uniprot", "hgnc-TEST_A"]
        .iter()
        .map(|id| Source {
            id: (*id).into(),
            url: format!("https://example.org/{id}"),
            file: format!("{id}.json"),
            version: "synthetic-v1".into(),
            retrieved_at: "2026-10-03T00:00:00Z".into(),
            sha256: "0".repeat(64),
            bytes: 0,
            licence: None,
            headers: Default::default(),
            elapsed_seconds: 0.0,
        })
        .collect();
    (
        Manifest {
            schema: "atlas.discovery.sources".into(),
            version: 1,
            seeds: vec!["TEST_A".into()],
            sources,
            discovery: json!({"panel_id":1,"query":"Test panel"}),
        },
        vec![
            json!({"results":[{"id":1,"name":"Test panel","relevant_disorders":[]}],"next":null}),
            json!({"id":1,"name":"Test panel","genes":[
            {"gene_data":{"gene_symbol":"TEST_A","hgnc_id":"HGNC:1"},"phenotypes":["ambiguous symptom"]},
            {"gene_data":{"gene_symbol":"OUT_OF_SCOPE","hgnc_id":"HGNC:2"},"phenotypes":[]}],
            "strs":[{"entity_name":"synthetic repeat"}],"regions":[]}),
            json!({"results":[{"primaryAccession":"SYNTHETIC", "organism":{"taxonId":9606},
            "genes":[{"geneName":{"value":"TEST_A"}}],
            "proteinDescription":{"recommendedName":{"fullName":{"value":"Synthetic protein"}}},
            "uniProtKBCrossReferences":[{"database":"HGNC","id":"HGNC:1"}]}]}),
            json!({"response":{"docs":[{"hgnc_id":"HGNC:1","symbol":"TEST_A","status":"Approved"}]}}),
        ],
    )
}

#[test]
fn adapters_keep_exclusions_and_never_merge_protein_or_panel_into_gene() {
    let (m, data) = fixture();
    let b = build(m, data, "synthetic".into()).unwrap();
    assert_eq!(b.records.len(), 4);
    assert_eq!(b.metrics["panelapp.excluded"], 2);
    assert_eq!(b.mappings.len(), 2);
    assert_eq!(b.edges.len(), 2);
    assert!(b.mappings.iter().all(|m| m.subject_id.ends_with("gene-mention")));
    assert!(b.review_queue.iter().any(|r| r.reason.contains("licence")));
    let graph = prov_jsonld(&b);
    assert!(
        graph["@graph"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["atlas:status"] == "excluded")
    );
    validate(&b).unwrap();
    let mut broken = b.clone();
    broken.edges[0].to = "missing".into();
    assert!(validate(&broken).is_err());
}

#[test]
fn nonhuman_protein_and_schema_drift_are_not_silently_accepted() {
    let (m, mut data) = fixture();
    data[2]["results"][0]["organism"]["taxonId"] = json!(10090);
    let b = build(m.clone(), data.clone(), "synthetic".into()).unwrap();
    assert_eq!(b.metrics["uniprot.automatic"], 0);
    data[1].as_object_mut().unwrap().remove("genes");
    assert!(build(m, data, "synthetic".into()).is_err());
}

#[test]
fn review_is_bound_to_bundle_and_cannot_approve_identity_merge() {
    let (m, data) = fixture();
    let b = build(m, data, "synthetic".into()).unwrap();
    let bytes = serde_json::to_vec(&b).unwrap();
    let mut d = ReviewDecision {
        review_id: b.review_queue[0].id.clone(),
        reviewer: "synthetic reviewer".into(),
        reviewed_at: "2026-10-03T00:00:00Z".into(),
        disposition: "approve_typed_link".into(),
        rationale: "synthetic test rationale".into(),
        evidence_url: "https://example.org/review".into(),
        bundle_sha256: sha256(&bytes),
    };
    validate_review(&b, &d, &bytes).unwrap();
    d.disposition = "approve_exact_merge".into();
    assert!(validate_review(&b, &d, &bytes).is_err());
    d.disposition = "reject".into();
    assert!(validate_review(&b, &d, b"stale").is_err());
}

#[test]
fn raw_integrity_schema_and_cache_boundary_are_enforced() {
    let dir = std::env::temp_dir().join(format!("atlas-discovery-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (mut m, _) = fixture();
    m.sources.truncate(1);
    let raw = b"{}";
    m.sources[0].sha256 = sha256(raw);
    m.sources[0].bytes = 2;
    std::fs::write(dir.join(&m.sources[0].file), raw).unwrap();
    let path = dir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    load(&path).unwrap();
    std::fs::write(dir.join(&m.sources[0].file), b"[]").unwrap();
    assert!(load(&path).unwrap_err().to_string().contains("integrity"));
    m.version = 99;
    std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(load(&path).unwrap_err().to_string().contains("schema"));
    let original_file = m.sources[0].file.clone();
    m.version = 1;
    m.sources[0].file = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(load(&path).unwrap_err().to_string().contains("escapes cache"));
    // Do not recursively remove a computed path on the shared Windows machine.
    std::fs::remove_file(dir.join(original_file)).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
