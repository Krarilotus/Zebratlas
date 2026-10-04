//! Identity alignment rules (docs/design/IDENTITY.md) on synthetic rows.

use atlas_discovery::align::{
    CLOSE, EXACT, MappingSet, Row, check_cardinality, check_clusters, check_clusters_ordered, clusters,
    trial::{ictrp_main, recognise},
};

fn set(rows: &[(&str, &str)]) -> MappingSet {
    MappingSet {
        rows: rows.iter().map(|(s, o)| Row::xref(s, o, 0, "test")).collect(),
        ..Default::default()
    }
}

fn pred<'a>(s: &'a MappingSet, sub: &str, obj: &str) -> &'a Row {
    s.rows
        .iter()
        .find(|r| r.subject_id == sub && r.object_id == obj)
        .unwrap()
}

#[test]
fn one_to_one_stays_exact_one_to_many_is_flagged_not_dropped() {
    let mut s = set(&[("MONDO:1", "OMIM:1"), ("MONDO:2", "OMIM:2"), ("MONDO:2", "OMIM:3")]);
    check_cardinality(&mut s);
    assert_eq!(pred(&s, "MONDO:1", "OMIM:1").predicate, EXACT);
    assert_eq!(pred(&s, "MONDO:1", "OMIM:1").cardinality, "1:1");
    let r = pred(&s, "MONDO:2", "OMIM:3");
    assert_eq!(
        (r.predicate.as_str(), r.asserted.as_str(), r.conflict.as_str()),
        (CLOSE, EXACT, "one_to_many")
    );
    assert_eq!(s.rows.len(), 3, "conflicting rows are kept");
}

#[test]
fn many_to_one_is_flagged() {
    let mut s = set(&[
        ("NCT:NCT00000001", "EUDRACT:2010-000001-11"),
        ("NCT:NCT00000002", "EUDRACT:2010-000001-11"),
    ]);
    check_cardinality(&mut s);
    assert!(
        s.rows
            .iter()
            .all(|r| r.predicate == CLOSE && r.conflict == "many_to_one")
    );
}

#[test]
fn chained_exact_links_may_not_join_two_ids_of_one_prefix() {
    // MONDO:1 = ORPHA:1 = OMIM:9 = MONDO:2 is 1:1 per pair but joins two MONDO ids.
    let mut a = set(&[("MONDO:1", "ORPHA:1"), ("MONDO:2", "OMIM:9")]);
    let mut b = set(&[("ORPHA:1", "OMIM:9")]);
    check_cardinality(&mut a);
    check_cardinality(&mut b);
    let n = check_clusters(&mut [&mut a, &mut b], &[]);
    assert!(n > 0);
    assert!(
        a.rows
            .iter()
            .chain(&b.rows)
            .all(|r| !r.is_exact() || !r.subject_id.starts_with("MONDO"))
    );
}

#[test]
fn ordered_check_cuts_the_weakest_terminology_first() {
    // Two sources disagree on the UMLS CUI; the MONDO–ORPHA link survives, both UMLS links are flagged.
    let mut a = set(&[("MONDO:1", "ORPHA:1"), ("MONDO:1", "UMLS:C1")]);
    let mut b = set(&[("ORPHA:1", "UMLS:C2")]);
    check_clusters_ordered(&mut [&mut a, &mut b], &[], &["UMLS", "ORPHA", "MONDO"]);
    assert_eq!(pred(&a, "MONDO:1", "ORPHA:1").predicate, EXACT);
    assert_eq!(pred(&a, "MONDO:1", "UMLS:C1").conflict, "cluster_conflict");
    assert_eq!(pred(&b, "ORPHA:1", "UMLS:C2").predicate, CLOSE);
}

#[test]
fn registry_ids_are_recognised_by_syntax() {
    assert_eq!(recognise("EudraCT 2021-002821-32"), ["EUDRACT:2021-002821-32"]);
    assert_eq!(
        recognise("2022-502298-41-00"),
        ["EUCT:2022-502298-41-00"],
        "EU CT number is not read as EudraCT"
    );
    assert_eq!(recognise("ISRCTN12345678"), ["ISRCTN:12345678"]);
    assert_eq!(recognise("DRKS00012345"), ["DRKS:DRKS00012345"]);
    assert_eq!(recognise("CTRI/2019/01/012345"), ["CTRI:CTRI/2019/01/012345"]);
    assert!(recognise("IRB-2019-77").is_empty());
    assert!(
        recognise("U1111-1332-5367").is_empty(),
        "WHO UTN is not a registry record"
    );
    assert_eq!(recognise("NCT01234567 / 2015-001234-12").len(), 2);
}

#[test]
fn ictrp_main_ids_follow_registry_conventions() {
    assert_eq!(
        ictrp_main("EUCTR2021-002821-32-DE").as_deref(),
        Some("EUDRACT:2021-002821-32")
    );
    assert_eq!(
        ictrp_main("CTIS2022-502298-41-00").as_deref(),
        Some("EUCT:2022-502298-41-00")
    );
    assert_eq!(ictrp_main("NCT05232630").as_deref(), Some("NCT:NCT05232630"));
    assert_eq!(
        ictrp_main("ChiCTR2000034567").as_deref(),
        Some("CHICTR:ChiCTR2000034567")
    );
}

#[test]
fn dedupe_clusters_follow_exact_links_only() {
    let mut s = set(&[("NCT:NCT1", "EUDRACT:1"), ("ICTRP:EUCTR1-DE", "EUDRACT:1")]);
    s.rows.push(Row::link("NCT:NCT1", CLOSE, "NCT:NCT2", 0, "candidate"));
    let c = clusters(&s.rows);
    assert_eq!(
        c,
        vec![vec![
            "EUDRACT:1".to_string(),
            "ICTRP:EUCTR1-DE".into(),
            "NCT:NCT1".into()
        ]]
    );
}
