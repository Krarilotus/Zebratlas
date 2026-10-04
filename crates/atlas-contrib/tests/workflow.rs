//! State machine, auto-checks, duplicate/conflict detection, PROV records, overlay.

mod common;

use atlas_contrib::model::{CheckStatus, Decision, NodeInput, ReviewInput, SubjectKind};
use atlas_contrib::state::{self, Event};
use atlas_contrib::{ContribError, ContributionKind, Overlay, State, Submission};
use common::*;

const PAGE: &str = "https://example.org/about";
const PAGE_HTML: &str = "<html><body><h1>About</h1><p>We support families living with <b>Example encephalopathy</b> in Europe.</p></body></html>";

fn report_code(c: &atlas_contrib::Contribution, name: &str) -> (CheckStatus, String, bool) {
    let check = c
        .checks
        .as_ref()
        .unwrap()
        .find(name)
        .unwrap_or_else(|| panic!("no {name} check: {:#?}", c.checks));
    (check.status, check.code.clone(), check.blocking)
}

fn accept(reason: &str) -> ReviewInput {
    ReviewInput {
        decision: Decision::Accept,
        reason: reason.into(),
    }
}

fn reject(reason: &str) -> ReviewInput {
    ReviewInput {
        decision: Decision::Reject,
        reason: reason.into(),
    }
}

// --- state machine -------------------------------------------------------------------------------

#[test]
fn state_machine_allows_only_the_documented_transitions() {
    let r = Default::default();
    use State::*;
    let ok = |from, ev| state::next(from, ev).ok();
    assert_eq!(ok(Submitted, Event::Checked(&r)), Some(AutoChecked));
    assert_eq!(ok(AutoChecked, Event::Checked(&r)), Some(AutoChecked));
    assert_eq!(ok(AutoChecked, Event::Accept { reason: "ok" }), Some(Accepted));
    assert_eq!(ok(AutoChecked, Event::Reject { reason: "no" }), Some(Rejected));
    assert_eq!(ok(Submitted, Event::Reject { reason: "spam" }), Some(Rejected));
    // Not allowed: accepting unchecked, anything after a final state.
    assert!(matches!(
        state::next(Submitted, Event::Accept { reason: "x" }),
        Err(ContribError::Transition { .. })
    ));
    for done in [Accepted, Rejected] {
        assert!(state::next(done, Event::Checked(&r)).is_err());
        assert!(state::next(done, Event::Accept { reason: "x" }).is_err());
        assert!(state::next(done, Event::Reject { reason: "x" }).is_err());
    }
    // A reason is required.
    assert!(matches!(
        state::next(AutoChecked, Event::Accept { reason: "  " }),
        Err(ContribError::Invalid(_))
    ));
    assert!(matches!(
        state::next(AutoChecked, Event::Reject { reason: "" }),
        Err(ContribError::Invalid(_))
    ));
}

#[tokio::test]
async fn review_workflow_end_to_end() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    let c = s
        .submit(
            new_link(
                "Example Parents Network",
                "MONDO:0000001",
                Some(PAGE),
                Some("support families living with Example encephalopathy"),
            ),
            None,
        )
        .unwrap();
    assert_eq!((c.state, c.version), (State::Submitted, 1));
    // Accepting before the checks is refused.
    assert!(matches!(
        s.review(&c.id, &accept("fine"), reviewer()),
        Err(ContribError::Transition { .. })
    ));

    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!((c.state, c.version), (State::AutoChecked, 2));
    let c = s
        .review(&c.id, &accept("Group website confirms it."), reviewer())
        .unwrap();
    assert_eq!((c.state, c.version), (State::Accepted, 3));
    assert_eq!(c.review.as_ref().unwrap().reason, "Group website confirms it.");
    // Final: no re-check, no second decision.
    assert!(s.run_checks(&c.id).await.is_err());
    assert!(s.review(&c.id, &reject("changed mind"), reviewer()).is_err());
}

#[tokio::test]
async fn spam_can_be_rejected_before_checks() {
    let s = stubbed(vec![]);
    let c = s
        .submit(
            new_link("Buy cheap pills", "MONDO:0000001", Some("https://spam.example/"), None),
            None,
        )
        .unwrap();
    let c = s.review(&c.id, &reject("spam"), reviewer()).unwrap();
    assert_eq!(c.state, State::Rejected);
    assert!(s.overlay().unwrap().edges.is_empty());
}

