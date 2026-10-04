//! D42 deletion hooks preserve accepted facts while removing personal attribution.
mod common;

use atlas_contrib::model::{Agent, Decision, ReviewInput};
use atlas_contrib::{Contrib, ContribConfig, Contribution, Database, State, Submission, UserRef};
use serde_json::json;
use std::sync::Arc;

fn user(id: &str) -> UserRef {
    UserRef {
        id: id.into(),
        name: Some("Placeholder Private Name".into()),
        email: Some("private@example.invalid".into()),
    }
}

fn submit(s: &Contrib, id: &str, label: &str) -> Contribution {
    let mut sub = common::new_link(label, "MONDO:0000001", None, None);
    sub.contributor.name = None;
    sub.contributor.organisation = Some("Placeholder private affiliation".into());
    s.submit(sub, Some(user(id))).unwrap()
}

fn decision(decision: Decision) -> ReviewInput {
    ReviewInput {
        decision,
        reason: "Placeholder reviewer decision".into(),
    }
}

async fn accept(s: &Contrib, c: &Contribution) -> Contribution {
    s.run_checks(&c.id).await.unwrap();
    s.review(&c.id, &decision(Decision::Accept), common::reviewer())
        .unwrap()
}

#[tokio::test]
async fn export_includes_all_states_private_fields_history_and_only_the_requested_user() {
    let s = common::stubbed(vec![]);
    let pending = submit(&s, "usr_private", "Placeholder pending");
    let checked = submit(&s, "usr_private", "Placeholder checked");
    s.run_checks(&checked.id).await.unwrap();
    let accepted = submit(&s, "usr_private", "Placeholder accepted");
    accept(&s, &accepted).await;
    let rejected = submit(&s, "usr_private", "Placeholder rejected");
    s.review(&rejected.id, &decision(Decision::Reject), common::reviewer())
        .unwrap();
    let unrelated = submit(&s, "usr_unrelated", "Placeholder unrelated");
    let export = s.export_for_user("usr_private").unwrap();
    let items = export["contributions"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    for state in State::ALL {
        assert!(items.iter().any(|c| c["contribution"]["state"] == json!(state)));
    }
    assert!(items.iter().any(|c| c["contribution"]["id"] == pending.id));
    assert!(!export.to_string().contains(&unrelated.id));
    assert!(
        items
            .iter()
            .all(|c| c["contribution"]["contributor"]["contact"] == "private@example.invalid")
    );
    assert!(items.iter().all(|c| !c["history"].as_array().unwrap().is_empty()));
    assert_eq!(s.export_for_user("usr_unknown").unwrap()["contributions"], json!([]));
}

#[tokio::test]
async fn anonymisation_covers_all_states_history_indexes_and_stale_writers() {
    let s = common::stubbed(vec![]);
    let pending = submit(&s, "usr_private", "Placeholder pending");
    let checked = submit(&s, "usr_private", "Placeholder checked");
    s.run_checks(&checked.id).await.unwrap();
    let accepted = submit(&s, "usr_private", "Placeholder accepted");
    let accepted = accept(&s, &accepted).await;
    let rejected = submit(&s, "usr_private", "Placeholder rejected");
    s.review(&rejected.id, &decision(Decision::Reject), common::reviewer())
        .unwrap();
    let unrelated = submit(&s, "usr_unrelated", "Placeholder unrelated");
    let before = s.overlay().unwrap().edges[0].clone();
    assert_eq!(s.anonymise_for_user("usr_private", false).unwrap(), 4);
    for id in [&pending.id, &checked.id, &accepted.id, &rejected.id] {
        let c = s.get(id).unwrap();
        assert!(c.contributor.user_id.is_none());
        assert!(c.contributor.contact.is_none());
        assert!(c.contributor.name.is_none());
        assert!(c.contributor.organisation.is_none());
        let history = s.prov_events(id).unwrap();
        let event = history.last().unwrap();
        assert_eq!(event.activity_type, "account_anonymisation");
        assert_eq!(event.agent.kind, "prov:SoftwareAgent");
        assert_eq!(event.from_state, Some(c.state));
        assert_eq!(event.to_state, c.state);
        assert_eq!(event.generated_sha256.as_deref().unwrap().len(), 64);
        let stored = json!({"contribution": c, "history": history, "prov": s.prov_jsonld(id).unwrap()}).to_string();
        for personal in [
            "usr_private",
            "private@example.invalid",
            "Placeholder Private Name",
            "Placeholder private affiliation",
        ] {
            assert!(!stored.contains(personal), "{personal} leaked for {id}");
        }
    }
    let after = &s.overlay().unwrap().edges[0];
    assert_eq!(
        (&before.id, &before.from, &before.to, &before.relation),
        (&after.id, &after.from, &after.to, &after.relation)
    );
    assert_eq!(
        after.attribution.as_ref().unwrap().label.as_deref(),
        Some("anonymous contributor")
    );
    assert_eq!(s.get(&accepted.id).unwrap().state, State::Accepted);
    assert_eq!(s.get(&unrelated.id).unwrap(), unrelated);
    assert_eq!(s.export_for_user("usr_private").unwrap()["contributions"], json!([]));
    let ev = atlas_contrib::prov::ProvEvent::new(
        "stale",
        Agent::software(),
        Some(&pending),
        &pending,
        pending.updated_at.clone(),
    );
    assert!(matches!(
        s.store().update(&pending, pending.version, &ev),
        Err(atlas_contrib::ContribError::Stale)
    ));
    assert_eq!(s.anonymise_for_user("usr_private", false).unwrap(), 0);
}

#[tokio::test]
async fn keep_credit_is_limited_to_accepted_names_and_never_keeps_contact_or_account() {
    let s = common::stubbed(vec![]);
    let accepted = submit(&s, "usr_credit", "Placeholder accepted");
    accept(&s, &accepted).await;
    let pending = submit(&s, "usr_credit", "Placeholder pending");
    let rejected = submit(&s, "usr_credit", "Placeholder rejected");
    s.review(&rejected.id, &decision(Decision::Reject), common::reviewer())
        .unwrap();
    assert_eq!(s.anonymise_for_user("usr_credit", true).unwrap(), 3);
    assert_eq!(
        s.get(&accepted.id).unwrap().contributor.name.as_deref(),
        Some("Placeholder Private Name")
    );
    assert_eq!(s.get(&pending.id).unwrap().contributor.name, None);
    assert_eq!(s.get(&rejected.id).unwrap().contributor.name, None);
    assert_eq!(
        s.overlay().unwrap().edges[0]
            .attribution
            .as_ref()
            .unwrap()
            .label
            .as_deref(),
        Some("Placeholder Private Name")
    );
    for id in [&accepted.id, &pending.id, &rejected.id] {
        let history = s.prov_jsonld(id).unwrap().to_string();
        let c = s.get(id).unwrap();
        assert!(c.contributor.user_id.is_none());
        assert!(c.contributor.contact.is_none());
        assert!(!history.contains("usr_credit"));
        assert!(!history.contains("private@example.invalid"));
        let activity = serde_json::to_string(s.prov_events(id).unwrap().last().unwrap()).unwrap();
        assert!(!activity.contains("Placeholder Private Name"));
    }
}

#[tokio::test]
async fn deletion_removes_reviewer_account_links_from_other_peoples_history() {
    let s = common::stubbed(vec![]);
    let c = submit(&s, "usr_unrelated", "Placeholder unrelated");
    s.run_checks(&c.id).await.unwrap();
    s.review(
        &c.id,
        &decision(Decision::Accept),
        Agent::person("agent:user/usr_reviewer", Some("Placeholder Reviewer Name".into())),
    )
    .unwrap();
    assert_eq!(s.anonymise_for_user("usr_reviewer", false).unwrap(), 0);
    let stored = json!({"contribution": s.get(&c.id).unwrap(), "prov": s.prov_jsonld(&c.id).unwrap()}).to_string();
    assert!(!stored.contains("usr_reviewer"));
    assert!(!stored.contains("Placeholder Reviewer Name"));
    assert_eq!(
        s.get(&c.id).unwrap().contributor.user_id.as_deref(),
        Some("usr_unrelated")
    );
}

#[test]
fn export_is_not_limited_by_public_queue_pagination() {
    let s = common::service();
    for _ in 0..1001 {
        submit(&s, "usr_many", "Placeholder contribution");
    }
    assert_eq!(
        s.export_for_user("usr_many").unwrap()["contributions"]
            .as_array()
            .unwrap()
            .len(),
        1001
    );
}

#[tokio::test]
async fn file_exports_are_rebuilt_and_the_database_stays_anonymous_after_reopening() {
    let dir = std::env::temp_dir().join(format!(
        "atlas-contrib-d42-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = ContribConfig {
        database: Database::File(dir.join("contrib.sqlite")),
        overlay_path: Some(dir.join("overlay.json")),
        discovery_candidates_path: Some(dir.join("candidates.json")),
        ..ContribConfig::for_tests()
    };
    let s = Contrib::new(config.clone(), Arc::new(common::graph()))
        .unwrap()
        .with_fetcher(Arc::new(common::StubFetcher(vec![])));
    let link = submit(&s, "usr_files", "Placeholder accepted link");
    accept(&s, &link).await;
    let source: Submission = serde_json::from_value(json!({"kind": "data_source", "data_source": {
        "description": "Placeholder source needing manual investigation"}}))
    .unwrap();
    let source = s.submit(source, Some(user("usr_files"))).unwrap();
    accept(&s, &source).await;
    for filename in ["overlay.json", "candidates.json"] {
        assert!(
            std::fs::read_to_string(dir.join(filename))
                .unwrap()
                .contains("Placeholder Private Name")
        );
    }
    s.anonymise_for_user("usr_files", false).unwrap();
    for filename in ["overlay.json", "candidates.json"] {
        let text = std::fs::read_to_string(dir.join(filename)).unwrap();
        assert!(!text.contains("usr_files"));
        assert!(!text.contains("Placeholder Private Name"));
        assert!(!text.contains("private@example.invalid"));
        assert!(text.contains("anonymous contributor"));
    }
    drop(s);
    let database_bytes = std::fs::read(dir.join("contrib.sqlite")).unwrap();
    for personal in ["usr_files", "private@example.invalid", "Placeholder Private Name"] {
        assert!(
            !database_bytes
                .windows(personal.len())
                .any(|bytes| bytes == personal.as_bytes())
        );
    }
    let reopened = Contrib::new(config, Arc::new(common::graph())).unwrap();
    assert!(reopened.get(&source.id).unwrap().contributor.user_id.is_none());
    assert_eq!(
        reopened.export_for_user("usr_files").unwrap()["contributions"],
        json!([])
    );
    drop(reopened);
    for filename in [
        "contrib.sqlite",
        "contrib.sqlite-shm",
        "contrib.sqlite-wal",
        "overlay.json",
        "candidates.json",
    ] {
        let path = dir.join(filename);
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    std::fs::remove_dir(dir).unwrap();
}

#[tokio::test]
async fn retry_after_failed_public_export_does_not_restore_personal_fields() {
    let dir = std::env::temp_dir().join(format!(
        "atlas-contrib-d42-retry-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let blocker = dir.join("blocked-parent");
    std::fs::write(&blocker, "placeholder blocking file").unwrap();
    let overlay = blocker.join("overlay.json");
    let s = Contrib::new(
        ContribConfig {
            overlay_path: Some(overlay.clone()),
            ..ContribConfig::for_tests()
        },
        Arc::new(common::graph()),
    )
    .unwrap()
    .with_fetcher(Arc::new(common::StubFetcher(vec![])));
    let c = submit(&s, "usr_retry", "Placeholder accepted link");
    s.run_checks(&c.id).await.unwrap();
    assert!(
        s.review(&c.id, &decision(Decision::Accept), common::reviewer())
            .is_err()
    );
    assert_eq!(s.get(&c.id).unwrap().state, State::Accepted);
    assert!(s.anonymise_for_user("usr_retry", false).is_err());
    assert!(s.get(&c.id).unwrap().contributor.user_id.is_none());
    std::fs::remove_file(&blocker).unwrap();
    std::fs::create_dir(&blocker).unwrap();
    assert_eq!(s.anonymise_for_user("usr_retry", false).unwrap(), 0);
    let text = std::fs::read_to_string(&overlay).unwrap();
    assert!(!text.contains("Placeholder Private Name"));
    assert!(text.contains("anonymous contributor"));
    std::fs::remove_file(overlay).unwrap();
    std::fs::remove_dir(blocker).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
