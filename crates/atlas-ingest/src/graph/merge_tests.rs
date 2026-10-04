use super::{fixtures, sssom};
use atlas_core::graph::Graph;

fn row(rule: &str, version: &str, hash: &str, locator: &str, predicate: &str) -> String {
    format!(
        "# mapping_set_id: https://example.org/mappings/test\n# mapping_set_version: fixture-v1\nsubject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\nMONDO:0012812\t{predicate}\tDOID:1\tsemapv:DatabaseCrossReference\t{rule}\t{version}\thttps://example.org/source\t{hash}\t{locator}\n"
    )
}

#[test]
fn exact_without_known_rule_version_or_complete_evidence_fails_closed() {
    let atlas = fixtures::atlas();
    for (rule, version, hash, loc) in [
        ("", "1.0.0", "a".repeat(64), "record"),
        ("R-DIS-01", "99.0.0", "a".repeat(64), "record"),
        ("R-DIS-01", "1.0.0", "broken".into(), "record"),
        ("R-DIS-01", "1.0.0", "a".repeat(64), ""),
        ("R-TRI-02", "1.0.0", "a".repeat(64), "record"),
    ] {
        let d = fixtures::TempData::new();
        d.write(
            "cache/mappings/test.sssom.tsv",
            row(rule, version, &hash, loc, "skos:exactMatch").as_bytes(),
        );
        let b = fixtures::builder(&atlas);
        assert!(sssom::identity(&b, d.path()).is_err());
    }
}

#[test]
fn cluster_retains_each_member_and_exact_row_provenance_after_snapshot() {
    let atlas = fixtures::atlas();
    let d = fixtures::TempData::new();
    d.write(
        "cache/mappings/test.sssom.tsv",
        row("R-DIS-01", "1.0.0", &"a".repeat(64), "record", "skos:exactMatch").as_bytes(),
    );
    fixtures::gate(&d);
    let mut b = fixtures::builder(&atlas);
    let id = sssom::identity(&b, d.path()).unwrap();
    sssom::finish(&mut b, d.path(), &id).unwrap();
    let snapshot = d.path().join("graph.snapshot");
    atlas_core::snapshot::save_graph(&snapshot, &b.data, "test").unwrap();
    let (graph, _) = atlas_core::snapshot::load_graph(&snapshot).unwrap();
    let merge = graph.identity_merge("DOID:1").unwrap();
    assert_eq!(merge.activity.label, "identity merge");
    assert_eq!(merge.mappings[0].rule_id, "R-DIS-01");
    assert!(merge.mappings[0].mapping_set_version.starts_with("identity-gate-"));
    assert!(!merge.mappings[0].decision_id.is_empty());
    assert_eq!(merge.mappings[0].gate_manifest_sha256.len(), 64);
    assert_eq!(merge.members.len(), 2);
    assert!(merge.members.iter().all(|m| !m.derived_from.is_empty()));
    assert!(super::verify::record(d.path(), &graph, merge.mappings[0].record).matches);
    let c = atlas_core::integrity::check(&atlas, &graph);
    assert!(
        c.contracts
            .iter()
            .find(|c| c.id == "identity-merge-has-rule-evidence")
            .unwrap()
            .passed
    );
    let mut broken = graph.into_data();
    broken.identity_merges.clear();
    let c = atlas_core::integrity::check(&atlas, &Graph::new(broken));
    assert!(
        !c.contracts
            .iter()
            .find(|c| c.id == "identity-merge-has-rule-evidence")
            .unwrap()
            .passed
    );
}

#[test]
fn candidates_remain_candidates_without_merge_metadata() {
    let atlas = fixtures::atlas();
    let d = fixtures::TempData::new();
    d.write(
        "cache/mappings/test.sssom.tsv",
        row("", "", "", "", "skos:closeMatch").as_bytes(),
    );
    let b = fixtures::builder(&atlas);
    let id = sssom::identity(&b, d.path()).unwrap();
    assert_eq!(id.canonical("DOID:1"), "DOID:1");
}

#[test]
fn composite_organisation_rows_retain_candidate_evidence_without_aliases() {
    let atlas = fixtures::atlas();
    let d = fixtures::TempData::new();
    d.write(
        "cache/mappings/org-ror.sssom.tsv",
        b"subject_id\tpredicate_id\tobject_id\tmapping_justification\n\
atlasorg:PLACEHOLDER\tskos:exactMatch\tROR:PLACEHOLDER\tsemapv:CompositeMatching\n\
atlasorg:PLACEHOLDER\towl:sameAs\tROR:PLACEHOLDER\tsemapv:CompositeMatching\n\
atlasorg:PLACEHOLDER\towl:equivalentClass\tROR:PLACEHOLDER\tsemapv:CompositeMatching\n",
    );
    let mut b = fixtures::builder(&atlas);
    b.register("atlasorg:PLACEHOLDER", atlas_core::node::NodeKind::Organisation, 0);
    b.register("ROR:PLACEHOLDER", atlas_core::node::NodeKind::Organisation, 1);
    let identity = sssom::identity(&b, d.path()).unwrap();
    assert_eq!(identity.exact_rows, 0);
    assert_eq!(identity.canonical("ROR:PLACEHOLDER"), "ROR:PLACEHOLDER");
    sssom::finish(&mut b, d.path(), &identity).unwrap();
    assert!(b.data.aliases.is_empty());
    assert!(b.data.identity_merges.is_empty());
    let edge = &b.data.edges[0];
    assert_eq!(edge.relation, atlas_core::graph::Relation::CandidateSameAs);
    assert_eq!(edge.from, "atlasorg:PLACEHOLDER");
    assert_eq!(edge.to, "ROR:PLACEHOLDER");
    assert_eq!(edge.records.len(), 3);
    let record = &b.data.records[edge.records[0] as usize];
    assert_ne!(record.sha256, [0; 32]);
    assert_eq!(record.id, "atlasorg:PLACEHOLDER");
}