#[test]
fn submissions_are_validated_per_kind() {
    let s = service();
    let mut sub = new_link("X", "MONDO:0000001", None, None);
    assert!(s.submit(sub.clone(), None).is_ok());
    sub.evidence_url = Some("javascript:alert(1)".into());
    assert!(matches!(s.submit(sub.clone(), None), Err(ContribError::Invalid(_))));
    sub.evidence_url = Some("https://user:pw@example.org/".into());
    assert!(matches!(s.submit(sub.clone(), None), Err(ContribError::Invalid(_))));
    sub.evidence_url = Some("https://example.org/".into());
    sub.subject_kind = None;
    assert!(s.submit(sub.clone(), None).is_ok());
    sub.subject_kind = Some(SubjectKind::Registry);
    sub.target = None;
    assert!(s.submit(sub.clone(), None).is_err());
    sub.target = Some(NodeInput {
        id: Some("MONDO:0000001".into()),
        label: None,
    });
    sub.statement = "   ".into();
    assert!(s.submit(sub.clone(), None).is_ok());
    sub.statement = "x".repeat(2001);
    assert!(s.submit(sub.clone(), None).is_err());
    sub.statement = "ok\u{0007}".into();
    assert!(s.submit(sub.clone(), None).is_err());
    sub.statement = "  A registry for this condition.  ".into();
    let c = s.submit(sub, None).unwrap();
    assert_eq!(c.submission.statement, "A registry for this condition.");

    let mut e = correction("not-an-edge");
    assert!(s.submit(e.clone(), None).is_err());
    e.edge = Some("a|b|c".into());
    assert!(s.submit(e, None).is_ok());
    let outdated = Submission {
        kind: ContributionKind::OutdatedContact,
        subject: NodeInput::default(),
        ..correction("a|b|c")
    };
    assert!(s.submit(outdated, None).is_err());
}

// --- auto-checks -----------------------------------------------------------------------------------

#[tokio::test]
async fn url_and_quote_checks() {
    let s = stubbed(vec![
        (PAGE, 200, PAGE_HTML),
        ("https://example.org/gone", 404, "not found"),
    ]);
    let found = s
        .submit(
            new_link(
                "Example Parents Network",
                "MONDO:0000001",
                Some(PAGE),
                Some("We support families living with Example encephalopathy"),
            ),
            None,
        )
        .unwrap();
    let found = s.run_checks(&found.id).await.unwrap();
    assert_eq!(report_code(&found, "evidence_url").1, "url_ok");
    assert_eq!(
        report_code(&found, "quote"),
        (CheckStatus::Pass, "quote_found".into(), false)
    );
    let fetched = &found.checks.as_ref().unwrap().fetched[0];
    assert_eq!(fetched.sha256.as_ref().unwrap().len(), 64);

    let wrong = s
        .submit(
            new_link(
                "Other Network",
                "MONDO:0000001",
                Some(PAGE),
                Some("families living with another disease"),
            ),
            None,
        )
        .unwrap();
    let wrong = s.run_checks(&wrong.id).await.unwrap();
    assert_eq!(
        report_code(&wrong, "quote"),
        (CheckStatus::Fail, "quote_not_found".into(), false)
    );

    let gone = s
        .submit(
            new_link(
                "Third Network",
                "MONDO:0000001",
                Some("https://example.org/gone"),
                Some("x"),
            ),
            None,
        )
        .unwrap();
    let gone = s.run_checks(&gone.id).await.unwrap();
    assert_eq!(
        report_code(&gone, "evidence_url"),
        (CheckStatus::Fail, "url_http_error".into(), false)
    );
    assert_eq!(report_code(&gone, "quote").1, "quote_unchecked");

    let down = s
        .submit(
            new_link(
                "Fourth Network",
                "MONDO:0000001",
                Some("https://unreachable.example/"),
                None,
            ),
            None,
        )
        .unwrap();
    let down = s.run_checks(&down.id).await.unwrap();
    assert_eq!(
        report_code(&down, "evidence_url"),
        (CheckStatus::Warn, "url_unreachable".into(), false)
    );
    assert_eq!(report_code(&down, "quote").1, "no_quote");
}

