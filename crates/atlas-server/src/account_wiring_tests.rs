use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

async fn call(app: &Router, method: &str, path: &str, cookie: &str, body: Value) -> (StatusCode, String, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("x-atlas-csrf", "1")
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .header("x-forwarded-for", "203.0.113.19")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, cookie, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn real_services_export_credit_choice_and_session_revocation() {
    for keep_credit in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AccountsConfig::for_tests()
            .with_database(atlas_accounts::Database::File(dir.path().join("accounts.sqlite")));
        let cc = ContribConfig {
            database: atlas_contrib::store::Database::InMemory,
            overlay_path: Some(dir.path().join("overlay.json")),
            discovery_candidates_path: None,
            check_on_submit: false,
            ..Default::default()
        };
        let service = Contrib::new(cc, Arc::new(atlas_contrib::graph::NoGraph)).unwrap();
        let (app, c) = mount_services(cfg, AccountsExtras::default(), Ok(service));
        let app = deletion_gate(app);
        let c = c.unwrap();
        let (status,cookie,user)=call(&app,"POST","/api/account/signup","",json!({"email":"synthetic@example.invalid","password":"synthetic long password","display_name":"Synthetic Contributor"})).await;
        assert_eq!(status, StatusCode::CREATED);
        let (status,_,sub)=call(&app,"POST","/api/contribute",&cookie,json!({"kind":"other","kind_other":"Synthetic test","statement":"Synthetic proposed connection for a test."})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{sub}");
        // The account e-mail fills the reviewer-only contact, with no contact supplied in JSON.
        let id = sub["contribution"]["id"]
            .as_str()
            .or_else(|| sub["id"].as_str())
            .unwrap();
        let mut accepted = c.get(id).unwrap();
        assert_eq!(
            accepted.contributor.contact.as_deref(),
            Some("synthetic@example.invalid")
        );
        let uid = accepted.contributor.user_id.clone().unwrap();
        assert!(user.to_string().contains(&uid));
        // Fixture accepted state, no external fetch or clinical assertion.
        let before = accepted.clone();
        accepted.state = atlas_contrib::model::State::Accepted;
        accepted.version += 1;
        let ev = atlas_contrib::prov::ProvEvent::new(
            "synthetic_fixture",
            atlas_contrib::model::Agent::software(),
            Some(&before),
            &accepted,
            "2026-10-04T00:00:00Z".into(),
        );
        c.store().update(&accepted, before.version, &ev).unwrap();
        let (status, _, export) = call(&app, "GET", "/api/account/export", &cookie, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            export["contributions"]["items"]["contributions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let (status, _, deleted) = call(
            &app,
            "DELETE",
            "/api/account/me",
            &cookie,
            json!({"password":"synthetic long password","keep_credit":keep_credit}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{deleted}");
        let remaining = c.get(id).unwrap();
        assert!(remaining.contributor.user_id.is_none());
        assert!(remaining.contributor.contact.is_none());
        assert_eq!(remaining.contributor.name.is_some(), keep_credit);
        assert_eq!(remaining.state, atlas_contrib::model::State::Accepted);
        assert_eq!(deleted["contributions"]["accepted_kept"], 1);
        assert_eq!(deleted["contributions"]["personal_fields_removed"], 1);
        let history = c.store().prov(id).unwrap();
        let history = serde_json::to_string(&history).unwrap();
        assert!(!history.contains("synthetic@example.invalid"));
        assert!(!history.contains(&uid));
        let overlay = std::fs::read_to_string(dir.path().join("overlay.json")).unwrap();
        assert!(!overlay.contains("synthetic@example.invalid"));
        assert!(!overlay.contains(&uid));
        let (status, _, _) = call(&app, "GET", "/api/account/me", &cookie, Value::Null).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn unavailable_contributions_blocks_partial_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let cfg =
        AccountsConfig::for_tests().with_database(atlas_accounts::Database::File(dir.path().join("accounts.sqlite")));
    let (app, _) = mount_services(
        cfg,
        AccountsExtras::default(),
        Err(atlas_contrib::error::ContribError::invalid("synthetic unavailable")),
    );
    let (_, cookie, _) = call(
        &app,
        "POST",
        "/api/account/signup",
        "",
        json!({"email":"test@example.invalid","password":"synthetic long password"}),
    )
    .await;
    let (status, _, _) = call(
        &app,
        "DELETE",
        "/api/account/me",
        &cookie,
        json!({"password":"synthetic long password"}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _, _) = call(&app, "GET", "/api/account/me", &cookie, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
}
