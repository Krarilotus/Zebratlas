use super::*;
use crate::graph::PersonSource;

fn person(id: &str, name: &str, orcid: Option<&str>, aff: &str) -> Person {
    Person {
        id: id.into(),
        name: name.into(),
        name_variants: vec![],
        orcids: orcid.into_iter().map(String::from).collect(),
        affiliations: vec![aff.into()],
        source: PersonSource::Resolved,
        genes: vec![],
        communities: vec![],
        cross_community: false,
        matched_by: vec![],
        merge_basis: vec![],
        records: vec![],
    }
}

fn entry(salt: &Salt, keys: Vec<String>) -> SuppressionEntry {
    SuppressionEntry {
        id: "sup_test".into(),
        keys,
        scope: Scope::All,
        reason: "gdpr_art17_erasure".into(),
        date: "2026-10-04T10:00:00Z".into(),
        reviewer: "agent:reviewer/tester".into(),
        request: Some("pr_test".into()),
        salt_id: salt.id().into(),
    }
}

fn list(entries: Vec<SuppressionEntry>) -> Vec<u8> {
    serde_json::to_vec(&SuppressionFile {
        entries,
        ..SuppressionFile::new()
    })
    .unwrap()
}

#[test]
fn orcid_spellings_normalise() {
    let want = Some("0000-0002-1825-009X".to_string());
    assert_eq!(normalise_orcid("https://orcid.org/0000-0002-1825-009x"), want);
    assert_eq!(normalise_orcid("ORCID:0000000218250 09X"), want);
    assert_eq!(normalise_orcid("12345"), None);
}

#[test]
fn keys_are_salted_and_normalised() {
    let a = Salt::new("one");
    let b = Salt::new("two");
    assert_eq!(
        a.key(KeyKind::NameAff, "Ada  LOVELACE|Analytical Society"),
        a.key(KeyKind::NameAff, "ada lovelace|analytical society!")
    );
    assert_ne!(a.key(KeyKind::Email, "x@y.org"), b.key(KeyKind::Email, "x@y.org"));
    assert_eq!(a.key(KeyKind::Email, "nope"), None);
    assert!(!format!("{a:?}").contains("one"), "the salt is never printed");
}

#[test]
fn suppressed_person_matches_by_orcid_or_name_affiliation() {
    let salt = Salt::new("t");
    let p = person("ORCID:0000-0002-1825-0097", "Test Person", None, "Example Lab");
    let other = person("person:other", "Other Person", None, "Example Lab");
    let by_orcid = entry(
        &salt,
        salt.key(KeyKind::Orcid, "0000-0002-1825-0097").into_iter().collect(),
    );
    let w = Withhold::from_lists(salt.clone(), Some(&list(vec![by_orcid])), None).unwrap();
    assert_eq!(w.person(&p).unwrap().entry, "sup_test");
    assert!(w.person(&other).is_none());
    let by_name = entry(
        &salt,
        salt.key(KeyKind::NameAff, "test person|example lab")
            .into_iter()
            .collect(),
    );
    let w = Withhold::from_lists(salt, Some(&list(vec![by_name])), None).unwrap();
    assert!(w.person(&p).is_some());
    assert!(w.contact("Test Person", "Example Lab", None).is_some());
}

#[test]
fn fail_closed_on_bad_list_version_or_salt() {
    let salt = Salt::new("t");
    assert!(Withhold::from_lists(salt.clone(), Some(b"{oops"), None).is_err());
    let mut f = SuppressionFile::new();
    f.version = 2;
    assert!(matches!(
        Withhold::from_lists(salt.clone(), Some(&serde_json::to_vec(&f).unwrap()), None),
        Err(WithholdError::Version { .. })
    ));
    let foreign = entry(&Salt::new("other"), vec![]);
    let w = Withhold::from_lists_or_closed(salt.clone(), Some(&list(vec![foreign])), None);
    assert!(w.closed_reason().is_some());
    let anyone = person("person:x", "Anyone", None, "");
    assert_eq!(w.person(&anyone).unwrap().entry, "closed");
    assert!(w.contact("Anyone", "", None).is_some());
    // Missing files are empty lists, not errors.
    assert!(
        Withhold::from_lists(salt.clone(), None, None)
            .unwrap()
            .person(&anyone)
            .is_none()
    );
    // The suppression file never stores identifiers in clear text.
    let text = String::from_utf8(list(vec![entry(&salt, salt.person_keys(&anyone))])).unwrap();
    assert!(!text.contains("Anyone") && !text.contains("person:x"));
}