#[tokio::test]
async fn private_fetch_bypass_is_unavailable_in_library_builds() {
    use wiremock::MockServer;
    let server = MockServer::start().await;
    // Integration tests use the normal library build: even for_tests() cannot bypass SSRF checks.
    let s = service();
    let url = format!("{}/old", server.uri());
    let mut sub = new_link(
        "Example Parents Network",
        "MONDO:0000001",
        None,
        Some("support families living with Example encephalopathy"),
    );
    sub.evidence_url = Some(url.clone());
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!(report_code(&c, "evidence_url").1, "url_blocked");
    assert!(server.received_requests().await.unwrap().is_empty());

    // The production policy refuses the same local address, and that blocks accepting.
    let strict = atlas_contrib::Contrib::new(
        atlas_contrib::ContribConfig {
            fetch: atlas_contrib::FetchPolicy::default(),
            ..atlas_contrib::ContribConfig::for_tests()
        },
        std::sync::Arc::new(graph()),
    )
    .unwrap();
    let mut sub = new_link("Example Parents Network", "MONDO:0000001", None, None);
    sub.evidence_url = Some(format!("{}/about", server.uri()));
    let c = strict.submit(sub, None).unwrap();
    let c = strict.run_checks(&c.id).await.unwrap();
    assert_eq!(
        report_code(&c, "evidence_url"),
        (CheckStatus::Fail, "url_blocked".into(), true)
    );
    assert!(matches!(
        strict.review(&c.id, &accept("ok"), reviewer()),
        Err(ContribError::Blocked(_))
    ));
}

#[tokio::test]
async fn identity_resolution() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    // Existing group by exact name.
    let mut sub = new_link("example families group", "MONDO:0000001", Some(PAGE), None);
    sub.target = Some(NodeInput {
        id: None,
        label: Some("EXE".into()),
    });
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    let r = c.checks.as_ref().unwrap();
    assert_eq!(r.subject.as_ref().unwrap().id, "org:example-group");
    assert_eq!(r.target.as_ref().unwrap().id, "MONDO:0000001");
    assert_eq!(r.relation.as_deref(), Some("serves_condition"));

    // New group: allowed, flagged as new.
    let c = s
        .submit(
            new_link("Brand New Parents Network", "MONDO:0000001", Some(PAGE), None),
            None,
        )
        .unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!(
        report_code(&c, "subject"),
        (CheckStatus::Pass, "identity_new".into(), false)
    );

    // Ambiguous target by partial name; a new link then can't be accepted.
    let mut sub = new_link("Brand New Parents Network 2", "MONDO:0000001", Some(PAGE), None);
    sub.target = Some(NodeInput {
        id: None,
        label: Some("Example syndrome".into()),
    });
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    let (status, code, blocking) = report_code(&c, "target");
    assert_eq!(
        (status, code.as_str(), blocking),
        (CheckStatus::Fail, "identity_candidates", true)
    );
    assert!(matches!(
        s.review(&c.id, &accept("ok"), reviewer()),
        Err(ContribError::Blocked(_))
    ));

    // Kind mismatch: a gene id given as the subject of a registry link.
    let mut sub = new_link("x", "MONDO:0000001", Some(PAGE), None);
    sub.subject_kind = Some(SubjectKind::Registry);
    sub.subject = NodeInput {
        id: Some("HGNC:1".into()),
        label: None,
    };
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!(report_code(&c, "subject").1, "identity_kind_mismatch");
    // A gene target gives the gene relation.
    let mut sub = new_link("Gene Parents", "HGNC:1", Some(PAGE), None);
    sub.subject_kind = Some(SubjectKind::Study);
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!(c.checks.as_ref().unwrap().relation.as_deref(), Some("names_gene"));
}

// --- duplicates and conflicts ----------------------------------------------------------------------

