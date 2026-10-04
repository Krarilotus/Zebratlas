//! HTTP API: submit, public views without private data, reviewer auth, overlay, rate limit.

mod common;

use std::sync::Arc;

use atlas_contrib::{ContribConfig, Limit, router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use common::*;

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-forwarded-for", "203.0.113.7");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn link_body() -> Value {
    json!({
        "kind": "new_link",
        "subject_kind": "patient_group",
        "subject": { "label": "Brand New Parents Network" },
        "target": { "id": "MONDO:0000001" },
        "statement": "They run family days for this condition.",
        "evidence_url": "https://example.org/about",
        "quote": "We support families living with Example encephalopathy",
        "found_via": { "page": "/en/c/MONDO:0000001", "assistant": "Some Assistant" },
        "contributor": { "name": "A parent", "contact": "parent@example.invalid" }
    })
}

#[tokio::test]
async fn data_source_api_preserves_reviewer_only_contact_and_stages_acceptance() {
    let state = stubbed(vec![(
        "https://example.org/data.json",
        200,
        r#"{"license":"MIT","openapi":"3.1.0","id":"MONDO:0000001"}"#,
    )])
    .into_state();
    let app = router(state.clone());
    let body = json!({"kind":"data_source", "contributor":{"name":"Example researcher", "organisation":"Example institution", "contact":"private@example.invalid"},
        "data_source":{"resource_kind":"api", "url":"https://example.org/data.json", "licence":"MIT (submitter claim)", "spdx_id":"MIT", "identifier_systems":["MONDO"], "description":"Placeholder API for tests only", "consent":true}});
    let (status, receipt) = call(&app, "POST", "/api/contribute", None, Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(!receipt.to_string().contains("private@example.invalid"));
    let id = receipt["id"].as_str().unwrap();
    let (_, public) = call(&app, "GET", &format!("/api/contribute/{id}"), None, None).await;
    assert!(!public.to_string().contains("private@example.invalid"));
    let (status, _) = call(&app, "GET", &format!("/api/review/{id}"), None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, full) = call(&app, "GET", &format!("/api/review/{id}"), Some("test-token"), None).await;
    assert_eq!(
        full["contribution"]["contributor"]["contact"],
        "private@example.invalid"
    );
    state.run_checks(id).await.unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/review/{id}"),
        Some("test-token"),
        Some(json!({"decision":"accept", "reason":"Test-only discovery approval."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, exported) = call(&app, "GET", "/api/contribute/discovery-candidates", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exported["candidates"].as_array().unwrap().len(), 1);
    assert!(!exported.to_string().contains("private@example.invalid"));
    assert_eq!(
        exported["candidates"][0]["checks"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "formats")
            .unwrap()["detail"]["detected"],
        json!(["JSON", "OpenAPI"])
    );
    let (_, overlay) = call(&app, "GET", "/api/contribute/overlay", None, None).await;
    assert!(overlay["nodes"].as_array().unwrap().is_empty());
    assert!(overlay["edges"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn submit_review_and_overlay_over_http() {
    let state = stubbed(vec![(
        "https://example.org/about",
        200,
        "<p>We support families living with Example encephalopathy.</p>",
    )])
    .into_state();
    let app = router(state.clone());

    let (status, body) = call(&app, "POST", "/api/contribute", None, Some(link_body())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["state"], "submitted");
    assert!(
        !body.to_string().contains("parent@example.invalid"),
        "contact must stay private"
    );

    // Checks run (in the server they start in the background; here explicitly).
    state.run_checks(&id).await.unwrap();
    let (status, public) = call(&app, "GET", &format!("/api/contribute/{id}"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public["state"], "auto_checked");
    assert_eq!(public["contributor"]["name"], "A parent");
    assert_eq!(public["contributor"]["signed_in"], false);
    assert!(!public.to_string().contains("parent@example.invalid"));
    let codes: Vec<&str> = public["checks"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"quote_found") && codes.contains(&"identity_new"),
        "{codes:?}"
    );

    // Listing by node.
    let (_, list) = call(&app, "GET", "/api/contribute?node=MONDO:0000001", None, None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // Review needs a reviewer.
    let decision = json!({ "decision": "accept", "reason": "Website confirms." });
    let (status, _) = call(&app, "POST", &format!("/api/review/{id}"), None, Some(decision.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/review/{id}"),
        Some("wrong-token"),
        Some(decision.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, me) = call(&app, "GET", "/api/review/me", Some("test-token"), None).await;
    assert_eq!(me["reviewer"], true);
    let (status, queue) = call(&app, "GET", "/api/review", Some("test-token"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        queue["items"][0]["contributor"]["contact"], "parent@example.invalid",
        "reviewers see the contact"
    );
    assert_eq!(queue["counts"]["auto_checked"], 1);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/review/{id}"),
        Some("test-token"),
        Some(json!({ "decision": "accept", "reason": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, done) = call(
        &app,
        "POST",
        &format!("/api/review/{id}"),
        Some("test-token"),
        Some(decision.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["contribution"]["state"], "accepted");
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/review/{id}"),
        Some("test-token"),
        Some(decision),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (_, overlay) = call(&app, "GET", "/api/contribute/overlay", None, None).await;
    assert_eq!(overlay["format"], "rare-atlas-contrib-overlay/1");
    assert_eq!(overlay["edges"][0]["kind"], "user_asserted");
    assert_eq!(overlay["edges"][0]["to"], "MONDO:0000001");

    let (_, prov) = call(&app, "GET", &format!("/api/contribute/{id}/prov"), None, None).await;
    assert!(prov["@graph"].as_array().unwrap().len() >= 7);
    let (_, public) = call(&app, "GET", &format!("/api/contribute/{id}"), None, None).await;
    assert_eq!(public["review"]["reason"], "Website confirms.");
    assert!(public["review"].get("reviewer").is_none());
}

#[tokio::test]
async fn invalid_input_and_unknown_ids() {
    let app = router(service().into_state());
    let (status, body) = call(
        &app,
        "POST",
        "/api/contribute",
        None,
        Some(json!({ "kind": "new_link", "statement": "x", "contributor": {"contact": "parent@example.invalid"} })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["detail"].as_str().unwrap().contains("subject"));
    let (status, _) = call(
        &app,
        "POST",
        "/api/contribute",
        None,
        Some(json!({ "kind": "nonsense" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&app, "GET", "/api/contribute/c_doesnotexist", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "GET", "/api/contribute?state=bogus", None, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn signed_in_contributors_and_reviewer_accounts() {
    let config = ContribConfig {
        reviewer_users: vec!["usr_reviewer".into()],
        ..ContribConfig::for_tests()
    };
    let state = atlas_contrib::Contrib::new(config, Arc::new(graph()))
        .unwrap()
        .with_user_resolver(Arc::new(|h: &axum::http::HeaderMap| {
            h.get("x-test-user")
                .and_then(|v| v.to_str().ok())
                .map(|id| atlas_contrib::UserRef {
                    id: id.into(),
                    name: Some(format!("User {id}")),
                    email: Some("account@example.invalid".into()),
                })
        }))
        .into_state();
    let app = router(state.clone());
    let mut signed_submission = link_body();
    signed_submission["contributor"]
        .as_object_mut()
        .unwrap()
        .remove("contact");
    let req = Request::builder()
        .method("POST")
        .uri("/api/contribute")
        .header("x-test-user", "usr_parent")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(signed_submission.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
    let id = body["id"].as_str().unwrap();
    let c = state.get(id).unwrap();
    assert_eq!(c.contributor.user_id.as_deref(), Some("usr_parent"));
    assert_eq!(c.contributor.contact.as_deref(), Some("account@example.invalid"));
    assert_eq!(state.prov_events(id).unwrap()[0].agent.id, "agent:user/usr_parent");
    assert_eq!(body["contribution"]["contributor"]["signed_in"], true);

    let me = |user: &'static str| {
        Request::builder()
            .uri("/api/review/me")
            .header("x-test-user", user)
            .body(Body::empty())
            .unwrap()
    };
    for (user, expected) in [("usr_parent", false), ("usr_reviewer", true)] {
        let res = app.clone().oneshot(me(user)).await.unwrap();
        let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(v["reviewer"], expected, "{user}");
    }
}

#[tokio::test]
async fn submissions_are_rate_limited_per_client() {
    let config = ContribConfig {
        submit_limit: Limit {
            max: 2,
            window: std::time::Duration::from_secs(60),
        },
        ..ContribConfig::for_tests()
    };
    let app = router(
        atlas_contrib::Contrib::new(config, Arc::new(graph()))
            .unwrap()
            .into_state(),
    );
    for _ in 0..2 {
        let (status, _) = call(&app, "POST", "/api/contribute", None, Some(link_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    let (status, body) = call(&app, "POST", "/api/contribute", None, Some(link_body())).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
}
