//! KGX + SSSOM over small fixtures: Biolink categories map to atlas kinds, exact SSSOM rows merge
//! ids, other rows stay candidates, the two-hop scope holds, licence classes travel per record.

use atlas_core::graph::{AssetKind, Graph, LicenceClass, Relation};
use atlas_core::integrity;
use serde_json::json;

use super::fixtures::{self, CONDITION, GENE, TempData};
use super::{kgx, sssom};

/// Upgrade the old synthetic fixtures to the explicit rule/evidence contract.
fn fixture_rules(d: &TempData, file: &str) {
    let path = d.path().join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if line.starts_with("subject_id") {
            out.push_str(&format!(
                "{line}\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\n"
            ));
        } else {
            let rule = if line.starts_with("HGNC:") {
                "R-GEN-01"
            } else if line.starts_with("NCT:") {
                "R-TRI-01"
            } else if line.starts_with("atlasorg:") {
                "R-ORG-01"
            } else {
                "R-DIS-01"
            };
            out.push_str(&format!(
                "{line}\t{rule}\t1.0.0\thttps://example.org/source\t{}\tfixture record\n",
                "a".repeat(64)
            ));
        }
    }
    d.write(file, out.as_bytes());
    fixtures::gate(d);
}

fn edge<'g>(g: &'g Graph, from: &str, rel: Relation, to: &str) -> Option<&'g atlas_core::graph::GraphEdge> {
    g.edges()
        .iter()
        .find(|e| e.from == from && e.relation == rel && e.to == to)
}

fn lic(class: &str) -> serde_json::Value {
    let l = match class {
        "open" => "CC-BY-4.0",
        "share_alike" => "CC-BY-SA-3.0",
        _ => "unknown",
    };
    json!({"license": l, "license_class": class})
}

fn with(mut v: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

#[test]
fn access_routes_null_site_status_and_release_opt_out_survive_snapshot() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    let context = json!({"access_routes": [
        {"route_type":"study_coordinator", "request_url":"https://example.org/starr", "permitted_use":"ask about consent"},
        {"route_type":"study_coordinator", "request_url":"https://example.org/esco", "permitted_use":"ask about population fit"}],
        "overall_status":"RECRUITING", "site_status":null, "availability":"unknown",
        "components":[{"population":"synthetic population"}], "rights":{"data_reuse":"not assessed"}});
    d.jsonl(
        "cache/kgx/access/nodes.jsonl",
        &[with(
            with(
                json!({"id":"access:fixture", "name":"Synthetic access fixture",
        "category":["biolink:Dataset"], "release":false}),
                lic("open"),
            ),
            context.clone(),
        )],
    );
    d.jsonl(
        "cache/kgx/access/edges.jsonl",
        &[with(
            json!({"subject":"access:fixture", "predicate":"biolink:related_to", "object":GENE}),
            lic("open"),
        )],
    );
    let mut b = fixtures::builder(&atlas);
    let ident = sssom::identity(&b, d.path()).unwrap();
    kgx::ingest(&mut b, d.path(), &ident, kgx::Scope::default()).unwrap();
    let path = d.path().join("access.snapshot");
    atlas_core::snapshot::save_graph(&path, &b.data, "synthetic access fixture").unwrap();
    let (g, _) = atlas_core::snapshot::load_graph(&path).unwrap();
    let a = g.asset(g.node("access:fixture").unwrap().idx);
    assert_eq!(a.access_context(), context);
    assert_eq!(a.access.url.as_deref(), Some("https://example.org/starr"));
    assert!(
        !a.release,
        "an open page does not override an explicit bulk-release opt-out"
    );
}