#[tokio::test]
async fn duplicate_of_a_curated_edge_blocks_accepting() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    let mut sub = new_link("Example Families Group", "MONDO:0000001", Some(PAGE), None);
    sub.subject = NodeInput {
        id: Some("org:example-group".into()),
        label: None,
    };
    let c = s.submit(sub, None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    let (status, code, blocking) = report_code(&c, "duplicate");
    assert_eq!(
        (status, code.as_str(), blocking),
        (CheckStatus::Fail, "duplicate_curated", true)
    );
    assert_eq!(
        c.checks.as_ref().unwrap().find("duplicate").unwrap().detail["edge"],
        "org:example-group|serves_condition|MONDO:0000001"
    );
    let err = s.review(&c.id, &accept("ok"), reviewer()).unwrap_err();
    let check = c.checks.as_ref().unwrap().find("duplicate").unwrap();
    assert_eq!(check.message_msg["key"], "contribute.check.duplicate_curated");
    assert!(
        matches!(err, ContribError::Blocked(ref m) if m.contains(&check.message)),
        "{err}"
    );
    // Rejecting is still possible.
    assert_eq!(
        s.review(&c.id, &reject("already curated"), reviewer()).unwrap().state,
        State::Rejected
    );
}

#[tokio::test]
async fn duplicates_among_contributions() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    let first = s
        .submit(
            new_link("Brand New Parents Network", "MONDO:0000001", Some(PAGE), None),
            None,
        )
        .unwrap();
    let first = s.run_checks(&first.id).await.unwrap();
    assert_eq!(report_code(&first, "duplicate").1, "no_duplicate");

    // Same group (different spelling), same condition, while the first waits: warning.
    let second = s
        .submit(
            new_link("Brand-new parents network!", "MONDO:0000001", Some(PAGE), None),
            None,
        )
        .unwrap();
    let second = s.run_checks(&second.id).await.unwrap();
    let (status, code, blocking) = report_code(&second, "duplicate");
    assert_eq!(
        (status, code.as_str(), blocking),
        (CheckStatus::Warn, "duplicate_pending", false)
    );

    // Once the first is accepted, the second is a blocking duplicate.
    s.review(&first.id, &accept("website lists the condition"), reviewer())
        .unwrap();
    let second = s.run_checks(&second.id).await.unwrap();
    let (status, code, blocking) = report_code(&second, "duplicate");
    assert_eq!(
        (status, code.as_str(), blocking),
        (CheckStatus::Fail, "duplicate_contribution", true)
    );

    // A different condition is not a duplicate.
    let third = s
        .submit(
            new_link("Brand New Parents Network", "MONDO:0000002", Some(PAGE), None),
            None,
        )
        .unwrap();
    let third = s.run_checks(&third.id).await.unwrap();
    assert_eq!(report_code(&third, "duplicate").1, "no_duplicate");
}

#[tokio::test]
async fn conflicts_with_curated_edges_and_other_contributions() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    // A correction of a curated edge: the edge exists, the dispute is flagged for the reviewer.
    let c = s
        .submit(correction("org:example-group|serves_condition|MONDO:0000001"), None)
        .unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert_eq!(report_code(&c, "edge").1, "edge_found");
    assert_eq!(
        report_code(&c, "conflict"),
        (CheckStatus::Warn, "conflict_curated".into(), false)
    );
    // A second correction of the same edge: same report, not blocking.
    let c2 = s
        .submit(correction("org:example-group|serves_condition|MONDO:0000001"), None)
        .unwrap();
    let c2 = s.run_checks(&c2.id).await.unwrap();
    assert_eq!(report_code(&c2, "duplicate").1, "duplicate_pending");

    // A correction of an edge that doesn't exist can't be accepted.
    let ghost = s
        .submit(correction("org:none|serves_condition|MONDO:0000001"), None)
        .unwrap();
    let ghost = s.run_checks(&ghost.id).await.unwrap();
    assert_eq!(
        report_code(&ghost, "edge"),
        (CheckStatus::Fail, "edge_missing".into(), true)
    );

    // A new link that someone else disputes.
    let link = s
        .submit(
            new_link("Brand New Parents Network", "MONDO:0000001", Some(PAGE), None),
            None,
        )
        .unwrap();
    let link = s.run_checks(&link.id).await.unwrap();
    let asserted = format!("contrib:{}|serves_condition|MONDO:0000001", link.id);
    let dispute = s.submit(correction(&asserted), None).unwrap();
    let dispute = s.run_checks(&dispute.id).await.unwrap();
    assert_eq!(report_code(&dispute, "conflict").1, "conflict_contribution");
    let link = s.run_checks(&link.id).await.unwrap();
    assert_eq!(report_code(&link, "conflict").1, "conflict_contribution");
}

