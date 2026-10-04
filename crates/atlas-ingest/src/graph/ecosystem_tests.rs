use super::{
    cache, ecosystem,
    fixtures::{self, TempData},
};
use atlas_core::withhold::{KeyKind, Salt, Withhold};
use atlas_core::{Graph, snapshot};
use serde_json::{Value, json};

fn fixture(data: &TempData) -> (Vec<Value>, Vec<Value>) {
    let body = b"Fixture official page";
    let sha = cache::hex(&cache::sha256(body));
    data.write(&format!("cache/ecosystem/responses/{sha}.bin"), body);
    let proof = json!({"source_url":"https://example.org/", "sha256":sha,
        "sha256_basis":"original HTTP response bytes", "retrieved_at":"2026-10-04T00:00:00Z",
        "version":"retrieval:2026-10-04", "record_locator":"html:a[0]@href"});
    let route = json!({"action":"Participate", "url":"https://example.org/join", "audience":"families",
        "outcome":"Check participation steps", "official_route_verified":true, "destination_status":"ok",
        "retrieved_at":"2026-10-04T00:00:00Z", "sha256":sha, "prov:wasDerivedFrom":[proof]});
    let nodes = vec![
        json!({"id":"ecosystem:test", "name":"Test initiative", "description":"Connects families.",
        "url":"https://example.org/", "source_url":"https://example.org/", "status":"verified", "excluded":false,
        "retrieved_at":"2026-10-04T00:00:00Z", "languages":["en"],
        "scope":[{"type":"gene", "label":"STXBP1", "target":fixtures::GENE}],
        "action_routes":[route], "prov:wasDerivedFrom":[proof]}),
    ];
    let edges = vec![
        json!({"id":"ecosystem:test|serves_gene|HGNC:11444", "subject":"ecosystem:test",
        "object":fixtures::GENE, "relation":"serves_gene", "source_url":"https://example.org/",
        "retrieved_at":"2026-10-04T00:00:00Z", "prov:wasDerivedFrom":[proof]}),
    ];
    (nodes, edges)
}

fn write(data: &TempData, nodes: &[Value], edges: &[Value]) {
    data.envelope(ecosystem::INITIATIVES, "ecosystem.initiatives", json!(nodes));
    data.envelope(ecosystem::EDGES, "ecosystem.edges", json!(edges));
}

#[test]
fn ecosystem_nodes_actions_exact_links_and_snapshot_roundtrip() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (nodes, edges) = fixture(&data);
    write(&data, &nodes, &edges);
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &Withhold::empty(Salt::new("test"))).unwrap();
    assert_eq!(b.data.orgs[0].description.as_deref(), Some("Connects families."));
    assert_eq!(b.data.initiatives[0].actions[0].url, "https://example.org/join");
    assert_eq!(b.data.edges[0].to, fixtures::GENE);
    let g = Graph::new(b.data.clone());
    assert!(g.node("ecosystem:test").is_some());
    for rec in &g.data().records {
        let check = cache::rehash(
            &data.path().join(&g.provenance().entity(rec.entity).file),
            &rec.locator,
            rec.hash,
        );
        assert_eq!(check.computed_sha256, Some(cache::hex(&rec.sha256)));
    }
    let path = data.path().join("graph.snapshot");
    snapshot::save_graph(&path, &b.data, "ecosystem").unwrap();
    let (roundtrip, _) = snapshot::load_graph(&path).unwrap();
    assert_eq!(roundtrip.data(), &b.data);
}

#[test]
fn unavailable_actions_and_excluded_nodes_never_become_active() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (mut nodes, edges) = fixture(&data);
    nodes[0]["action_routes"][0]["destination_status"] = json!("blocked");
    let mut closed = nodes[0]["action_routes"][0].clone();
    closed["destination_status"] = json!("ok");
    closed["availability"] = json!("closed");
    nodes[0]["action_routes"].as_array_mut().unwrap().push(closed);
    write(&data, &nodes, &edges);
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &Withhold::empty(Salt::new("test"))).unwrap();
    assert!(b.data.initiatives[0].actions.is_empty());
    nodes[0]["excluded"] = json!(true);
    write(&data, &nodes, &edges);
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &Withhold::empty(Salt::new("test"))).unwrap();
    assert!(b.data.orgs[0].description.is_none());
    assert!(b.data.edges.is_empty());
    assert!(!b.data.initiatives[0].verified);
    assert!(!b.data.records.is_empty(), "the exclusion remains auditable");
}