#[test]
fn record_withhold_trait_combines_both_lists_and_canonical_locators() {
    use crate::graph::{RecordHash, RecordWithhold, SourceRecord};
    use crate::provenance::SourceEntity;
    let salt = Salt::new("t");
    let suppressed = person("person:removed", "Synthetic Removed", None, "Example Lab");
    let mut quarantined = person("person:quarantined", "Synthetic Quarantined", None, "Example Lab");
    quarantined.records = vec![0];
    let suppression = list(vec![entry(&salt, salt.person_keys(&suppressed))]);
    let quarantine = br#"{"records":[{"cache_file":"x.jsonl","record_locator":"line:7","reason":"blocked"}]}"#;
    let w = Withhold::from_lists(salt, Some(&suppression), Some(quarantine)).unwrap();
    assert!(w.record("cache/x.jsonl", &Locator::Record("#line:7".into())).is_some());
    let mut data = GraphData::default();
    let entity = data.provenance.add_entity(SourceEntity {
        file: "cache/x.jsonl".into(),
        ..Default::default()
    });
    data.records.push(SourceRecord {
        entity,
        locator: Locator::Line(7),
        id: "fixture".into(),
        url: None,
        fetched_at: None,
        hash: RecordHash::CanonicalJson,
        sha256: [0; 32],
    });
    data.people = vec![
        suppressed,
        quarantined,
        person("person:kept", "Synthetic Kept", None, "Example Lab"),
    ];
    let mut graph = Graph::new(data);
    graph.set_withhold(w);
    assert_eq!(
        graph.node_withheld(graph.node("person:removed").unwrap()),
        Some("gdpr_art17_erasure")
    );
    assert_eq!(
        graph.node_withheld(graph.node("person:quarantined").unwrap()),
        Some("blocked")
    );
    assert_eq!(graph.records_withheld(&[0]), Some("blocked"));
    assert_eq!(graph.node_withheld(graph.node("person:kept").unwrap()), None);
    for invalid in [
        br#"{}"#.as_slice(),
        br#"{"records":[{"file":"../x"}]}"#,
        br#"{"records":[{"file":"x","locator":{}}]}"#,
    ] {
        assert!(Withhold::from_lists(Salt::new("t"), None, Some(invalid)).is_err());
    }
}

#[test]
fn quarantine_records_by_file_and_locator() {
    let q = br#"{"schema":"quarantine","version":1,"records":[
        {"file":"outcomes/resources.json","record_locator":"/records/1","reason":"fetched_after_block"},
        {"file":"cache/x/whole.jsonl"}]}"#;
    let w = Withhold::from_lists(Salt::new("t"), None, Some(q)).unwrap();
    let hit = w.record("cache/outcomes/resources.json", &Locator::Record("records[1]".into()));
    assert_eq!(hit.map(|h| h.reason), Some("fetched_after_block"));
    assert!(
        w.record("cache/outcomes/resources.json", &Locator::Record("records[0]".into()))
            .is_none()
    );
    assert!(w.record("x/whole.jsonl", &Locator::Line(7)).is_some());
}

#[test]
fn cached_visibility_refreshes_and_closed_lists_still_withhold_every_person() {
    use crate::graph::RecordWithhold;
    let salt = Salt::new("fixture");
    let p = person("person:fixture", "Synthetic Fixture", None, "Example Lab");
    let suppression = list(vec![entry(&salt, salt.person_keys(&p))]);
    let mut graph = Graph::new(GraphData {
        people: vec![p],
        ..Default::default()
    });
    let key = graph.node("person:fixture").unwrap();
    graph.set_withhold(Withhold::from_lists(salt.clone(), Some(&suppression), None).unwrap());
    assert_eq!(graph.node_withheld(key), Some("gdpr_art17_erasure"));
    graph.set_withhold(Withhold::empty(salt.clone()));
    assert_eq!(graph.node_withheld(key), None, "a new filter replaces cached decisions");
    graph.set_withhold(Withhold::closed(salt, "synthetic unreadable list"));
    assert!(
        graph.node_withheld(key).is_some(),
        "the derived cache must stay fail-closed"
    );
}

#[test]
fn quarantine_url_fallback_ignores_empty_or_invalid_urls() {
    for url in ["", "not a URL", "https://", "https://user:password@example.org/x"] {
        let file = serde_json::json!({"records":[{"file":"x.json", "source_url":url}]});
        let w = Withhold::from_lists(Salt::new("t"), None, Some(&serde_json::to_vec(&file).unwrap())).unwrap();
        assert!(w.source_url(url).is_none());
        assert!(
            w.source_url("").is_none(),
            "missing provenance URLs must not match quarantine"
        );
        assert!(
            w.record("x.json", &Locator::Line(1)).is_some(),
            "record quarantine still applies"
        );
    }
}
