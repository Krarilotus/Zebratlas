//! "Your data" (D41, D42): saved documents, a view of everything stored, and the deletion summary.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{AppState, Session, json_response};
use crate::documents::{self, AnonymiseReport};
use crate::error::Result;
use crate::store::{Footprint, SavedFilter};

pub(crate) async fn list_documents(State(state): State<AppState>, s: Session) -> Result<Response> {
    let docs = documents::list(&state.store, state.kek.as_ref(), &s.info.user.id)?;
    Ok(json_response(
        StatusCode::OK,
        &json!({ "documents": docs, "can_save": state.kek.is_some() }),
    ))
}

pub(crate) async fn get_document(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
) -> Result<Response> {
    let doc = documents::get(&state.store, state.kek.as_ref(), &s.info.user.id, &id)?;
    Ok(json_response(StatusCode::OK, &doc))
}

pub(crate) async fn delete_document(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
) -> Result<Response> {
    if state.store.delete_document(&s.info.user.id, &id)? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(crate::error::AccountsError::NotFound)
    }
}

/// Contributions of the user via the host's hooks: `{ available, items }`.
pub(crate) fn contributions(state: &AppState, user_id: &str) -> Value {
    match &state.hooks {
        Some(h) => match h.export_for_user(user_id) {
            Ok(v) => json!({ "available": true, "items": v }),
            Err(_) => {
                json!({ "available": false, "items": [], "note": msg("account.data.contributions_unavailable", json!({}), "Your contributions could not be loaded right now.") })
            }
        },
        None => json!({ "available": false, "items": [] }),
    }
}

/// `GET /api/account/data`: what we store about you, at a glance.
pub(crate) async fn view(State(state): State<AppState>, s: Session) -> Result<Response> {
    let uid = &s.info.user.id;
    let f = state.store.footprint(uid)?;
    let docs = documents::list(&state.store, state.kek.as_ref(), uid)?;
    let saved = state.store.list_saved(uid, &SavedFilter::default())?.len();
    Ok(json_response(
        StatusCode::OK,
        &json!({
            "account": s.info.user,
            "sessions": f.sessions,
            "saved_items": saved,
            "conversations": f.conversations,
            "documents": docs,
            "contributions": contributions(&state, uid),
            "export_url": "/api/account/export",
        }),
    ))
}

pub(crate) fn msg(key: &str, params: Value, fallback: &str) -> Value {
    json!({ "key": key, "params": params, "fallback": fallback })
}

/// The plain summary of an account deletion (D42 §4): what was deleted, what was kept anonymised.
pub(crate) fn deletion_summary(f: &Footprint, contrib: Option<(AnonymiseReport, bool)>) -> Value {
    let mut summary = vec![
        msg("account.deleted.account", json!({}), "Your account is deleted."),
        msg(
            "account.deleted.sessions",
            json!({ "n": f.sessions }),
            "You are signed out everywhere.",
        ),
        msg(
            "account.deleted.documents",
            json!({ "n": f.documents }),
            &format!(
                "{} saved documents deleted, with the key that could read them.",
                f.documents
            ),
        ),
        msg(
            "account.deleted.saved",
            json!({ "n": f.saved_items, "conversations": f.conversations }),
            &format!(
                "{} saved items and {} conversations deleted.",
                f.saved_items, f.conversations
            ),
        ),
    ];
    let contributions = match contrib {
        None => {
            summary.push(msg(
                "account.deleted.contributions_not_connected",
                json!({}),
                "Contributions are not stored on this server.",
            ));
            json!({ "status": "not_connected", "accepted_kept": 0, "personal_fields_removed": 0 })
        }
        Some((r, keep)) => {
            let status = match (r.accepted_kept + r.personal_fields_removed, keep) {
                (0, _) => "none",
                (_, true) => "kept_with_name",
                (_, false) => "anonymised",
            };
            summary.push(match status {
                "none" => msg("account.deleted.contributions_none", json!({}), "You had no contributions."),
                "kept_with_name" => msg(
                    "account.deleted.contributions_credited",
                    json!({ "n": r.accepted_kept }),
                    &format!(
                        "{} accepted contributions stay in the atlas with your name, as you chose; your e-mail and account link are removed.",
                        r.accepted_kept
                    ),
                ),
                _ => msg(
                    "account.deleted.contributions_anonymised",
                    json!({ "n": r.accepted_kept }),
                    &format!(
                        "{} accepted contributions stay in the atlas without your name, e-mail or account link.",
                        r.accepted_kept
                    ),
                ),
            });
            json!({ "status": status, "accepted_kept": r.accepted_kept, "personal_fields_removed": r.personal_fields_removed })
        }
    };
    json!({
        "deleted": { "account": true, "sessions": f.sessions, "saved_items": f.saved_items,
                     "conversations": f.conversations, "documents": f.documents, "data_key": f.data_key },
        "contributions": contributions,
        "summary": summary,
    })
}
