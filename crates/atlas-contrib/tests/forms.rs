//! D42: the form contract and validation agree, with no optional-field burden.
mod common;

use atlas_contrib::{ContributionKind, Submission, UserRef, router, schema};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn minimal(kind: ContributionKind) -> Value {
    let content = match kind {
        ContributionKind::NewLink => {
            json!({"subject": {"label": "Placeholder group"}, "target": {"id": "MONDO:0000001"}})
        }
        ContributionKind::Correction => {
            json!({"subject": {"id": "org:example-group"}, "statement": "Placeholder proposed change"})
        }
        ContributionKind::MissingEvidence => json!({"target": {"id": "MONDO:0000001"}}),
        ContributionKind::OutdatedContact => json!({"subject": {"id": "org:example-group"}}),
        ContributionKind::DataSource => {
            json!({"data_source": {"description": "Placeholder resource for reviewer investigation"}})
        }
        ContributionKind::Other => {
            json!({"kind_other": "Placeholder proposal category", "statement": "Placeholder proposal"})
        }
    };
    let mut v = content;
    v["kind"] = json!(kind);
    v["contributor"] = json!({"contact": "parent@example.invalid"});
    v
}

fn input(v: Value) -> Submission {
    serde_json::from_value(v).unwrap()
}

fn remove(v: &mut Value, path: &str) {
    let mut parts: Vec<_> = path.split('.').collect();
    let key = parts.pop().unwrap();
    let Some(parent) = parts.into_iter().try_fold(v, |v, p| v.get_mut(p)) else {
        return;
    };
    if let Some(parent) = parent.as_object_mut() {
        parent.remove(key);
    }
}