#[tokio::test]
async fn outdated_contact_compares_with_the_graph() {
    let s = stubbed(vec![
        ("https://example.org/contact", 200, "<p>Contact</p>"),
        ("https://example.net/new-contact", 200, "<p>New</p>"),
    ]);
    let mut sub = correction("a|b|c");
    sub.kind = ContributionKind::OutdatedContact;
    sub.edge = None;
    sub.subject = NodeInput {
        id: Some("org:example-group".into()),
        label: None,
    };
    sub.contact_url = Some("http://www.example.org/contact/".into());
    let same = s.submit(sub.clone(), None).unwrap();
    let same = s.run_checks(&same.id).await.unwrap();
    assert_eq!(report_code(&same, "conflict").1, "contact_same");
    sub.contact_url = Some("https://example.net/new-contact".into());
    let new = s.submit(sub, None).unwrap();
    let new = s.run_checks(&new.id).await.unwrap();
    assert_eq!(report_code(&new, "conflict").1, "contact_differs");
    assert_eq!(report_code(&new, "contact_url").1, "url_ok");
    assert_eq!(report_code(&new, "duplicate").1, "duplicate_pending");
}

// --- PROV and overlay --------------------------------------------------------------------------------

#[tokio::test]
async fn every_state_change_has_a_prov_record() {
    let s = stubbed(vec![(PAGE, 200, PAGE_HTML)]);
    let mut sub = new_link(
        "Brand New Parents Network",
        "MONDO:0000001",
        Some(PAGE),
        Some("We support families"),
    );
    sub.found_via.assistant = Some("Some Assistant".into());
    let c = s.submit(sub, None).unwrap();
    s.run_checks(&c.id).await.unwrap();
    let c = s
        .review(&c.id, &accept("confirmed on the website"), reviewer())
        .unwrap();

    let events = s.prov_events(&c.id).unwrap();
    let kinds: Vec<(&str, Option<State>, State, &str)> = events
        .iter()
        .map(|e| {
            (
                e.activity_type.as_str(),
                e.from_state,
                e.to_state,
                e.agent.kind.as_str(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("submission", None, State::Submitted, "prov:Person"),
            (
                "auto_check",
                Some(State::Submitted),
                State::AutoChecked,
                "prov:SoftwareAgent"
            ),
            ("review", Some(State::AutoChecked), State::Accepted, "prov:Person"),
        ]
    );
    assert_eq!(events[0].agent.id, format!("agent:anonymous/{}", c.id));
    assert_eq!(events[0].delegates[0].id, "agent:assistant/some-assistant");
    assert_eq!(events[1].agent.id, format!("agent:software/{}", atlas_contrib::AGENT));
    assert_eq!(events[2].agent.id, "agent:reviewer/tester");
    assert_eq!(events[2].note.as_deref(), Some("accepted: confirmed on the website"));
    // Versions chain: each activity used the previous version and generated the next.
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.generated, format!("contrib:{}/v{}", c.id, i + 1));
        if i > 0 {
            assert_eq!(e.used[0]["@id"], format!("contrib:{}/v{}", c.id, i));
        }
    }
    // The check used the fetched page, with its hash.
    assert!(
        events[1]
            .used
            .iter()
            .any(|u| u["@id"].as_str().unwrap().starts_with("page:sha256/") && u["prov:atLocation"] == PAGE)
    );

    let doc = s.prov_jsonld(&c.id).unwrap();
    assert_eq!(doc["@context"]["prov"], "http://www.w3.org/ns/prov#");
    let graph = doc["@graph"].as_array().unwrap();
    let v3 = graph
        .iter()
        .find(|n| n["@id"] == format!("contrib:{}/v3", c.id))
        .unwrap();
    assert_eq!(v3["prov:wasRevisionOf"]["@id"], format!("contrib:{}/v2", c.id));
    let v1 = graph
        .iter()
        .find(|n| n["@id"] == format!("contrib:{}/v1", c.id))
        .unwrap();
    assert_eq!(v1["prov:wasAttributedTo"]["@id"], format!("agent:anonymous/{}", c.id));
    let assistant = graph
        .iter()
        .find(|n| n["@id"] == "agent:assistant/some-assistant")
        .unwrap();
    assert_eq!(
        assistant["prov:actedOnBehalfOf"]["@id"],
        format!("agent:anonymous/{}", c.id)
    );
    // Private contact never appears in PROV.
    assert!(!doc.to_string().contains("parent@example.invalid"));
}

