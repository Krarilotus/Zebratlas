use atlas_core::withhold::{KeyKind, Salt};
use atlas_discovery::align::researchers::{orcid, withholding};

fn temp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-align-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn researcher_identifiers_require_orcid_checksum() {
    assert_eq!(
        orcid("https://orcid.org/0000-0002-1825-0097").as_deref(),
        Some("0000-0002-1825-0097")
    );
    assert!(orcid("0000-0002-1825-0098").is_none());
    assert!(orcid("bogus").is_none());
}

#[test]
fn gard_legacy_ids_are_only_zero_padded_not_renumbered() {
    use atlas_discovery::align::gard::gard_id;
    assert_eq!(gard_id("12900").as_deref(), Some("GARD:0012900"));
    assert_eq!(gard_id("GARD:0012900").as_deref(), Some("GARD:0012900"));
    assert!(gard_id("12900/new").is_none());
    assert!(gard_id("0").is_none());
}

#[test]
fn rxnorm_formulations_never_become_chemical_identity() {
    use atlas_discovery::align::rxnorm::ingredient;
    assert!(ingredient("IN"));
    assert!(ingredient("PIN"));
    for tty in ["SCD", "SBD", "MIN", "BN", "GPCK", "", "unknown"] {
        assert!(!ingredient(tty));
    }
}

#[test]
fn ror_chosen_metadata_only_corroborates_a_candidate() {
    use atlas_discovery::align::affiliation::chosen_corroborated;
    assert!(chosen_corroborated(true, true, true, true));
    for i in 0..4 {
        let mut guards = [true; 4];
        guards[i] = false;
        assert!(!chosen_corroborated(guards[0], guards[1], guards[2], guards[3]));
    }
}

#[test]
fn api_evidence_must_resolve_to_its_hashed_raw_response() {
    use atlas_discovery::align::{MappingSet, gard::http_input, sha256_file};
    let p = temp("http").join("response.json");
    std::fs::write(&p, b"{\"properties\":{\"tty\":\"IN\"}}").unwrap();
    let (hash, _) = sha256_file(&p).unwrap();
    let mut e = serde_json::json!({"status":200,"url":"https://rxnav.nlm.nih.gov/REST/rxcui/1/properties.json",
        "version":"test-fixture","retrieved_at":"2026-10-03T00:00:00Z","raw_path":p,"sha256":hash,
        "response":{"properties":{"tty":"IN"}}});
    assert!(
        http_input(&mut MappingSet::default(), &e, "public-domain")
            .unwrap()
            .is_some()
    );
    e["response"]["properties"]["tty"] = serde_json::json!("SCD");
    assert!(http_input(&mut MappingSet::default(), &e, "public-domain").is_err());
    e["status"] = serde_json::json!(403);
    assert!(
        http_input(&mut MappingSet::default(), &e, "public-domain")
            .unwrap()
            .is_none()
    );
    std::fs::remove_file(p).unwrap();
}

