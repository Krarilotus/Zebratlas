//! D41/D42: saved documents (encrypted), "your data" view and export, account deletion with the
//! contribution hooks. Synthetic content only.

use std::sync::{Arc, Mutex};

use atlas_accounts::{
    AccountsConfig, AccountsExtras, AnonymiseReport, ContributionHooks, Database, DocumentContent, DocumentMeta,
    DocumentVault, Kek, try_router_with,
};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

const PASSWORD: &str = "a long enough passphrase";

#[derive(Default)]
struct FakeContrib {
    calls: Mutex<Vec<(String, bool)>>,
}

impl ContributionHooks for FakeContrib {
    fn export_for_user(&self, user_id: &str) -> Result<Value, String> {
        Ok(json!([{ "id": "c1", "state": "accepted", "user": user_id }]))
    }
    fn anonymise_for_user(&self, user_id: &str, keep_credit: bool) -> Result<AnonymiseReport, String> {
        self.calls.lock().unwrap().push((user_id.to_owned(), keep_credit));
        Ok(AnonymiseReport {
            accepted_kept: 1,
            personal_fields_removed: 1,
        })
    }
}

fn kek() -> Kek {
    Kek::parse(&"5a".repeat(32)).unwrap()
}

fn db_path(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-accounts-{name}-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("x-atlas-csrf", "1")
        .header("x-forwarded-for", "203.0.113.9");
    if !cookie.is_empty() {
        req = req.header(header::COOKIE, cookie);
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, v)
}

async fn signup(app: &Router, email: &str) -> String {
    let (s, h, _) = call(
        app,
        "POST",
        "/api/account/signup",
        "",
        Some(json!({ "email": email, "password": PASSWORD })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    h[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn setup(name: &str) -> (Router, DocumentVault, Arc<FakeContrib>) {
    let config = AccountsConfig::for_tests().with_database(Database::File(db_path(name)));
    let hooks = Arc::new(FakeContrib::default());
    let app = try_router_with(
        config.clone(),
        AccountsExtras {
            hooks: Some(hooks.clone()),
            kek: Some(kek()),
        },
    )
    .unwrap();
    let vault = DocumentVault::open_with(&config, Some(kek())).unwrap();
    (app, vault, hooks)
}

fn save(vault: &DocumentVault, cookie: &str) -> String {
    let mut h = HeaderMap::new();
    h.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
    let user = vault.user(&h).unwrap().expect("signed in");
    vault
        .save(
            &user.id,
            &DocumentMeta {
                format: "txt".into(),
                bytes: 42,
                sha256: "ab".repeat(32),
            },
            &DocumentContent {
                title: "Synthetic letter".into(),
                text: "[NAME] has seizures; STXBP1 c.1162C>T".into(),
                terms: json!([{ "kind": "gene", "text": "STXBP1" }]),
            },
        )
        .unwrap()
        .id
}

#[tokio::test]
async fn documents_can_be_listed_read_exported_and_deleted_by_their_owner_only() {
    let (app, vault, _) = setup("docs");
    let maria = signup(&app, "maria@example.org").await;
    let other = signup(&app, "other@example.org").await;
    let id = save(&vault, &maria);

    let (s, _, v) = call(&app, "GET", "/api/account/documents", &maria, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["documents"][0]["title"], "Synthetic letter");
    assert_eq!(v["documents"][0]["terms"], 1);

    let path = format!("/api/account/documents/{id}");
    let (s, _, v) = call(&app, "GET", &path, &maria, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["text"].as_str().unwrap().contains("STXBP1"));
    // nobody else can read or delete it
    assert_eq!(call(&app, "GET", &path, &other, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "DELETE", &path, &other, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", &path, "", None).await.0, StatusCode::UNAUTHORIZED);

    let (_, _, data) = call(&app, "GET", "/api/account/data", &maria, None).await;
    assert_eq!(data["documents"].as_array().unwrap().len(), 1);
    assert_eq!(data["contributions"]["available"], true);
    let (s, h, export) = call(&app, "GET", "/api/account/export", &maria, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h[header::CONTENT_DISPOSITION].to_str().unwrap().contains("attachment"));
    assert!(export["documents"][0]["text"].as_str().unwrap().contains("c.1162C>T"));
    assert_eq!(export["contributions"]["items"][0]["id"], "c1");

    assert_eq!(
        call(&app, "DELETE", &path, &maria, None).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, _, v) = call(&app, "GET", "/api/account/documents", &maria, None).await;
    assert_eq!(v["documents"], json!([]));
}

#[tokio::test]
async fn account_deletion_removes_documents_and_key_and_anonymises_contributions() {
    let (app, vault, hooks) = setup("delete");
    let maria = signup(&app, "maria@example.org").await;
    save(&vault, &maria);
    save(&vault, &maria);

    let (s, h, v) = call(
        &app,
        "DELETE",
        "/api/account/me",
        &maria,
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(h[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
    assert_eq!(v["deleted"]["documents"], 2);
    assert_eq!(v["deleted"]["data_key"], true);
    assert_eq!(v["contributions"]["status"], "anonymised");
    let keys: Vec<&str> = v["summary"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"account.deleted.documents") && keys.contains(&"account.deleted.contributions_anonymised"));
    // keep_credit defaults to false (unticked)
    assert!(!hooks.calls.lock().unwrap()[0].1);
    // the session is gone
    assert_eq!(
        call(&app, "GET", "/api/account/documents", &maria, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn keep_credit_is_passed_through_when_chosen() {
    let (app, _, hooks) = setup("credit");
    let maria = signup(&app, "maria@example.org").await;
    let (s, _, v) = call(
        &app,
        "DELETE",
        "/api/account/me",
        &maria,
        Some(json!({ "password": PASSWORD, "keep_credit": true })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["contributions"]["status"], "kept_with_name");
    assert!(hooks.calls.lock().unwrap()[0].1);
}