#[test]
fn every_kind_accepts_its_minimum_and_enforces_the_published_rules() {
    let doc = schema::document();
    let s = common::service();
    for kind in ContributionKind::ALL {
        let v = minimal(kind);
        let c = s.submit(input(v.clone()), None).unwrap();
        assert!(c.contributor.name.is_none());
        assert!(c.contributor.organisation.is_none());
        let rule = doc["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == json!(kind))
            .unwrap();
        for field in rule["required"].as_array().unwrap() {
            let field = field.as_str().unwrap();
            if field == "kind" {
                continue;
            } // serde enforces the discriminator.
            let mut invalid = v.clone();
            remove(&mut invalid, field);
            assert!(s.submit(input(invalid), None).is_err(), "{kind:?}: {field}");
        }
        for group in rule["required_any"].as_array().unwrap() {
            let mut invalid = v.clone();
            for field in group.as_array().unwrap() {
                remove(&mut invalid, field.as_str().unwrap());
            }
            assert!(s.submit(input(invalid), None).is_err(), "{kind:?}: {group}");
            for field in group.as_array().unwrap() {
                let mut alternative = v.clone();
                for field in group.as_array().unwrap() {
                    remove(&mut alternative, field.as_str().unwrap());
                }
                let path = field.as_str().unwrap();
                let value = if path == "edge" {
                    "a|b|c"
                } else if path.ends_with("url") {
                    "https://example.invalid/resource"
                } else {
                    "Placeholder"
                };
                let parts: Vec<_> = path.split('.').collect();
                if parts.len() == 1 {
                    alternative[parts[0]] = json!(value);
                } else {
                    alternative[parts[0]][parts[1]] = json!(value);
                }
                assert!(
                    s.submit(input(alternative), None).is_ok(),
                    "{kind:?}: alternative {path}"
                );
            }
        }
    }
}

#[test]
fn optional_evidence_classification_and_legacy_consent_are_optional() {
    let mut link = minimal(ContributionKind::NewLink);
    for kind in ["patient_group", "organisation", "registry", "study", "person"] {
        link["subject_kind"] = json!(kind);
        assert!(input(link.clone()).clean().is_ok(), "{kind}");
    }
    let mut source = minimal(ContributionKind::DataSource);
    source["data_source"] = json!({"url": "https://example.invalid/resource", "consent": false});
    let c = input(source.clone()).clean().unwrap();
    assert!(c.data_source.unwrap().resource_kind.is_none());
    source["data_source"]["url"] = json!("file:///private");
    assert!(input(source).clean().is_err());
    let mut correction = minimal(ContributionKind::Correction);
    correction["evidence_url"] = json!("javascript:alert(1)");
    assert!(input(correction).clean().is_err());
}

#[test]
fn contact_email_is_required_for_every_kind_and_comes_from_the_account() {
    let s = common::service();
    for kind in ContributionKind::ALL {
        for bad in ["", "invalid", "a@b", "a @example.invalid", "a@@example.invalid"] {
            let mut v = minimal(kind);
            v["contributor"]["contact"] = json!(bad);
            assert!(s.submit(input(v), None).is_err(), "{kind:?}: {bad}");
        }
        let mut v = minimal(kind);
        v["contributor"] = json!({});
        let user = UserRef {
            id: "usr_placeholder".into(),
            name: None,
            email: Some("account@example.invalid".into()),
        };
        let c = s.submit(input(v.clone()), Some(user)).unwrap();
        assert_eq!(c.contributor.contact.as_deref(), Some("account@example.invalid"));
        assert!(!c.public().to_string().contains("account@example.invalid"));
        assert!(
            s.submit(
                input(v),
                Some(UserRef {
                    id: "usr_placeholder".into(),
                    name: None,
                    email: None
                })
            )
            .is_err()
        );
    }
    let c = s
        .submit(
            input(minimal(ContributionKind::DataSource)),
            Some(UserRef {
                id: "usr_placeholder".into(),
                name: None,
                email: Some("account@example.invalid".into()),
            }),
        )
        .unwrap();
    assert_eq!(c.contributor.contact.as_deref(), Some("account@example.invalid"));
}

#[test]
fn every_choice_accepts_bounded_other_text_and_rejects_missing_or_orphan_text() {
    for (kind, path, companion) in [
        (ContributionKind::Other, "kind", "kind_other"),
        (ContributionKind::NewLink, "subject_kind", "subject_kind_other"),
        (ContributionKind::NewLink, "relationship", "relationship_other"),
        (
            ContributionKind::DataSource,
            "data_source.resource_kind",
            "data_source.resource_kind_other",
        ),
        (
            ContributionKind::DataSource,
            "data_source.identifier_systems",
            "data_source.identifier_systems_other",
        ),
    ] {
        let set = |v: &mut Value, p: &str, value: Value| {
            let parts: Vec<_> = p.split('.').collect();
            if parts.len() == 1 {
                v[parts[0]] = value;
            } else {
                v[parts[0]][parts[1]] = value;
            }
        };
        let mut v = minimal(kind);
        let selection = if path.ends_with("identifier_systems") {
            json!(["MONDO", "other"])
        } else {
            json!("other")
        };
        set(&mut v, path, selection);
        remove(&mut v, companion);
        assert!(input(v.clone()).clean().is_err(), "{path}");
        set(&mut v, companion, json!("Placeholder custom value"));
        let cleaned = input(v.clone()).clean().unwrap();
        assert!(
            serde_json::to_string(&cleaned)
                .unwrap()
                .contains("Placeholder custom value")
        );
        assert_eq!(cleaned.relation(Some("gene")), None);
        set(&mut v, companion, json!("ö".repeat(300)));
        assert!(input(v.clone()).clean().is_ok());
        set(&mut v, companion, json!("ö".repeat(301)));
        assert!(input(v.clone()).clean().is_err());
        set(&mut v, companion, json!("Placeholder custom value"));
        let standard = match path {
            "kind" => json!("new_link"),
            "subject_kind" => json!("study"),
            "relationship" => json!("studies_condition"),
            "data_source.resource_kind" => json!("dataset"),
            _ => json!(["MONDO"]),
        };
        set(&mut v, path, standard);
        assert!(input(v).clean().is_err());
    }
    // Legacy OTHER is accepted and preserved as the canonical other choice with text.
    let mut v = minimal(ContributionKind::DataSource);
    v["data_source"]["identifier_systems"] = json!(["OTHER"]);
    v["data_source"]["identifier_systems_other"] = json!("Placeholder custom IDs");
    assert!(input(v).clean().is_ok());
}

#[tokio::test]
async fn schema_is_public_and_custom_values_reach_authenticated_review() {
    let state = common::service().into_state();
    let app = router(state);
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/contribute/schema")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let doc: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(doc, schema::document());
    let mut v = minimal(ContributionKind::DataSource);
    v["data_source"]["resource_kind"] = json!("other");
    v["data_source"]["resource_kind_other"] = json!("Placeholder custom asset");
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/contribute")
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let receipt: Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    let uri = format!("/api/review/{}", receipt["id"].as_str().unwrap());
    let res = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let reviewed: Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(
        reviewed["contribution"]["submission"]["data_source"]["resource_kind_other"],
        "Placeholder custom asset"
    );
}

#[tokio::test]
async fn description_only_checks_do_not_fetch_and_custom_links_are_never_mapped() {
    let s = common::stubbed(vec![]);
    let c = s.submit(input(minimal(ContributionKind::DataSource)), None).unwrap();
    let c = s.run_checks(&c.id).await.unwrap();
    assert!(c.checks.as_ref().unwrap().fetched.is_empty());
    assert_eq!(c.checks.unwrap().find("resource_url").unwrap().code, "no_url");
    for custom in ["subject_kind", "relationship"] {
        let mut v = minimal(ContributionKind::NewLink);
        v["subject_kind"] = json!("patient_group");
        v[custom] = json!("other");
        v[format!("{custom}_other")] = json!("Placeholder custom connection");
        let c = s.submit(input(v), None).unwrap();
        let c = s.run_checks(&c.id).await.unwrap();
        assert_eq!(c.checks.unwrap().relation, None);
        let c = s
            .review(
                &c.id,
                &atlas_contrib::model::ReviewInput {
                    decision: atlas_contrib::model::Decision::Accept,
                    reason: "Retain unmapped for investigation".into(),
                },
                common::reviewer(),
            )
            .unwrap();
        let overlay = s.overlay().unwrap();
        assert!(overlay.edges.is_empty());
        assert!(overlay.annotations.iter().any(|a| a.contribution == c.id));
    }
}