#[test]
fn kgx_sssom_scope_identity_licence() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    let open = lic("open");
    d.jsonl(
        "cache/kgx/assets/nodes.jsonl",
        &[
            with(json!({"id": "CVCL:0001", "category": ["biolink:CellLine"], "name": "iPSC STXBP1 line",
                        "access_routes": [{"holder": "EBiSC", "request_url": "https://ebisc.example/1", "route_type": "supplier_catalogue"}],
                        "primary_knowledge_source": "infores:cellosaurus"}), open.clone()),
            with(json!({"id": "ZFIN:G1", "category": ["biolink:Gene"], "name": "stxbp1a"}), open.clone()),
            with(json!({"id": "ENSEMBL:ENSG00000143621", "category": ["biolink:Gene"], "name": "synthetic alias source"}), open.clone()),
            with(json!({"id": "ZFIN:FISH1", "category": ["biolink:Genotype"], "name": "stxbp1a mutant"}), open.clone()),
            with(json!({"id": "CHEMBL.COMPOUND:C1", "category": ["biolink:Drug"], "name": "Drug C1"}), lic("share_alike")),
            with(json!({"id": "FAR:1", "category": ["biolink:Drug"], "name": "far away"}), open.clone()),
            with(json!({"id": "ORCID:0000-0001", "category": ["biolink:Person"], "name": "A Person"}), open.clone()),
            with(json!({"id": "bb:1", "category": ["biolink:Biospecimen"], "name": "withheld", "excluded": true}), open.clone()),
        ],
    );
    d.jsonl(
        "cache/kgx/assets/edges.jsonl",
        &[
            // hop 1: cell line -> gene (via an ENSEMBL alias merged by SSSOM)
            with(
                json!({"subject": "CVCL:0001", "predicate": "biolink:related_to", "object": "ENSEMBL:ENSG00000143621",
                        "knowledge_level": "knowledge_assertion", "agent_type": "manual_agent"}),
                open.clone(),
            ),
            // hop 1: ortholog
            with(
                json!({"subject": "ZFIN:G1", "predicate": "biolink:orthologous_to", "object": GENE,
                        "knowledge_level": "prediction"}),
                open.clone(),
            ),
            // hop 2: genotype of the ortholog
            with(
                json!({"subject": "ZFIN:FISH1", "predicate": "biolink:related_to", "object": "ZFIN:G1"}),
                open.clone(),
            ),
            // hop 1: drug studied for the condition (DOID alias merged by SSSOM)
            with(
                json!({"subject": "CHEMBL.COMPOUND:C1", "predicate": "biolink:related_to", "object": "DOID:1"}),
                lic("share_alike"),
            ),
            // out of scope: far from any seed
            with(
                json!({"subject": "FAR:1", "predicate": "biolink:related_to", "object": "FAR:2"}),
                open.clone(),
            ),
            // person edges are not ingested as people
            with(
                json!({"subject": "ORCID:0000-0001", "predicate": "biolink:related_to", "object": GENE}),
                open.clone(),
            ),
            with(
                json!({"subject": "bb:1", "predicate": "biolink:related_to", "object": GENE}),
                open,
            ),
        ],
    );
    d.write(
        "cache/mappings/test.sssom.tsv",
        b"# license: CC0-1.0\n\
subject_id\tpredicate_id\tobject_id\tmapping_justification\tconfidence\tconflict\n\
HGNC:11444\tskos:exactMatch\tENSEMBL:ENSG00000143621\tsemapv:DatabaseCrossReference\t1.0\t\n\
MONDO:0012812\tskos:exactMatch\tDOID:1\tsemapv:DatabaseCrossReference\t1.0\t\n\
MONDO:0012812\tskos:exactMatch\tDOID:2\tsemapv:DatabaseCrossReference\t1.0\tone-to-many\n\
CVCL:0001\tskos:closeMatch\tZFIN:FISH1\tsemapv:LexicalMatching\t0.8\t\n\
CVCL:0001\tskos:exactMatch\tCHEMBL.COMPOUND:C1\tsemapv:CompositeMatching\t0.95\t\n",
    );
    let mut b = fixtures::builder(&atlas);
    fixture_rules(&d, "cache/mappings/test.sssom.tsv");
    let ident = sssom::identity(&b, d.path()).unwrap();
    assert_eq!(ident.canonical("ENSEMBL:ENSG00000143621"), GENE);
    assert_eq!(ident.canonical("DOID:1"), CONDITION);
    assert_eq!(
        ident.canonical("DOID:2"),
        "DOID:2",
        "a conflict-flagged row never merges"
    );
    kgx::ingest(&mut b, d.path(), &ident, kgx::Scope::default()).unwrap();
    sssom::finish(&mut b, d.path(), &ident).unwrap();
    super::licences(&mut b);
    let g = Graph::new(b.data);

    let cell = g.asset(g.node("CVCL:0001").unwrap().idx);
    assert_eq!(cell.kind, AssetKind::CellLine);
    assert_eq!(cell.holder_name.as_deref(), Some("EBiSC"));
    assert_eq!(cell.access.route, "repository_order");
    assert!(
        edge(&g, "CVCL:0001", Relation::ModelOf, GENE).is_some(),
        "alias resolved, typed by kinds"
    );
    assert!(edge(&g, "ZFIN:G1", Relation::OrthologousTo, GENE).is_some());
    assert!(g.node("ZFIN:FISH1").is_some(), "hop 2 kept");
    let drug = g.node("CHEMBL.COMPOUND:C1").unwrap();
    assert_eq!(g.asset(drug.idx).kind, AssetKind::Drug);
    let studied = edge(&g, "CHEMBL.COMPOUND:C1", Relation::StudiedFor, CONDITION).unwrap();
    assert_eq!(g.licence_class(&studied.records), LicenceClass::ShareAlike);
    assert!(!g.asset(drug.idx).release, "share-alike: ids and links only");
    assert!(g.node("FAR:1").is_none(), "out of scope");
    assert!(g.node("ORCID:0000-0001").is_none(), "no people from KGX (D36)");
    assert!(g.node("bb:1").is_none(), "excluded records stay out");
    // candidates: closeMatch and a non-qualifying exactMatch, never merged
    assert!(edge(&g, "CVCL:0001", Relation::CandidateSameAs, "ZFIN:FISH1").is_some());
    assert!(edge(&g, "CVCL:0001", Relation::CandidateSameAs, "CHEMBL.COMPOUND:C1").is_some());
    assert_eq!(g.canonical_id("ENSEMBL:ENSG00000143621"), GENE);
    let member = g
        .identity_merge(GENE)
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == "ENSEMBL:ENSG00000143621")
        .unwrap();
    assert!(
        member
            .derived_from
            .iter()
            .any(|&r| g.record(r).id == "ENSEMBL:ENSG00000143621"
                && g.provenance().entity(g.record(r).entity).file == "cache/kgx/assets/nodes.jsonl"),
        "the original alias source record survives canonicalisation"
    );
    let cov = g.coverage_of("kgx:assets").unwrap();
    assert!(cov.excluded >= 2, "out-of-scope and excluded counted: {}", cov.excluded);
    let report = integrity::check(&atlas, &g);
    assert!(report.passed, "{:?}", report.violations);
    assert_eq!(report.identity.exact_merges, 2);

    // one-hop scope drops the genotype
    let mut b = fixtures::builder(&atlas);
    let scope = kgx::Scope {
        hops: 1,
        ..kgx::Scope::default()
    };
    kgx::ingest(&mut b, d.path(), &ident, scope).unwrap();
    assert!(b.node("ZFIN:FISH1").is_none());
}

