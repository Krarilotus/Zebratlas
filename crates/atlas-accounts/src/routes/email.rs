//! Explicit token consumption; GET email links never mutate account state.
use super::{AppState, Body, ClientKey, clear_cookie, json_response, with_cookie};
use crate::{
    auth::{Secret, check_new_password, new_session_token, token_hash},
    email::{EmailConfig, EmailErrorCode, EmailLocale},
    error::{AccountsError, Result},
    model::{User, clean_email, clean_locale},
    util::now,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use serde_json::json;

pub(super) fn require_delivery(state: &AppState, locale: EmailLocale) -> Result<&EmailConfig> {
    state.config.email.as_ref().ok_or(AccountsError::EmailFlow {
        code: EmailErrorCode::Unavailable,
        locale,
    })
}
pub(super) fn reply(status: StatusCode, state: &str, code: &str, locale: EmailLocale) -> Response {
    let message = crate::email::message(code, locale);
    json_response(
        status,
        &json!({"state":state,"code":code,"detail":message["fallback"],"detail_msg":message}),
    )
}
pub(super) fn queue(state: &AppState, user: User, purpose: &'static str, locale: EmailLocale) -> Result<()> {
    let config = require_delivery(state, locale)?.clone();
    let (token, hash) = new_session_token();
    let ttl = if purpose == "verify_email" {
        state.config.verification_ttl
    } else {
        state.config.password_reset_ttl
    };
    let t = now();
    state
        .store
        .issue_email_token(&user.id, purpose, &hash, t, t + ttl.as_secs() as i64)?;
    let email = config.prepare(user.email, purpose, &token, &hash, locale, (ttl.as_secs() / 60).max(1));
    deliver(state, config, email, hash);
    Ok(())
}

pub(super) fn signup_proof(
    state: &AppState,
    email: &str,
    name: Option<&str>,
    locale_tag: Option<&str>,
    phc: &str,
    locale: EmailLocale,
) -> Result<()> {
    let config = require_delivery(state, locale)?.clone();
    let (token, hash) = new_session_token();
    let t = now();
    let ttl = state.config.verification_ttl;
    if let Some(user) =
        state
            .store
            .signup_pending_with_token(email, name, locale_tag, phc, &hash, t, t + ttl.as_secs() as i64)?
    {
        let email = config.prepare(
            user.email,
            "verify_email",
            &token,
            &hash,
            locale,
            (ttl.as_secs() / 60).max(1),
        );
        deliver(state, config, email, hash);
    }
    Ok(())
}

fn deliver(state: &AppState, config: EmailConfig, email: crate::email::OutboundEmail, hash: String) {
    let Ok(permit) = config.work.clone().try_acquire_owned() else {
        let _ = state.store.discard_email_token(&hash);
        eprintln!("atlas-accounts: transactional email capacity unavailable; proof invalidated");
        return;
    };
    let state = state.clone();
    // Avoid recipient-dependent HTTP timing. The permit bounds all active deliveries;
    // no automatic retry, raw-token logs or delivery bodies reach the response.
    tokio::spawn(async move {
        let _permit = permit;
        if !config.delivery.send(email).await {
            let _ = state.store.discard_email_token(&hash);
            eprintln!("atlas-accounts: transactional email delivery failed; proof invalidated");
        }
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddressBody {
    email: String,
    #[serde(default)]
    locale: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TokenBody {
    token: Secret,
    #[serde(default)]
    locale: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResetBody {
    token: Secret,
    new_password: Secret,
    #[serde(default)]
    locale: Option<String>,
}
fn language(headers: &HeaderMap, locale: Option<String>) -> Result<EmailLocale> {
    let locale = clean_locale(locale)?;
    Ok(EmailLocale::from_tag(
        locale
            .as_deref()
            .or_else(|| headers.get("x-atlas-lang").and_then(|v| v.to_str().ok())),
    ))
}
fn proof_hash(token: &Secret, locale: EmailLocale) -> Result<String> {
    let raw = token.expose();
    if raw.len() != 43 || !raw.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(AccountsError::EmailFlow {
            code: EmailErrorCode::InvalidToken,
            locale,
        });
    }
    Ok(token_hash(raw))
}

pub(crate) async fn resend(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(body): Body<AddressBody>,
) -> Result<Response> {
    request_link(&state, &client, &headers, body, "verify_email").await
}
pub(crate) async fn forgot(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(body): Body<AddressBody>,
) -> Result<Response> {
    request_link(&state, &client, &headers, body, "reset_password").await
}
async fn request_link(
    state: &AppState,
    client: &str,
    headers: &HeaderMap,
    body: AddressBody,
    purpose: &'static str,
) -> Result<Response> {
    let locale = language(headers, body.locale)?;
    require_delivery(state, locale)?;
    state.email_per_client.hit(client)?;
    let address = clean_email(&body.email)?;
    state.store.reserve_email_action(&address, purpose, now())?;
    if let Some(user) = state.store.user_by_email(&address)? {
        if purpose == "reset_password" || state.store.email_pending(&user.id)? {
            queue(state, user, purpose, locale)?;
        }
    }
    Ok(reply(
        StatusCode::ACCEPTED,
        "accepted",
        if purpose == "verify_email" {
            "verification_sent"
        } else {
            "password_reset_sent"
        },
        locale,
    ))
}
pub(crate) async fn verify(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(body): Body<TokenBody>,
) -> Result<Response> {
    let locale = language(&headers, body.locale)?;
    state.email_per_client.hit(&client)?;
    let hash = proof_hash(&body.token, locale)?;
    if !state.store.verify_email_token(&hash, now())? {
        return Err(AccountsError::EmailFlow {
            code: EmailErrorCode::InvalidToken,
            locale,
        });
    }
    Ok(reply(StatusCode::OK, "verified", "email_verified", locale))
}
pub(crate) async fn reset(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(body): Body<ResetBody>,
) -> Result<Response> {
    let locale = language(&headers, body.locale)?;
    state.email_per_client.hit(&client)?;
    let hash = proof_hash(&body.token, locale)?;
    check_new_password(&body.new_password)?;
    let hasher = state.hasher.clone();
    let phc = state
        .config
        .password_work
        .run(move || hasher.hash(&body.new_password))
        .await??;
    if !state.store.reset_password_token(&hash, &phc, now())? {
        return Err(AccountsError::EmailFlow {
            code: EmailErrorCode::InvalidToken,
            locale,
        });
    }
    Ok(with_cookie(
        reply(StatusCode::OK, "password_reset", "password_reset_complete", locale),
        clear_cookie(state.config.secure_cookies),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::{Delivery, OutboundEmail};
    use axum::{
        Router,
        body::{Body as HttpBody, to_bytes},
        http::{Request, header},
    };
    use serde_json::Value;
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tower::ServiceExt;

    #[derive(Default)]
    struct MockDelivery {
        outbox: Mutex<Vec<OutboundEmail>>,
        fail: bool,
    }
    impl Delivery for MockDelivery {
        fn send(&self, email: OutboundEmail) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
            Box::pin(async move {
                self.outbox.lock().unwrap().push(email);
                !self.fail
            })
        }
    }
    fn fixture(fail: bool) -> (Router, AppState, Arc<MockDelivery>) {
        let mock = Arc::new(MockDelivery {
            fail,
            ..Default::default()
        });
        let mut config = crate::AccountsConfig::for_tests();
        config.require_email_verification = true;
        config.email = Some(EmailConfig {
            origin: "https://atlas.example.invalid".into(),
            delivery: mock.clone(),
            work: Arc::new(tokio::sync::Semaphore::new(4)),
        });
        let state = super::super::build_state(config, crate::AccountsExtras::default()).unwrap();
        (super::super::router_from_state(state.clone()), state, mock)
    }
    async fn request(app: &Router, path: &str, body: Value, cookie: Option<&str>) -> (StatusCode, HeaderMap, Value) {
        let mut req = Request::post(format!("/api/account/{path}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-atlas-csrf", "1");
        if let Some(cookie) = cookie {
            req = req.header(header::COOKIE, cookie);
        }
        let response = app
            .clone()
            .oneshot(req.body(HttpBody::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        (status, headers, serde_json::from_slice(&bytes).unwrap())
    }
    async fn token(mock: &MockDelivery, index: usize) -> String {
        for _ in 0..100 {
            if let Some(mail) = mock.outbox.lock().unwrap().get(index) {
                return mail
                    .text
                    .split("#token=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_owned();
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("fixture delivery did not run")
    }
    #[tokio::test]
    async fn signup_requires_proof_then_explicit_login_and_reset_revokes_all_sessions() {
        let (app, state, mock) = fixture(false);
        let (status, headers, pending) = request(
            &app,
            "signup",
            json!({"email":"proof@example.org","password":"old safe password","locale":"de"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(!headers.contains_key(header::SET_COOKIE));
        assert_eq!(pending["state"], "verification_required");
        assert!(pending.get("account").is_none());
        let proof = token(&mock, 0).await;
        assert!(!pending.to_string().contains(&proof));
        let mail = mock.outbox.lock().unwrap();
        assert!(mail[0].text.contains("&lang=de#token="));
        assert!(mail[0].subject.contains("Bestätige"));
        assert_eq!(format!("{:?}", mail[0]), "OutboundEmail(***)");
        drop(mail);
        let (status, _, body) = request(
            &app,
            "login",
            json!({"email":"proof@example.org","password":"old safe password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "email_unverified");
        let (status, headers, body) = request(&app, "verify-email", json!({"token":proof,"locale":"de"}), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!headers.contains_key(header::SET_COOKIE));
        assert_eq!(body["detail"], "E-Mail bestätigt. Du kannst dich jetzt anmelden.");
        assert_eq!(
            request(&app, "verify-email", json!({"token":proof}), None).await.2["code"],
            "invalid_token"
        );
        let (status, headers, body) = request(
            &app,
            "login",
            json!({"email":"proof@example.org","password":"old safe password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email_verification"]["state"], "verified");
        let cookie = headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let (status, _, known) = request(
            &app,
            "forgot-password",
            json!({"email":"proof@example.org","locale":"de"}),
            None,
        )
        .await;
        let (_, _, unknown) = request(
            &app,
            "forgot-password",
            json!({"email":"unknown@example.org","locale":"de"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(known, unknown);
        let reset = token(&mock, 1).await;
        assert_eq!(
            request(&app, "verify-email", json!({"token":reset}), None).await.2["code"],
            "invalid_token"
        );
        let (status, headers, body) = request(
            &app,
            "reset-password",
            json!({"token":reset,"new_password":"new safe password","locale":"de"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"], "password_reset_complete");
        assert!(headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
        let old_cookie_token = cookie.split_once('=').unwrap().1;
        assert!(
            state
                .store
                .session(&token_hash(old_cookie_token), now())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            request(
                &app,
                "reset-password",
                json!({"token":reset,"new_password":"another safe password"}),
                None
            )
            .await
            .2["code"],
            "invalid_token"
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":"proof@example.org","password":"old safe password"}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":"proof@example.org","password":"new safe password"}),
                None
            )
            .await
            .0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn failed_delivery_never_activates_account_or_leaves_a_usable_proof() {
        let (app, state, mock) = fixture(true);
        let (status, headers, _) = request(
            &app,
            "signup",
            json!({"email":"failed@example.org","password":"safe enough password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(!headers.contains_key(header::SET_COOKIE));
        let proof = token(&mock, 0).await;
        assert!(!state.store.verify_email_token(&token_hash(&proof), now()).unwrap());
        assert!(
            state
                .store
                .email_pending(&state.store.user_by_email("failed@example.org").unwrap().unwrap().id)
                .unwrap()
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":"failed@example.org","password":"safe enough password"}),
                None
            )
            .await
            .2["code"],
            "email_unverified"
        );
        assert_eq!(
            request(&app, "resend-verification", json!({"email":"failed@example.org"}), None)
                .await
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn missing_configuration_fails_closed_but_legacy_users_can_sign_in() {
        let mut config = crate::AccountsConfig::for_tests();
        config.require_email_verification = true;
        let state = super::super::build_state(config, crate::AccountsExtras::default()).unwrap();
        let phc = state.hasher.hash(&Secret::new("safe legacy password")).unwrap();
        let user = state
            .store
            .create_user("legacy@example.org", None, None, &phc, now())
            .unwrap();
        let app = super::super::router_from_state(state.clone());
        let (status, headers, body) = request(
            &app,
            "signup",
            json!({"email":"new@example.org","password":"safe enough password","locale":"de"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "mail_unavailable");
        assert!(!headers.contains_key(header::SET_COOKIE));
        assert!(state.store.user_by_email("new@example.org").unwrap().is_none());
        let (status, _, body) = request(
            &app,
            "login",
            json!({"email":"legacy@example.org","password":"safe legacy password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email_verification"]["state"], "legacy");
        assert_eq!(body["user"]["id"], user.id);
    }

    #[tokio::test]
    async fn legitimate_pending_resignup_replaces_attacker_credential_and_old_proof() {
        let (app, state, mock) = fixture(false);
        let address = "owner@example.org";
        assert_eq!(
            request(
                &app,
                "signup",
                json!({"email":address,"password":"attacker password"}),
                None
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        let old = token(&mock, 0).await;
        // Advance only the fixture's persistent minute cooldown; no production clock or database.
        state.store.test_expire_email_cooldown(address).unwrap();
        assert_eq!(
            request(
                &app,
                "signup",
                json!({"email":address,"password":"owner safe password","display_name":"Owner","locale":"de"}),
                None
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        let new = token(&mock, 1).await;
        assert_eq!(
            request(&app, "verify-email", json!({"token":old}), None).await.2["code"],
            "invalid_token"
        );
        assert_eq!(
            request(&app, "verify-email", json!({"token":new}), None).await.0,
            StatusCode::OK
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":address,"password":"attacker password"}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":address,"password":"owner safe password"}),
                None
            )
            .await
            .0,
            StatusCode::OK
        );
        state.store.test_expire_email_cooldown(address).unwrap();
        let (_, headers, body) = request(
            &app,
            "signup",
            json!({"email":address,"password":"other attacker password"}),
            None,
        )
        .await;
        assert_eq!(body["state"], "verification_required");
        assert!(!headers.contains_key(header::SET_COOKIE));
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":address,"password":"other attacker password"}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "login",
                json!({"email":address,"password":"owner safe password"}),
                None
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(mock.outbox.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn saturated_delivery_does_not_reveal_whether_an_address_has_an_account() {
        let (app, state, mock) = fixture(false);
        let phc = state.hasher.hash(&Secret::new("safe legacy password")).unwrap();
        state
            .store
            .create_user("known@example.org", None, None, &phc, now())
            .unwrap();
        let capacity = state
            .config
            .email
            .as_ref()
            .unwrap()
            .work
            .clone()
            .try_acquire_many_owned(4)
            .unwrap();
        let known = request(&app, "forgot-password", json!({"email":"known@example.org"}), None).await;
        let unknown = request(&app, "forgot-password", json!({"email":"unknown@example.org"}), None).await;
        assert_eq!(known.0, StatusCode::ACCEPTED);
        assert_eq!(unknown.0, known.0);
        assert_eq!(unknown.2, known.2);
        assert!(mock.outbox.lock().unwrap().is_empty());
        drop(capacity);
    }

    #[tokio::test]
    async fn reset_between_password_verification_and_session_issuance_rejects_stale_login() {
        let (_, state, _) = fixture(false);
        let old = state.hasher.hash(&Secret::new("old safe password")).unwrap();
        let user = state
            .store
            .create_user("race@example.org", None, None, &old, now())
            .unwrap();
        state
            .store
            .create_session(&user.id, "old-session-hash", "password", now(), now() + 300)
            .unwrap();
        let session = state.store.session("old-session-hash", now()).unwrap().unwrap();
        let (_, reset_hash) = new_session_token();
        state
            .store
            .issue_email_token(&user.id, "reset_password", &reset_hash, now(), now() + 300)
            .unwrap();
        let login_state = state.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let login = tokio::spawn(async move {
            let (user, phc) = login_state.store.password_login("race@example.org").unwrap().unwrap();
            assert!(login_state.hasher.verify(&Secret::new("old safe password"), &phc));
            ready_tx.send(()).unwrap();
            resume_rx.await.unwrap();
            matches!(
                super::super::identity::start_session(
                    &login_state,
                    &HeaderMap::new(),
                    &user,
                    StatusCode::OK,
                    Some(&phc)
                ),
                Err(AccountsError::BadCredentials)
            )
        });
        ready_rx.await.unwrap();
        let new = state.hasher.hash(&Secret::new("new safe password")).unwrap();
        assert!(state.store.reset_password_token(&reset_hash, &new, now()).unwrap());
        resume_tx.send(()).unwrap();
        assert!(login.await.unwrap());
        assert!(state.store.session("old-session-hash", now()).unwrap().is_none());
        assert!(
            state
                .store
                .rotate_password_session(
                    &user.id,
                    &session.id,
                    &old,
                    "stale-changing-phc",
                    "stale-new-session",
                    now(),
                    now() + 300
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state.store.password_hash(&user.id).unwrap().as_deref(),
            Some(new.as_str())
        );
    }
}
