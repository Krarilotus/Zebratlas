use atlas_discovery::align::{MappingSet, XREF, safety::*};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "atlas-identity-safety-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(p.join("inputs")).unwrap();
    p
}
fn input(dir: &Path, name: &str, pairs: &[(&str, &str, &str)]) {
    let mut text = "subject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\n".to_string();
    for (a, p, b) in pairs {
        text.push_str(&format!(
            "{a}\t{p}\t{b}\t{LEGACY_XREF}\tR-DIS-01\t1.0.0\thttps://example.invalid/synthetic\t{}\tsynthetic-record\n",
            "a".repeat(64)
        ));
    }
    std::fs::write(dir.join(name), text).unwrap();
}
fn ledger(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("identity-decisions.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn regime_method_and_syntax_are_independent_vetoes() {
    assert_eq!(
        eligibility(
            "atlasorg:synthetic",
            "ROR:synthetic",
            "semapv:CompositeMatching",
            "",
            "R-ORG-05"
        ),
        Err("organisation_without_source_equivalence")
    );
    assert_eq!(
        eligibility("atlasorg:synthetic", "ROR:synthetic", XREF, "", "R-ORG-01"),
        Err("organisation_without_source_equivalence")
    );
    assert!(eligibility("crossref.funder:synthetic", "ROR:synthetic", XREF, "", "R-FUN-02").is_ok());
    assert_eq!(
        eligibility("HGNC:1", "UniProtKB:P1", XREF, "", "R-GEN-02"),
        Err("incompatible_entity_regime")
    );
    assert_eq!(
        eligibility("OMIMPS:1", "MONDO:1", XREF, "", "R-DIS-01"),
        Err("incompatible_entity_regime")
    );
    assert_eq!(
        eligibility(
            "ORCID:0000-0002-1825-0098",
            "author-mention:synthetic",
            XREF,
            "",
            "R-PER-01"
        ),
        Err("invalid_identifier")
    );
    assert_eq!(
        eligibility(
            "reporter-pi:synthetic1",
            "cordis-person:synthetic2",
            XREF,
            "",
            "R-PER-01"
        ),
        Err("person_without_orcid")
    );
    assert_eq!(
        eligibility("MONDO:1", "DOID:1", "semapv:InventedMatching", "", "R-DIS-01"),
        Err("unknown_justification")
    );
    assert!(!known_justification(LEGACY_XREF));
    assert!(!valid_doi("10.bogus"));
    assert!(!valid_doi("10.1234/"));
    assert!(!valid_doi("10.1234/space here"));
    assert!(valid_doi("10.1234/synthetic(1)"));
    assert!(eligibility("UNII:SYNTHETIC0", "RXNORM:123", XREF, "", "R-DRG-02").is_ok());
    assert_eq!(
        eligibility("UNII:SYNTHETIC0", "RXNORM:123", XREF, "", "R-DRG-01"),
        Err("incompatible_entity_regime")
    );
}

#[test]
fn historical_organisation_merges_are_invalidated_without_losing_assertions() {
    let p = fixture();
    let mut content = "subject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tevidence_url\tevidence_sha256\tevidence_locator\n".to_string();
    for (n, rule, method) in [
        (1, "R-ORG-01", "semapv:CompositeMatching"),
        (2, "R-ORG-03", XREF), // API match cannot be rehabilitated by relabeling its method.
        (3, "R-ORG-05", "semapv:CompositeMatching"),
    ] {
        content.push_str(&format!("atlasorg:synthetic{n}\tskos:exactMatch\tROR:synthetic{n}\t{method}\t{rule}\t1.0.0\thttps://example.invalid/synthetic\t{}\trecord {n}\n", "a".repeat(64)));
    }
    content.push_str(&format!("crossref.funder:synthetic\tskos:exactMatch\tROR:synthetic-funder\t{XREF}\tR-FUN-02\t1.0.0\thttps://example.invalid/synthetic\t{}\tpreferred FundRef\n", "a".repeat(64)));
    std::fs::write(p.join("inputs/org.sssom.tsv"), &content).unwrap();
    let report = directory(&p.join("inputs"), &p.join("output"), None).unwrap();
    assert_eq!(report["counts"]["rejected"], 3);
    assert_eq!(report["counts"]["accepted"], 1);
    assert_eq!(
        std::fs::read_to_string(p.join("inputs/org.sssom.tsv")).unwrap(),
        content
    );
    let records = ledger(&p.join("output"));
    for record in &records[..3] {
        assert_eq!(record["reason"], "organisation_without_source_equivalence");
        assert_eq!(record["prov:wasDerivedFrom"]["original_values"][1], "skos:exactMatch");
    }
    let invalidations: Vec<Value> = std::fs::read_to_string(p.join("output/identity-invalidations.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(invalidations.len(), 3);
    for (record, invalidation) in records.iter().zip(invalidations) {
        assert_eq!(
            invalidation["invalidated_assertion_id"],
            record["prov:wasDerivedFrom"]["@id"]
        );
        assert!(invalidation["scope"].as_str().unwrap().contains("inference dependents"));
    }
    let projected = std::fs::read_to_string(p.join("output/org.sssom.tsv")).unwrap();
    let lines: Vec<_> = projected.lines().filter(|line| !line.starts_with('#')).collect();
    assert!(
        lines[1..4]
            .iter()
            .all(|line| line.split('\t').nth(1) == Some("skos:closeMatch"))
    );
    assert_eq!(lines[4].split('\t').nth(1), Some("skos:exactMatch"));
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn cross_file_collision_is_quarantined_with_all_assertions_and_witnesses() {
    let p = fixture();
    input(
        &p.join("inputs"),
        "a.sssom.tsv",
        &[("MONDO:1", "skos:exactMatch", "DOID:1")],
    );
    input(
        &p.join("inputs"),
        "b.sssom.tsv",
        &[
            ("MONDO:2", "skos:exactMatch", "DOID:1"),
            ("HGNC:1", "RO:0002205", "UniProtKB:P1"),
        ],
    );
    let report = directory(&p.join("inputs"), &p.join("output"), None).unwrap();
    assert_eq!(report["counts"]["rejected"], 2);
    assert_eq!(report["counts"]["typed_or_candidate"], 1);
    let records = ledger(&p.join("output"));
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["conflict_component_witness"].as_array().unwrap().len(), 3);
    assert_eq!(
        records[0]["prov:wasDerivedFrom"]["original_values"][1],
        "skos:exactMatch"
    );
    for file in ["a.sssom.tsv", "b.sssom.tsv"] {
        let text = std::fs::read_to_string(p.join("output").join(file)).unwrap();
        let rows: Vec<_> = text.lines().filter(|l| !l.starts_with('#')).collect();
        let n = rows[0].split('\t').count();
        assert!(rows[1..].iter().all(|r| r.split('\t').count() == n));
    }
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn revocation_rebuilds_split_and_stale_reviews_fail_closed() {
    let p = fixture();
    input(
        &p.join("inputs"),
        "a.sssom.tsv",
        &[("MONDO:1", "skos:exactMatch", "DOID:1")],
    );
    let first = directory(&p.join("inputs"), &p.join("first"), None).unwrap();
    assert_eq!(first["counts"]["accepted"], 1);
    let id = ledger(&p.join("first"))[0]["prov:wasDerivedFrom"]["@id"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    let review = json!({"input_digest":first["input_digest"],"actor":"synthetic-reviewer","reviewed_at":"2026-10-04T00:00:00Z","reason":"synthetic revocation","revoke":[id]});
    std::fs::write(p.join("review.json"), serde_json::to_vec(&review).unwrap()).unwrap();
    let second = directory(&p.join("inputs"), &p.join("second"), Some(&p.join("review.json"))).unwrap();
    assert_eq!(second["counts"]["rejected"], 1);
    assert_ne!(
        first["prov:wasGeneratedBy"]["@id"],
        second["prov:wasGeneratedBy"]["@id"]
    );
    assert_ne!(ledger(&p.join("first"))[0]["@id"], ledger(&p.join("second"))[0]["@id"]);
    assert_eq!(ledger(&p.join("second"))[0]["reason"], "review_revoked");
    let third = directory(&p.join("inputs"), &p.join("third"), None).unwrap();
    assert_eq!(
        third["counts"]["accepted"], 1,
        "original inputs reproduce the accepted component"
    );
    input(
        &p.join("inputs"),
        "a.sssom.tsv",
        &[("MONDO:2", "skos:exactMatch", "DOID:1")],
    );
    assert!(directory(&p.join("inputs"), &p.join("stale"), Some(&p.join("review.json"))).is_err());
    assert!(!p.join("stale").exists());
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn every_exclusion_survives_beyond_summary_examples() {
    let p = fixture();
    let mut s = MappingSet {
        id: "gene-xrefs".into(),
        ..Default::default()
    };
    s.inputs.push(atlas_discovery::align::Input {
        role: "synthetic".into(),
        path: "synthetic.tsv".into(),
        url: "https://example.invalid/synthetic".into(),
        version: "synthetic-v1".into(),
        sha256: "a".repeat(64),
        bytes: 0,
        license: "CC0-1.0".into(),
        retrieved_at: None,
    });
    for i in 0..17 {
        s.exclude_from(Some(0), "synthetic exclusion", format!("record {i}"));
    }
    assert_eq!(s.excluded["synthetic exclusion"].0, 17);
    assert_eq!(s.excluded["synthetic exclusion"].1.len(), 5);
    assert_eq!(s.exclusion_ledger.len(), 17);
    atlas_discovery::align::write_set(&s, &p, "2026-10-04T00:00:00Z", 0).unwrap();
    let rows: Vec<Value> = std::fs::read_to_string(p.join("gene-xrefs.exclusions.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 17);
    assert_eq!(rows[16]["record_locator"], "record 16");
    assert_eq!(rows[16]["prov:wasDerivedFrom"][0]["version"], "synthetic-v1");
    std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn asset_receipt_aliases_round_trip_and_unknown_methods_never_accept() {
    let p = fixture();
    let content = format!(
        "subject_id\tpredicate_id\tobject_id\tmapping_justification\trule_id\trule_version\tprov_source_url\tprov_sha256\tprov_locator\nCVCL:SYNTHETIC\tskos:exactMatch\tRRID:CVCL_SYNTHETIC\tsemapv:ManualMappingCuration\tR-CELL-01\t1.0.0\thttps://example.invalid/synthetic\t{}\t/synthetic/record\n",
        "a".repeat(64)
    );
    std::fs::write(p.join("inputs/assets-cell-identifiers.sssom.tsv"), content).unwrap();
    let report = directory(&p.join("inputs"), &p.join("output"), None).unwrap();
    assert_eq!(report["counts"]["accepted"], 1);
    assert!(
        std::fs::read_to_string(p.join("output/assets-cell-identifiers.sssom.tsv"))
            .unwrap()
            .contains("evidence_locator")
    );
    let bad = std::fs::read_to_string(p.join("inputs/assets-cell-identifiers.sssom.tsv"))
        .unwrap()
        .replace("semapv:ManualMappingCuration", "semapv:InventedMatching");
    std::fs::write(p.join("inputs/assets-cell-identifiers.sssom.tsv"), bad).unwrap();
    let bad_report = directory(&p.join("inputs"), &p.join("bad"), None).unwrap();
    assert_eq!(bad_report["counts"]["rejected"], 1);
    assert_eq!(ledger(&p.join("bad"))[0]["reason"], "unknown_justification");
    std::fs::remove_dir_all(p).unwrap();
}