#[test]
fn new_links_may_not_collapse_existing_granularity_or_join_two_ids() {
    use atlas_discovery::align::{CLOSE, MappingSet, Row, check_background};
    let p = temp("constraints");
    std::fs::create_dir_all(p.join("cache/mappings")).unwrap();
    let file = p.join("cache/mappings/background.sssom.tsv");
    std::fs::write(&file,"subject_id\tpredicate_id\tobject_id\nMONDO:0000001\tskos:exactMatch\tORPHA:1\nMONDO:0000002\tskos:exactMatch\tORPHA:2\nMONDO:0000001\tskos:broadMatch\tOMIM:111111\n").unwrap();
    let mut s = MappingSet {
        rows: vec![
            Row::xref("MONDO:0000001", "GARD:0000001", 0, "test"),
            Row::xref("OMIM:111111", "GARD:0000001", 0, "test"),
        ],
        ..Default::default()
    };
    check_background(&mut s, &p, &["background"], "GARD").unwrap();
    assert!(
        s.rows
            .iter()
            .all(|r| r.predicate == CLOSE && r.conflict == "cross_source")
    );
    let mut s = MappingSet {
        rows: vec![
            Row::xref("MONDO:0000001", "GARD:0000001", 0, "test"),
            Row::xref("ORPHA:2", "GARD:0000001", 0, "test"),
        ],
        ..Default::default()
    };
    check_background(&mut s, &p, &["background"], "GARD").unwrap();
    assert!(
        s.rows
            .iter()
            .all(|r| r.predicate == CLOSE && r.conflict == "cluster_conflict")
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn researcher_build_applies_shared_suppression_and_never_releases_people() {
    let p = temp("suppression");
    std::fs::create_dir_all(p.join("cache/pubmed")).unwrap();
    std::fs::write(p.join("cache/pubmed/test.json"), serde_json::to_vec(&serde_json::json!({
        "schema":"pubmed.articles", "version":1, "header":{"retrieved_at":"2026-10-03T00:00:00Z"},
        "records":[{"id":"PMID:1", "url":"https://pubmed.ncbi.nlm.nih.gov/1/", "gene":"STXBP1",
            "authors":[{"fore_name":"Synthetic", "last_name":"Fixture", "orcid":"0000-0002-1825-0097", "affiliations":[]}]}]
    })).unwrap()).unwrap();
    let set = atlas_discovery::align::researchers::build(&p).unwrap().remove(0);
    assert_eq!(set.rows.len(), 1);
    assert_eq!(set.extra["release"], false);
    assert!(set.rows[0].subject_label.is_empty());
    let salt = Salt::from_config(std::env::var("ATLAS_SUPPRESSION_SALT").ok().as_deref());
    std::fs::write(
        p.join("suppression.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema":"suppression", "version":1, "entries":[{"id":"sup_fixture", "reason":"test",
              "date":"2026-10-03T00:00:00Z", "reviewer":"test", "salt_id":salt.id(),
              "keys":[salt.key(KeyKind::Orcid,"0000-0002-1825-0097").unwrap()]}]
        }))
        .unwrap(),
    )
    .unwrap();
    let set = atlas_discovery::align::researchers::build(&p).unwrap().remove(0);
    assert!(set.rows.is_empty());
    assert_eq!(set.excluded["suppressed researcher mention"].0, 1);
    std::fs::write(p.join("suppression.json"), b"{broken").unwrap();
    assert!(withholding(&p).is_err());
    std::fs::remove_file(p.join("suppression.json")).unwrap();
    assert!(withholding(&p).is_ok());
    // Only explicit fixture files are removed; no shared data or recursive delete.
    std::fs::remove_file(p.join("cache/pubmed/test.json")).unwrap();
}

#[test]
fn researcher_name_ror_and_topic_only_nominate_and_distinct_orcids_never_join() {
    let p = temp("researcher-candidates");
    std::fs::create_dir_all(p.join("cache/pubmed")).unwrap();
    let path = p.join("cache/pubmed/test.json");
    let author = serde_json::json!({"fore_name":"Synthetic","last_name":"Fixture","affiliations":["Synthetic institution"],"ror_ids":["ROR:03yrm5c26"]});
    let mut v = serde_json::json!({"schema":"pubmed.articles","version":1,"header":{"retrieved_at":"2026-10-03T00:00:00Z"},
      "records":[{"id":"PMID:1","gene":"STXBP1","authors":[author.clone()]},{"id":"PMID:2","gene":"STXBP1","authors":[author]}]});
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let s = atlas_discovery::align::researchers::build(&p).unwrap().remove(0);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].predicate, "skos:closeMatch");
    v["records"][0]["authors"][0]["orcid"] = serde_json::json!("0000-0002-1825-0097");
    v["records"][1]["authors"][0]["orcid"] = serde_json::json!("0000-0001-5109-3700");
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let s = atlas_discovery::align::researchers::build(&p).unwrap().remove(0);
    assert_eq!(s.rows.len(), 2);
    assert!(s.rows.iter().all(|r| r.predicate == "skos:exactMatch"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn suppression_of_one_named_assertion_removes_other_occurrences_of_its_orcid() {
    let p = temp("suppression-propagation");
    std::fs::create_dir_all(p.join("cache/pubmed")).unwrap();
    let path = p.join("cache/pubmed/test.json");
    let v = serde_json::json!({"schema":"pubmed.articles","version":1,"header":{},"records":[
      {"id":"PMID:1","authors":[{"fore_name":"Synthetic","last_name":"Fixture","orcid":"0000-0002-1825-0097","affiliations":["Institution"]}]},
      {"id":"PMID:2","authors":[{"fore_name":"Variant","last_name":"Name","orcid":"0000-0002-1825-0097","affiliations":["Another institution"]}]}]});
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let salt = Salt::from_config(std::env::var("ATLAS_SUPPRESSION_SALT").ok().as_deref());
    std::fs::write(
        p.join("suppression.json"),
        serde_json::to_vec(&serde_json::json!({"schema":"suppression","version":1,
      "entries":[{"id":"sup_fixture","keys":[salt.key(KeyKind::NameAff,"Synthetic Fixture|Institution")],
      "reason":"test","date":"2026-10-04T00:00:00Z","reviewer":"test","salt_id":salt.id()}]}))
        .unwrap(),
    )
    .unwrap();
    let s = atlas_discovery::align::researchers::build(&p).unwrap().remove(0);
    assert!(s.rows.is_empty());
    assert_eq!(s.excluded["suppression propagated across asserted identity"].0, 1);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(p.join("suppression.json")).unwrap();
}
