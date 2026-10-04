//! End-to-end tests of `/api/account/*` against an in-memory database.

use std::time::Duration;

use atlas_accounts::{AccountsConfig, Database, Limit, router, try_router};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

const PASSWORD: &str = "a long enough passphrase";

struct Resp {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

struct Client {
    app: Router,
    cookie: Option<String>,
    forwarded_for: String,
}

impl Client {
    fn new(app: &Router) -> Self {
        Self {
            app: app.clone(),
            cookie: None,
            forwarded_for: "203.0.113.7".into(),
        }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> Resp {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("x-forwarded-for", &self.forwarded_for)
            .header("x-atlas-csrf", "1");
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("lang=de; {c}"));
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        if let Some(set) = headers.get(header::SET_COOKIE).and_then(|v| v.to_str().ok()) {
            let pair = set.split(';').next().unwrap().to_string();
            self.cookie = if pair.ends_with('=') { None } else { Some(pair) };
        }
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        Resp { status, headers, body }
    }

    async fn get(&mut self, path: &str) -> Resp {
        self.call("GET", path, None).await
    }

    async fn post(&mut self, path: &str, body: Value) -> Resp {
        self.call("POST", path, Some(body)).await
    }

    async fn signup(&mut self, email: &str) -> Resp {
        self.post(
            "/api/account/signup",
            json!({ "email": email, "password": PASSWORD, "display_name": "Maria", "locale": "de" }),
        )
        .await
    }
}

fn app() -> Router {
    try_router(AccountsConfig::for_tests()).expect("router")
}

#[tokio::test]
async fn status_is_public_and_account_routes_need_a_session() {
    let app = app();
    let mut c = Client::new(&app);
    let r = c.get("/api/account/status").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["enabled"], true);
    assert_eq!(r.headers[header::CACHE_CONTROL], "no-store");
    for path in [
        "/api/account/me",
        "/api/account/saved",
        "/api/account/conversations",
        "/api/account/export",
    ] {
        let r = c.get(path).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(r.body["code"], "unauthorized");
    }
    assert_eq!(c.get("/api/account/nope").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn signup_sets_a_secure_http_only_cookie_and_signs_in() {
    let app = app();
    let mut c = Client::new(&app);
    let r = c.signup("Maria@Example.org").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let set = r.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set.starts_with("__Host-atlas_session="));
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax") && set.contains("Secure"));
    assert_eq!(r.body["user"]["email"], "maria@example.org");
    assert_eq!(r.body["plan"], "free");
    // The token is only in the cookie, never in the body.
    let token = set
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("__Host-atlas_session=");
    assert!(!r.body.to_string().contains(token));

    let me = c.get("/api/account/me").await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["user"]["display_name"], "Maria");
    assert_eq!(me.body["user"]["locale"], "de");