#[test]
fn trial_registrations_merge_onto_the_study_but_flagged_conflicts_do_not() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    d.write(
        "cache/mappings/trial-xrefs.sssom.tsv",
        b"subject_id\tpredicate_id\tobject_id\tmapping_justification\tconfidence\tconflict\n\
NCT:NCT00000001\tskos:exactMatch\tEUDRACT:2004-000066-13\tsemapv:DatabaseCrossReference\t1.0\t\n\
NCT:NCT00000002\tskos:closeMatch\tEUDRACT:2020-002729-27\tsemapv:DatabaseCrossReference\t1.0\tone_to_many\n\
atlasorg:x\tskos:exactMatch\tROR:018hh3649\tsemapv:CompositeMatching\t0.95\t\n",
    );
    let mut b = fixtures::builder(&atlas);
    for (i, nct) in ["NCT00000001", "NCT00000002"].iter().enumerate() {
        b.register(nct, atlas_core::node::NodeKind::Study, i);
    }
    fixture_rules(&d, "cache/mappings/trial-xrefs.sssom.tsv");
    let ident = sssom::identity(&b, d.path()).unwrap();
    assert_eq!(ident.canonical("EUDRACT:2004-000066-13"), "NCT00000001");
    assert_eq!(ident.canonical("EUDRACT:2020-002729-27"), "EUDRACT:2020-002729-27");
    assert_eq!(
        ident.canonical("ROR:018hh3649"),
        "ROR:018hh3649",
        "composite organisation matches are candidates, never source-asserted identity"
    );
}

#[test]
fn original_edge_ids_and_source_metadata_reverify_after_canonical_linking() {
    let atlas = fixtures::atlas();
    let d = TempData::new();
    d.jsonl("cache/kgx/proof/nodes.jsonl", &[with(json!({"id":"CHEMBL.COMPOUND:proof", "category":["biolink:Drug"], "name":"Synthetic drug"}), lic("open"))]);
    let metadata = json!({"prov_source_url":"https://example.invalid/original-row", "prov_retrieved_at":"2026-10-03T00:00:00Z"});
    d.jsonl("cache/kgx/proof/edges.jsonl", &[
        with(with(json!({"id":"original:edge-7","subject":GENE,"predicate":"biolink:affects","object":"CHEMBL.COMPOUND:proof"}), lic("open")), metadata.clone()),
        with(with(json!({"subject":"STXBP1","predicate":"biolink:interacts_with","object":"CHEMBL.COMPOUND:proof"}), lic("open")), metadata),
    ]);
    let mut b = fixtures::builder(&atlas);
    let ident = sssom::identity(&b, d.path()).unwrap();
    kgx::ingest(&mut b, d.path(), &ident, kgx::Scope::default()).unwrap();
    let g = Graph::new(b.data);
    let mut ids = Vec::new();
    for (idx, record) in g.data().records.iter().enumerate() {
        if !g.provenance().entity(record.entity).file.ends_with("edges.jsonl") { continue; }
        ids.push(record.id.clone());
        assert_eq!(record.url.as_deref(), Some("https://example.invalid/original-row"));
        assert_eq!(record.fetched_at.as_deref(), Some("2026-10-03T00:00:00Z"));
        assert!(super::verify::record(d.path(), &g, idx as u32).matches);
    }
    assert_eq!(ids, ["original:edge-7", "STXBP1|biolink:interacts_with|CHEMBL.COMPOUND:proof"]);
    assert!(g.edges().iter().all(|e| e.to == GENE));
    assert!(integrity::check(&atlas, &g).passed);
}