#[test]
fn checksum_and_original_byte_tampering_fail_closed() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (nodes, edges) = fixture(&data);
    write(&data, &nodes, &edges);
    let path = data.path().join(ecosystem::INITIATIVES);
    let mut env: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    env["records"][0]["name"] = json!("tampered");
    data.write(ecosystem::INITIATIVES, env.to_string().as_bytes());
    assert!(
        ecosystem::ingest(
            &mut fixtures::builder(&atlas),
            data.path(),
            &Withhold::empty(Salt::new("test"))
        )
        .is_err()
    );
    write(&data, &nodes, &edges);
    let sha = nodes[0]["prov:wasDerivedFrom"][0]["sha256"].as_str().unwrap();
    data.write(&format!("cache/ecosystem/responses/{sha}.bin"), b"tampered");
    assert!(
        ecosystem::ingest(
            &mut fixtures::builder(&atlas),
            data.path(),
            &Withhold::empty(Salt::new("test"))
        )
        .is_err()
    );
}

#[test]
fn broad_scope_cannot_be_used_to_assert_a_gene_edge() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (mut nodes, edges) = fixture(&data);
    nodes[0]["scope"][0]["type"] = json!("all_rare_diseases");
    write(&data, &nodes, &edges);
    assert!(
        ecosystem::ingest(
            &mut fixtures::builder(&atlas),
            data.path(),
            &Withhold::empty(Salt::new("test"))
        )
        .is_err()
    );
}

#[test]
fn suppressing_an_initiative_prevents_its_reingestion() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (nodes, edges) = fixture(&data);
    write(&data, &nodes, &edges);
    let salt = Salt::new("test");
    let list = json!({"schema":"suppression", "version":1, "entries":[{"id":"sup_test",
        "keys":[salt.key(KeyKind::Node,"ecosystem:test").unwrap()], "salt_id":salt.id(),
        "scope":"all", "date":"2026-10-04T00:00:00Z", "reason":"test", "reviewer":"test"}]});
    let bytes = serde_json::to_vec(&list).unwrap();
    let filter = Withhold::from_lists(salt, Some(&bytes), None).unwrap();
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &filter).unwrap();
    assert!(b.data.orgs.is_empty());
    assert!(b.data.initiatives.is_empty());
    assert!(b.data.edges.is_empty());
}

#[test]
fn invented_exact_scope_is_rejected_and_missing_cache_is_optional() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &Withhold::empty(Salt::new("test"))).unwrap();
    assert_eq!(b.data.coverage[0].status, "absent");
    let (nodes, mut edges) = fixture(&data);
    edges[0]["object"] = json!("HGNC:11132");
    write(&data, &nodes, &edges);
    assert!(
        ecosystem::ingest(
            &mut fixtures::builder(&atlas),
            data.path(),
            &Withhold::empty(Salt::new("test"))
        )
        .is_err()
    );
}

#[test]
fn provided_context_is_hashed_but_never_becomes_a_verified_action() {
    let data = TempData::new();
    let atlas = fixtures::atlas();
    let (mut nodes, edges) = fixture(&data);
    let bytes = b"Provided task brief relaying a partner reply";
    let sha = cache::hex(&cache::sha256(bytes));
    data.write(&format!("cache/ecosystem/responses/{sha}.bin"), bytes);
    let context = json!({"source_url":"file:///provided-brief.md","sha256":sha,
        "sha256_basis":"provided context document bytes; original email not supplied",
        "retrieved_at":"2026-10-04T00:00:00Z","version":"provided-brief-sha256",
        "record_locator":"markdown:partner-update"});
    nodes[0]["excluded"] = json!(true);
    nodes[0]["status"] = json!("unverified");
    nodes[0]["exclusion_reason"] = json!("direct HTTP 403");
    nodes[0]["reported_action_routes"] = json!([{"url":"https://example.org/reported",
        "action":"Request updates","destination_status":"blocked",
        "verification":"founder-reported; original email not supplied","prov:wasDerivedFrom":[context]}]);
    write(&data, &nodes, &edges);
    let mut b = fixtures::builder(&atlas);
    ecosystem::ingest(&mut b, data.path(), &Withhold::empty(Salt::new("test"))).unwrap();
    assert!(b.data.initiatives[0].actions.is_empty());
    assert!(!b.data.initiatives[0].verified);
    assert_eq!(b.data.initiatives[0].reported_actions[0].sha256, sha);
    assert!(b.data.edges.is_empty());
    data.write(&format!("cache/ecosystem/responses/{sha}.bin"), b"changed brief");
    assert!(
        ecosystem::ingest(
            &mut fixtures::builder(&atlas),
            data.path(),
            &Withhold::empty(Salt::new("test"))
        )
        .is_err()
    );
}