    // Same email again, any case: conflict.
    let mut other = Client::new(&app);
    assert_eq!(other.signup("MARIA@example.org").await.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn signup_validates_email_and_password() {
    let app = app();
    let mut c = Client::new(&app);
    let r = c
        .post(
            "/api/account/signup",
            json!({ "email": "not-an-email", "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = c
        .post("/api/account/signup", json!({ "email": "a@b.de", "password": "short" }))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!r.body.to_string().contains("short"), "errors don't echo the password");
    assert!(c.cookie.is_none());
}

#[tokio::test]
async fn login_logout_and_bearer_tokens() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("devon@example.org").await;
    assert_eq!(
        c.post("/api/account/logout", json!({})).await.status,
        StatusCode::NO_CONTENT
    );
    assert!(c.cookie.is_none());
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);

    let bad = c
        .post(
            "/api/account/login",
            json!({ "email": "devon@example.org", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad.body["code"], "bad_credentials");
    let unknown = c
        .post(
            "/api/account/login",
            json!({ "email": "nobody@example.org", "password": PASSWORD }),
        )
        .await;
    assert_eq!(unknown.body, bad.body, "unknown email and wrong password look the same");

    let ok = c
        .post(
            "/api/account/login",
            json!({ "email": " DEVON@example.org ", "password": PASSWORD }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    let token = c
        .cookie
        .clone()
        .unwrap()
        .trim_start_matches("__Host-atlas_session=")
        .to_string();

    // The same token works as a bearer token for API clients.
    let req = Request::get("/api/account/me")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

    // A logged-out token is dead even if replayed.
    c.post("/api/account/logout", json!({})).await;
    c.cookie = Some(format!("__Host-atlas_session={token}"));
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_is_rate_limited_per_account_and_per_client() {
    let mut cfg = AccountsConfig::for_tests();
    cfg.login_per_account = Limit {
        max: 3,
        window: Duration::from_secs(900),
    };
    cfg.login_per_client = Limit {
        max: 6,
        window: Duration::from_secs(900),
    };
    let app = try_router(cfg).unwrap();
    let mut c = Client::new(&app);
    c.signup("a@example.org").await;
    for _ in 0..3 {
        let r = c
            .post(
                "/api/account/login",
                json!({ "email": "a@example.org", "password": "wrong password!" }),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // Even the right password is refused while the account is locked.
    let r = c
        .post(
            "/api/account/login",
            json!({ "email": "a@example.org", "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.headers.contains_key(header::RETRY_AFTER));

    // Per client: a different account from the same address runs into the client limit.
    let mut d = Client::new(&app);
    for _ in 0..2 {
        d.post(
            "/api/account/login",
            json!({ "email": "x@example.org", "password": "wrong password!" }),
        )
        .await;
    }
    let r = d
        .post(
            "/api/account/login",
            json!({ "email": "y@example.org", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    // Another address is unaffected.
    d.forwarded_for = "198.51.100.1".into();
    let r = d
        .post(
            "/api/account/login",
            json!({ "email": "y@example.org", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn saved_items_crud_and_lookup_by_atlas_id() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("maria@example.org").await;

    let r = c
        .post(
            "/api/account/saved",
            json!({
                "kind": "condition",
                "title": "Dravet syndrome",
                "refs": ["MONDO:0100135"],
                "payload": { "label": "Dravet syndrome" },
                "atlas_snapshot": "atlas.snapshot@sha256:abc"
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(r.body["refs"], json!(["MONDO:0100135"]));

    c.post(
        "/api/account/saved",
        json!({ "kind": "message_draft", "title": "To the Dravet Syndrome Foundation", "payload": { "body": "Hello" }, "refs": ["MONDO:0100135", "ORG:dsf"] }),
    )
    .await;

    let all = c.get("/api/account/saved").await;
    assert_eq!(all.body["items"].as_array().unwrap().len(), 2);
    let drafts = c.get("/api/account/saved?kind=message_draft").await;
    assert_eq!(drafts.body["items"].as_array().unwrap().len(), 1);
    let by_ref = c.get("/api/account/saved?ref=ORG:dsf").await;
    assert_eq!(by_ref.body["items"][0]["kind"], "message_draft");
    assert_eq!(
        c.get("/api/account/saved?kind=bogus").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let r = c
        .call(
            "PATCH",
            &format!("/api/account/saved/{id}"),
            Some(json!({ "note": "ask about the registry" })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["note"], "ask about the registry");
    assert_eq!(r.body["title"], "Dravet syndrome");
    let r = c
        .call(
            "PATCH",
            &format!("/api/account/saved/{id}"),
            Some(json!({ "note": null })),
        )
        .await;
    assert_eq!(r.body["note"], Value::Null);

    // Another user can't see or touch it.
    let mut eve = Client::new(&app);
    eve.signup("eve@example.org").await;
    assert_eq!(
        eve.get(&format!("/api/account/saved/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        eve.call("DELETE", &format!("/api/account/saved/{id}"), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(eve.get("/api/account/saved").await.body["items"], json!([]));

    assert_eq!(
        c.call("DELETE", &format!("/api/account/saved/{id}"), None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.get(&format!("/api/account/saved/{id}")).await.status,
        StatusCode::NOT_FOUND
    );

    // Validation: unknown kind, ids with spaces, oversized payload.
    let bad = [
        json!({ "kind": "nope", "title": "x" }),
        json!({ "kind": "node", "title": "x", "refs": ["HGNC 1"] }),
        json!({ "kind": "node", "title": "", "refs": [] }),
        json!({ "kind": "node", "title": "x", "payload": { "big": "x".repeat(70_000) } }),
    ];
    for b in bad {
        assert_eq!(
            c.post("/api/account/saved", b).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}

#[tokio::test]
async fn conversations_keep_questions_answers_and_citations() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("devon@example.org").await;
    let r = c
        .post(
            "/api/account/conversations",
            json!({
                "locale": "en",
                "messages": [
                    { "role": "user", "content": "Who else works on SCN1A epilepsy?" },
                    { "role": "assistant", "content": "Two groups ...",
                      "citations": [{ "id": "PMID:123", "label": "Smith 2020", "url": "https://pubmed.ncbi.nlm.nih.gov/123/" }],
                      "meta": { "provider": "kisski", "model": "gpt-oss-120b" } }
                ]
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["title"], "Who else works on SCN1A epilepsy?");
    let id = r.body["id"].as_str().unwrap().to_string();

    let r = c
        .post(
            &format!("/api/account/conversations/{id}/messages"),
            json!({ "messages": [{ "role": "user", "content": "And trials?" }] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["messages"][0]["seq"], 3);

    let full = c.get(&format!("/api/account/conversations/{id}")).await;
    let msgs = full.body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[1]["citations"][0]["id"], "PMID:123");
    assert_eq!(msgs[1]["meta"]["model"], "gpt-oss-120b");

    let list = c.get("/api/account/conversations").await;
    assert_eq!(list.body["conversations"][0]["message_count"], 3);

    let r = c
        .call(
            "PATCH",
            &format!("/api/account/conversations/{id}"),
            Some(json!({ "title": "SCN1A" })),
        )
        .await;
    assert_eq!(r.body["title"], "SCN1A");

    // Bad input: no title and no question; string citations.
    assert_eq!(
        c.post("/api/account/conversations", json!({})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = c
        .post(
            &format!("/api/account/conversations/{id}/messages"),
            json!({ "messages": [{ "role": "assistant", "content": "x", "citations": ["PMID:1"] }] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let mut eve = Client::new(&app);
    eve.signup("eve@example.org").await;
    assert_eq!(
        eve.get(&format!("/api/account/conversations/{id}")).await.status,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        c.call("DELETE", &format!("/api/account/conversations/{id}"), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.get("/api/account/conversations").await.body["conversations"],
        json!([])
    );
}

#[tokio::test]
async fn organisations_share_items_with_members_and_carry_a_plan_flag() {
    let app = app();
    let mut owner = Client::new(&app);
    owner.signup("lead@group.org").await;
    let r = owner
        .post("/api/account/orgs", json!({ "name": "Dravet Parents e.V." }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["plan"], "free");
    assert_eq!(r.body["role"], "owner");
    let org = r.body["id"].as_str().unwrap().to_string();

    let r = owner
        .post(
            "/api/account/saved",
            json!({ "kind": "connection_card", "title": "Study team", "org_id": org }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(
        owner.get(&format!("/api/account/saved?org={org}")).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // A non-member can neither share into nor list the organisation.
    let mut eve = Client::new(&app);
    eve.signup("eve@example.org").await;
    let r = eve
        .post(
            "/api/account/saved",
            json!({ "kind": "node", "title": "x", "org_id": org }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(
        eve.get(&format!("/api/account/saved?org={org}")).await.status,
        StatusCode::FORBIDDEN
    );

    let me = owner.get("/api/account/me").await;
    assert_eq!(me.body["organisations"][0]["name"], "Dravet Parents e.V.");
}

#[tokio::test]
async fn password_change_ends_other_sessions() {
    let app = app();
    let mut phone = Client::new(&app);
    phone.signup("maria@example.org").await;
    let mut stolen = Client::new(&app);
    stolen.cookie = phone.cookie.clone();
    let mut laptop = Client::new(&app);
    laptop
        .post(
            "/api/account/login",
            json!({ "email": "maria@example.org", "password": PASSWORD }),
        )
        .await;
    assert_eq!(laptop.get("/api/account/me").await.status, StatusCode::OK);

    let r = phone
        .post(
            "/api/account/password",
            json!({ "current_password": "not my password", "new_password": "another long passphrase" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = phone
        .post(
            "/api/account/password",
            json!({ "current_password": PASSWORD, "new_password": "another long passphrase" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["ended_other_sessions"], 1);
    assert_ne!(stolen.cookie, phone.cookie);
    assert_eq!(stolen.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(laptop.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(phone.get("/api/account/me").await.status, StatusCode::OK);

    let mut fresh = Client::new(&app);
    let r = fresh
        .post(
            "/api/account/login",
            json!({ "email": "maria@example.org", "password": "another long passphrase" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn export_contains_everything_but_secrets() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("maria@example.org").await;
    c.post(
        "/api/account/saved",
        json!({ "kind": "condition", "title": "Dravet syndrome", "refs": ["MONDO:0100135"] }),
    )
    .await;
    c.post(
        "/api/account/conversations",
        json!({ "messages": [{ "role": "user", "content": "What is Dravet?" }] }),
    )
    .await;
    c.post("/api/account/orgs", json!({ "name": "Family" })).await;
    let token = c
        .cookie
        .clone()
        .unwrap()
        .trim_start_matches("__Host-atlas_session=")
        .to_string();

    let r = c.get("/api/account/export").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    let b = &r.body;
    assert_eq!(b["format"], "rare-disease-atlas/account-export");
    assert_eq!(b["user"]["email"], "maria@example.org");
    assert_eq!(b["saved_items"][0]["refs"][0], "MONDO:0100135");
    assert_eq!(b["conversations"][0]["messages"][0]["content"], "What is Dravet?");
    assert_eq!(b["organisations"][0]["name"], "Family");
    assert_eq!(b["credentials"][0]["kind"], "password");
    assert_eq!(b["sessions"].as_array().unwrap().len(), 1);
    let text = b.to_string();
    assert!(!text.contains("$argon2"), "no password hash in the export");
    assert!(!text.contains(&token), "no session token in the export");
    assert!(!text.contains(PASSWORD));
}

#[tokio::test]
async fn deleting_the_account_removes_all_data_and_needs_the_password() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("maria@example.org").await;
    c.post(
        "/api/account/saved",
        json!({ "kind": "condition", "title": "Dravet", "refs": ["MONDO:0100135"] }),
    )
    .await;
    c.post("/api/account/conversations", json!({ "title": "Notes" })).await;
    c.post("/api/account/orgs", json!({ "name": "Solo org" })).await;
    let old_cookie = c.cookie.clone();

    let r = c.call("DELETE", "/api/account/me", Some(json!({}))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = c
        .call(
            "DELETE",
            "/api/account/me",
            Some(json!({ "password": "wrong password!" })),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = c
        .call("DELETE", "/api/account/me", Some(json!({ "password": PASSWORD })))
        .await;
    // D42: 200 with a plain summary of what was deleted
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["deleted"]["saved_items"], 1);
    assert_eq!(r.body["contributions"]["status"], "not_connected");
    assert!(r.headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));

    c.cookie = old_cookie;
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);
    let r = c
        .post(
            "/api/account/login",
            json!({ "email": "maria@example.org", "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // The address is free again.
    let mut again = Client::new(&app);
    let r = again.signup("maria@example.org").await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(again.get("/api/account/saved").await.body["items"], json!([]));
    assert_eq!(
        again.get("/api/account/conversations").await.body["conversations"],
        json!([])
    );
    assert_eq!(again.get("/api/account/me").await.body["organisations"], json!([]));
}

#[tokio::test]
async fn profile_updates_and_clears_fields() {
    let app = app();
    let mut c = Client::new(&app);
    c.signup("maria@example.org").await;
    let r = c
        .call(
            "PATCH",
            "/api/account/me",
            Some(json!({ "display_name": "  Maria   S. ", "locale": "zh-Hans" })),
        )
        .await;
    assert_eq!(r.body["user"]["display_name"], "Maria S.");
    assert_eq!(r.body["user"]["locale"], "zh-Hans");
    let r = c
        .call("PATCH", "/api/account/me", Some(json!({ "display_name": null })))
        .await;
    assert_eq!(r.body["user"]["display_name"], Value::Null);
    assert_eq!(r.body["user"]["locale"], "zh-Hans");
    let r = c
        .call("PATCH", "/api/account/me", Some(json!({ "locale": "en; DROP" })))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn non_json_bodies_get_json_errors() {
    let app = app();
    let req = Request::post("/api/account/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=a%40b.de&password=x"))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn data_survives_a_restart_with_a_file_database() {
    let dir = std::env::temp_dir().join(format!("atlas-accounts-test-{}", std::process::id()));
    let path = dir.join("app").join("accounts.sqlite");
    let cfg = || AccountsConfig::for_tests().with_database(Database::File(path.clone()));
    {
        let app = try_router(cfg()).unwrap();
        let mut c = Client::new(&app);
        c.signup("maria@example.org").await;
        c.post(
            "/api/account/saved",
            json!({ "kind": "condition", "title": "Dravet", "refs": ["MONDO:0100135"] }),
        )
        .await;
    }
    {
        let app = try_router(cfg()).unwrap();
        let mut c = Client::new(&app);
        c.post(
            "/api/account/login",
            json!({ "email": "maria@example.org", "password": PASSWORD }),
        )
        .await;
        assert_eq!(c.get("/api/account/saved").await.body["items"][0]["title"], "Dravet");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_unopenable_database_degrades_to_503_without_breaking_the_server() {
    // A directory can't be opened as a SQLite file.
    let dir = std::env::temp_dir();
    let app = router(AccountsConfig::for_tests().with_database(Database::File(dir)));
    let mut c = Client::new(&app);
    assert_eq!(c.get("/api/account/status").await.body["enabled"], false);
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn cookie_mutations_require_csrf_header_and_secure_cookie_name() {
    let app = app();
    let mut c = Client::new(&app);
    assert_eq!(c.signup("csrf@example.org").await.status, StatusCode::CREATED);
    let cookie = c.cookie.clone().unwrap();
    for method_path in [
        ("POST", "/api/account/logout"),
        ("POST", "/api/account/logout-all"),
        ("DELETE", "/api/account/me"),
    ] {
        let req = Request::builder()
            .method(method_path.0)
            .uri(method_path.1)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::OK);
    c.cookie = Some(cookie.replace("__Host-atlas_session", "atlas_session"));
    assert_eq!(c.get("/api/account/me").await.status, StatusCode::UNAUTHORIZED);
}
