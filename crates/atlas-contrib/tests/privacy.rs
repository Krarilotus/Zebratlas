//! D43 privacy requests over HTTP: no account needed, validation, honeypot, rate limits,
//! reviewer-only queue, approval writes suppression through the host (fail-closed without one).

use std::sync::{Arc, Mutex};

use atlas_contrib::ContribConfig;
use atlas_contrib::privacy::{NoActions, Privacy, PrivacyActions, PrivacyConfig, SuppressOrder, TraceParams, router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    ip: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header("x-forwarded-for", ip);
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

#[derive(Default)]
struct Recorder {
    orders: Mutex<Vec<(String, String, Option<String>)>>,
}

impl PrivacyActions for Recorder {
    fn trace(&self, p: &TraceParams) -> Value {
        json!({ "counts": { "nodes": usize::from(p.orcid.is_some()) } })
    }
    fn suppress(&self, o: &SuppressOrder<'_>) -> Result<String, String> {
        self.orders.lock().unwrap().push((
            o.reference.to_owned(),
            o.kind.as_str().to_owned(),
            o.trace.orcid.clone(),
        ));
        Ok("sup_test".into())
    }
    fn email_hash(&self, email: &str) -> String {
        NoActions.email_hash(email)
    }
}

fn app(actions: Arc<dyn PrivacyActions>) -> axum::Router {
    let p = Privacy::new(ContribConfig::for_tests(), PrivacyConfig::default())
        .unwrap()
        .with_actions(actions);
    router(p.into_state())
}

fn form(email: &str) -> Value {
    json!({ "email": email, "concerns": "https://orcid.org/0000-0000-0000-0000", "type": "remove" })
}

#[tokio::test]
async fn submit_without_account_then_reviewer_approves() {
    let rec = Arc::new(Recorder::default());
    let app = app(rec.clone());
    let (s, ack) = call(
        &app,
        "POST",
        "/api/privacy/requests",
        "198.51.100.1",
        None,
        Some(form("a@example.org")),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let reference = ack["reference"].as_str().unwrap().to_owned();
    assert!(reference.starts_with("pr_") && ack["respond_by"].is_string());
    assert_eq!(ack["message"]["key"], "privacy.request.received");
    assert_eq!(ack["message"]["params"]["reference"], reference);
    assert_eq!(ack["respond_by_message"]["params"]["date"], ack["respond_by"]);
    assert!(
        ack.get("email").is_none(),
        "the acknowledgement never echoes the address"
    );

    // Validation matches the form.
    for bad in [
        json!({ "email": "not-an-address", "concerns": "x", "type": "remove" }),
        json!({ "email": "a@example.org", "concerns": " ", "type": "remove" }),
        json!({ "email": "a@example.org", "concerns": "x", "type": "delete-everything" }),
        json!({ "email": "a@example.org", "concerns": "x", "type": "remove", "extra": 1 }),
        json!({ "email": "a@example.org", "concerns": "x".repeat(2001), "type": "remove" }),
    ] {
        let (s, _) = call(&app, "POST", "/api/privacy/requests", "198.51.100.2", None, Some(bad)).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // Queue and decisions are reviewer-only.
    let (s, _) = call(&app, "GET", "/api/privacy/review", "198.51.100.1", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(
        &app,
        "GET",
        "/api/privacy/review",
        "198.51.100.1",
        Some("wrong-token-wrong"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, q) = call(
        &app,
        "GET",
        "/api/privacy/review",
        "198.51.100.1",
        Some("test-token"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(q["items"].as_array().unwrap().len(), 1);
    let (_, d) = call(
        &app,
        "GET",
        &format!("/api/privacy/review/{reference}"),
        "198.51.100.1",
        Some("test-token"),
        None,
    )
    .await;
    assert_eq!(d["trace_params"]["orcid"], "0000-0000-0000-0000");
    assert_eq!(d["trace"]["counts"]["nodes"], 1);

    let (s, r) = call(
        &app,
        "POST",
        &format!("/api/privacy/review/{reference}"),
        "198.51.100.1",
        Some("test-token"),
        Some(json!({ "decision": "approve", "reason": "identity confirmed from the ORCID-linked address" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["request"]["state"], "approved");
    assert_eq!(r["request"]["suppression_entry"], "sup_test");
    let orders = rec.orders.lock().unwrap().clone();
    assert_eq!(
        orders,
        [(reference.clone(), "remove".into(), Some("0000-0000-0000-0000".into()))]
    );
    // Decided requests cannot be decided again.
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/privacy/review/{reference}"),
        "198.51.100.1",
        Some("test-token"),
        Some(json!({ "decision": "reject", "reason": "x" })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn approval_fails_closed_without_suppression_host() {
    let app = app(Arc::new(NoActions));
    let (_, ack) = call(
        &app,
        "POST",
        "/api/privacy/requests",
        "198.51.100.3",
        None,
        Some(form("b@example.org")),
    )
    .await;
    let reference = ack["reference"].as_str().unwrap();
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/privacy/review/{reference}"),
        "198.51.100.3",
        Some("test-token"),
        Some(json!({ "decision": "approve" })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, q) = call(
        &app,
        "GET",
        "/api/privacy/review",
        "198.51.100.3",
        Some("test-token"),
        None,
    )
    .await;
    assert_eq!(q["items"][0]["state"], "received", "still open");
}

#[tokio::test]
async fn honeypot_and_rate_limits() {
    let app = app(Arc::new(NoActions));
    let mut bot = form("bot@example.org");
    bot["website"] = json!("https://spam.example");
    let (s, _) = call(&app, "POST", "/api/privacy/requests", "198.51.100.4", None, Some(bot)).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let (_, q) = call(
        &app,
        "GET",
        "/api/privacy/review?state=all",
        "198.51.100.4",
        Some("test-token"),
        None,
    )
    .await;
    assert!(
        q["items"].as_array().unwrap().is_empty(),
        "honeypot hits are not stored"
    );
    // Per address: 3 per day.
    let mut codes = vec![];
    for i in 0..4 {
        let (s, body) = call(
            &app,
            "POST",
            "/api/privacy/requests",
            &format!("198.51.100.{}", 10 + i),
            None,
            Some(form("same@example.org")),
        )
        .await;
        if i == 3 {
            assert_eq!(body["message"]["key"], "privacy.request.rate_limited");
            assert!(body["message"]["params"]["seconds"].is_number());
        }
        codes.push(s);
    }
    assert_eq!(codes[3], StatusCode::TOO_MANY_REQUESTS);
    // Per client: 5 per hour.
    let mut last = StatusCode::OK;
    for i in 0..6 {
        let (s, _) = call(
            &app,
            "POST",
            "/api/privacy/requests",
            "198.51.100.99",
            None,
            Some(form(&format!("u{i}@example.org"))),
        )
        .await;
        last = s;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}
