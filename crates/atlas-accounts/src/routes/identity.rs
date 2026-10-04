//! Sign-up, sign-in, sign-out, profile, password, organisations, export and deletion.

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AppState, Body, ClientKey, Session, clear_cookie, json_response, presented_token, session_cookie, with_cookie,
};
use crate::auth::{Secret, check_new_password, new_session_token, token_hash};
use crate::error::{AccountsError, Result};
use crate::model::{User, clean_display_name, clean_email, clean_locale, clean_org_name, double_option};
use crate::util::{now, rfc3339};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignupBody {
    email: String,
    password: Secret,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginBody {
    email: String,
    password: Secret,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileBody {
    #[serde(default, deserialize_with = "double_option")]
    display_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    locale: Option<Option<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordBody {
    current_password: Secret,
    new_password: Secret,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteBody {
    #[serde(default)]
    password: Option<Secret>,
    /// For accounts without a password (future passkey / magic-link sign-in): the literal `"delete"`.
    #[serde(default)]
    confirm: Option<String>,
    /// D42: keep the name on accepted contributions (an unticked option; default false).
    #[serde(default)]
    keep_credit: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OrgBody {
    name: String,
}

/// `{user, plan, organisations}`: what the UI needs to render the signed-in state.
fn account_view(state: &AppState, user: &User) -> Result<Value> {
    Ok(json!({
        "user": user,
        "plan": state.store.effective_plan(&user.id)?,
        "organisations": state.store.memberships(&user.id)?,
        "email_verification": { "state": state.store.email_status(&user.id)? },
    }))
}

/// Creates a session and returns the response with its cookie. Drops a session the request already
/// carried, so a fixed token can't survive sign-in.
pub(super) fn start_session(
    state: &AppState,
    headers: &HeaderMap,
    user: &User,
    status: StatusCode,
    expected_phc: Option<&str>,
) -> Result<Response> {
    let (token, hash) = new_session_token();
    let t = now();
    let ttl = state.config.session_ttl.as_secs();
    if let Some(phc) = expected_phc {
        if !state
            .store
            .create_password_session(&user.id, phc, &hash, t, t + ttl as i64)?
        {
            return Err(AccountsError::BadCredentials);
        }
    } else {
        state
            .store
            .create_session(&user.id, &hash, "password", t, t + ttl as i64)?;
    }
    if let Some(old) = presented_token(headers, state.config.secure_cookies) {
        state.store.delete_session(&token_hash(&old))?;
    }
    let mut body = account_view(state, user)?;
    body["session"] = json!({ "expires_at": rfc3339(t + ttl as i64) });
    let res = json_response(status, &body);
    Ok(with_cookie(
        res,
        session_cookie(token.expose(), ttl, state.config.secure_cookies),
    ))
}

pub(crate) async fn signup(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(b): Body<SignupBody>,
) -> Result<Response> {
    state.signup_per_client.hit(&client)?;
    let email = clean_email(&b.email)?;
    check_new_password(&b.password)?;
    let display_name = clean_display_name(b.display_name)?;
    let locale = clean_locale(b.locale)?;
    let email_locale = crate::email::EmailLocale::from_tag(locale.as_deref());
    if state.config.require_email_verification {
        super::email::require_delivery(&state, email_locale)?;
        state.store.reserve_email_action(&email, "verify_email", now())?;
    }

    let hasher = state.hasher.clone();
    let password = b.password;
    let phc = state.config.password_work.run(move || hasher.hash(&password)).await??;
    if state.config.require_email_verification {
        super::email::signup_proof(
            &state,
            &email,
            display_name.as_deref(),
            locale.as_deref(),
            &phc,
            email_locale,
        )?;
        return Ok(super::email::reply(
            StatusCode::ACCEPTED,
            "verification_required",
            "verification_sent",
            email_locale,
        ));
    }
    let user = state
        .store
        .create_user(&email, display_name.as_deref(), locale.as_deref(), &phc, now())?;
    start_session(&state, &headers, &user, StatusCode::CREATED, None)
}

pub(crate) async fn login(
    State(state): State<AppState>,
    ClientKey(client): ClientKey,
    headers: HeaderMap,
    Body(b): Body<LoginBody>,
) -> Result<Response> {
    state.login_per_client.hit(&client)?;
    let email = clean_email(&b.email).map_err(|_| AccountsError::BadCredentials)?;
    state.login_per_account.hit(&email)?;

    let found = state.store.password_login(&email)?;
    let hasher = state.hasher.clone();
    let password = b.password;
    let user = state
        .config
        .password_work
        .run(move || match found {
            Some((user, phc)) => hasher.verify(&password, &phc).then_some((user, phc)),
            None => {
                hasher.verify_dummy(&password);
                None
            }
        })
        .await?;
    let Some((user, phc)) = user else {
        return Err(AccountsError::BadCredentials);
    };
    if state.store.email_pending(&user.id)? {
        return Err(AccountsError::EmailFlow {
            code: crate::email::EmailErrorCode::Unverified,
            locale: crate::email::EmailLocale::from_tag(user.locale.as_deref()),
        });
    }
    state.login_per_account.reset(&email);
    start_session(&state, &headers, &user, StatusCode::OK, Some(&phc))
}

/// Ends the presented session (if any). Always succeeds, so it's safe to call twice.
pub(crate) async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let Some(token) = presented_token(&headers, state.config.secure_cookies) {
        state.store.delete_session(&token_hash(&token))?;
    }
    Ok(with_cookie(
        StatusCode::NO_CONTENT.into_response(),
        clear_cookie(state.config.secure_cookies),
    ))
}

/// Ends every session of the user, this one included.
pub(crate) async fn logout_all(State(state): State<AppState>, s: Session) -> Result<Response> {
    let ended = state.store.delete_sessions_of(&s.info.user.id, None)?;
    let res = json_response(StatusCode::OK, &json!({ "ended_sessions": ended }));
    Ok(with_cookie(res, clear_cookie(state.config.secure_cookies)))
}

pub(crate) async fn me(State(state): State<AppState>, s: Session) -> Result<Response> {
    let mut body = account_view(&state, &s.info.user)?;
    body["session"] = json!({ "expires_at": rfc3339(s.info.expires_at) });
    Ok(json_response(StatusCode::OK, &body))
}

pub(crate) async fn update_me(
    State(state): State<AppState>,
    s: Session,
    Body(b): Body<ProfileBody>,
) -> Result<Response> {
    let name = match b.display_name {
        Some(n) => Some(clean_display_name(n)?),
        None => None,
    };
    let locale = match b.locale {
        Some(l) => Some(clean_locale(l)?),
        None => None,
    };
    let user = state.store.update_profile(
        &s.info.user.id,
        name.as_ref().map(|n| n.as_deref()),
        locale.as_ref().map(|l| l.as_deref()),
        now(),
    )?;
    Ok(json_response(StatusCode::OK, &account_view(&state, &user)?))
}

/// Verifies the user's current password, rate-limited like sign-in.
async fn verify_password(state: &AppState, user: &User, password: Secret) -> Result<String> {
    state.login_per_account.hit(&user.email)?;
    let phc = state
        .store
        .password_hash(&user.id)?
        .ok_or(AccountsError::BadCredentials)?;
    let hasher = state.hasher.clone();
    let checked_phc = phc.clone();
    if state
        .config
        .password_work
        .run(move || hasher.verify(&password, &phc))
        .await?
    {
        Ok(checked_phc)
    } else {
        Err(AccountsError::BadCredentials)
    }
}

/// Changes the password, invalidates every old token and issues a fresh current session.
pub(crate) async fn change_password(
    State(state): State<AppState>,
    s: Session,
    Body(b): Body<PasswordBody>,
) -> Result<Response> {
    check_new_password(&b.new_password)?;
    let expected_phc = verify_password(&state, &s.info.user, b.current_password).await?;
    let hasher = state.hasher.clone();
    let new_password = b.new_password;
    let phc = state
        .config
        .password_work
        .run(move || hasher.hash(&new_password))
        .await??;
    let (token, hash) = new_session_token();
    let t = now();
    let ttl = state.config.session_ttl.as_secs();
    let ended = state
        .store
        .rotate_password_session(
            &s.info.user.id,
            &s.info.id,
            &expected_phc,
            &phc,
            &hash,
            t,
            t + ttl as i64,
        )?
        .ok_or(AccountsError::BadCredentials)?;
    Ok(with_cookie(
        json_response(
            StatusCode::OK,
            &json!({ "ended_other_sessions": ended.saturating_sub(1) }),
        ),
        session_cookie(token.expose(), ttl, state.config.secure_cookies),
    ))
}

/// Deletes the account and everything in it (GDPR Art. 17). Needs the password again.
pub(crate) async fn delete_me(
    State(state): State<AppState>,
    s: Session,
    Body(b): Body<DeleteBody>,
) -> Result<Response> {
    let user = &s.info.user;
    let has_password = state.store.password_hash(&user.id)?.is_some();
    match (has_password, b.password) {
        (true, Some(pw)) => {
            verify_password(&state, user, pw).await?;
        }
        (true, None) => return Err(AccountsError::invalid("enter your password to delete your account")),
        (false, _) if b.confirm.as_deref() == Some("delete") => {}
        (false, _) => {
            return Err(AccountsError::invalid(
                "send confirm: \"delete\" to delete your account",
            ));
        }
    }
    let footprint = state.store.footprint(&user.id)?;
    // contributions first: if they cannot be anonymised, nothing is deleted and the user can retry
    let contrib = match &state.hooks {
        Some(h) => Some((
            h.anonymise_for_user(&user.id, b.keep_credit)
                .map_err(|_| AccountsError::Unavailable)?,
            b.keep_credit,
        )),
        None => None,
    };
    state.store.delete_user(&user.id)?;
    let body = super::data::deletion_summary(&footprint, contrib);
    Ok(with_cookie(
        json_response(StatusCode::OK, &body),
        clear_cookie(state.config.secure_cookies),
    ))
}

/// Everything stored about the user as one JSON download (GDPR Art. 15 and 20).
pub(crate) async fn export(State(state): State<AppState>, s: Session) -> Result<Response> {
    let t = now();
    let mut body = state.store.export(&s.info.user.id, t)?;
    body["documents"] = serde_json::json!(crate::documents::export(
        &state.store,
        state.kek.as_ref(),
        &s.info.user.id
    )?);
    body["contributions"] = super::data::contributions(&state, &s.info.user.id);
    let date = rfc3339(t).get(..10).unwrap_or("export").to_string();
    let mut res = json_response(StatusCode::OK, &body);
    if let Ok(v) = HeaderValue::from_str(&format!(
        "attachment; filename=\"rare-disease-atlas-account-{date}.json\""
    )) {
        res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

pub(crate) async fn list_orgs(State(state): State<AppState>, s: Session) -> Result<Response> {
    let orgs = state.store.memberships(&s.info.user.id)?;
    Ok(json_response(StatusCode::OK, &json!({ "organisations": orgs })))
}

/// Creates an organisation on the free plan, owned by the caller. Plans change only by an operator.
pub(crate) async fn create_org(State(state): State<AppState>, s: Session, Body(b): Body<OrgBody>) -> Result<Response> {
    let name = clean_org_name(&b.name)?;
    if state.store.memberships(&s.info.user.id)?.len() >= 20 {
        return Err(AccountsError::invalid("too many organisations"));
    }
    let m = state.store.create_org(&s.info.user.id, &name, now())?;
    Ok(json_response(StatusCode::CREATED, &m))
}