#[tokio::test]
async fn accepted_contributions_form_a_user_asserted_overlay() {
    let dir = std::env::temp_dir().join(format!("atlas-contrib-test-{}", std::process::id()));
    let path = dir.join("contrib-overlay.json");
    let config = atlas_contrib::ContribConfig {
        overlay_path: Some(path.clone()),
        ..atlas_contrib::ContribConfig::for_tests()
    };
    let s = atlas_contrib::Contrib::new(config, std::sync::Arc::new(graph()))
        .unwrap()
        .with_fetcher(std::sync::Arc::new(StubFetcher(vec![(PAGE, 200, PAGE_HTML)])));

    let mut sub = new_link(
        "Brand New Parents Network",
        "MONDO:0000001",
        Some(PAGE),
        Some("We support families"),
    );
    sub.contact_url = Some(PAGE.into());
    let link = s.submit(sub, None).unwrap();
    s.run_checks(&link.id).await.unwrap();
    s.review(&link.id, &accept("website confirms"), reviewer()).unwrap();

    let fix = s
        .submit(correction("NCT00000001|studies_condition|MONDO:0000001"), None)
        .unwrap();
    s.run_checks(&fix.id).await.unwrap();
    s.review(&fix.id, &accept("sponsor page says the study ended"), reviewer())
        .unwrap();

    let pending = s
        .submit(new_link("Still Pending Group", "MONDO:0000001", Some(PAGE), None), None)
        .unwrap();
    s.run_checks(&pending.id).await.unwrap();

    let overlay = Overlay::load(&path).unwrap();
    assert_eq!(overlay.nodes.len(), 1);
    assert_eq!(overlay.nodes[0].id, format!("contrib:{}", link.id));
    assert_eq!(overlay.nodes[0].kind, "organisation");
    assert_eq!(overlay.edges.len(), 1, "pending contributions stay out");
    let e = &overlay.edges[0];
    assert_eq!((e.kind.as_str(), e.level.as_str()), ("user_asserted", "user_asserted"));
    assert_eq!(e.id, format!("contrib:{}|serves_condition|MONDO:0000001", link.id));
    assert_eq!(e.evidence[0].quote_found, Some(true));
    assert_eq!(e.evidence[0].sha256.as_ref().map(String::len), Some(64));
    assert_eq!(e.prov, format!("contrib:{}/v3", link.id));
    assert_eq!(overlay.annotations.len(), 1);
    assert_eq!(overlay.annotations[0].kind, "correction");
    assert_eq!(
        overlay.annotations[0].about,
        "NCT00000001|studies_condition|MONDO:0000001"
    );
    assert_eq!(s.overlay().unwrap().edges, overlay.edges);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn persists_in_a_sqlite_file() {
    let dir = std::env::temp_dir().join(format!("atlas-contrib-db-{}", std::process::id()));
    let db = dir.join("contrib.sqlite");
    let config = || atlas_contrib::ContribConfig {
        database: atlas_contrib::Database::File(db.clone()),
        ..atlas_contrib::ContribConfig::for_tests()
    };
    let id = {
        let s = atlas_contrib::Contrib::new(config(), std::sync::Arc::new(graph())).unwrap();
        s.submit(
            new_link("Brand New Parents Network", "MONDO:0000001", Some(PAGE), None),
            None,
        )
        .unwrap()
        .id
    };
    let s = atlas_contrib::Contrib::new(config(), std::sync::Arc::new(graph())).unwrap();
    let c = s.get(&id).unwrap();
    assert_eq!(c.contributor.contact.as_deref(), Some("parent@example.invalid"));
    let listed = s
        .list(&atlas_contrib::ListFilter {
            node: Some("MONDO:0000001".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}
