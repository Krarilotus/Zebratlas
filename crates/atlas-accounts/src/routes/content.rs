//! Saved items and conversations.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::{AppState, Body, Session, json_response};
use crate::error::{AccountsError, Result};
use crate::model::{
    MAX_MESSAGES_PER_REQUEST, NewConversation, NewMessage, NewSaved, SavedKind, SavedPatch, clean_locale, clean_note,
    clean_payload, clean_refs, clean_snapshot, clean_title,
};
use crate::store::{SavedChanges, SavedFields, SavedFilter};
use crate::util::now;

const DEFAULT_LIMIT: u32 = 200;
const MAX_LIMIT: u32 = 1000;

#[derive(Deserialize)]
pub(crate) struct SavedQuery {
    kind: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    org: Option<String>,
    limit: Option<u32>,
}

pub(crate) async fn list_saved(
    State(state): State<AppState>,
    s: Session,
    Query(q): Query<SavedQuery>,
) -> Result<Response> {
    let kind = match q.kind.as_deref().filter(|k| !k.is_empty()) {
        Some(k) => Some(SavedKind::parse(k).ok_or_else(|| AccountsError::invalid("unknown kind"))?),
        None => None,
    };
    let filter = SavedFilter {
        kind,
        reference: q.reference.filter(|r| !r.is_empty()),
        org_id: q.org.filter(|o| !o.is_empty()),
        limit: Some(q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)),
    };
    let items = state.store.list_saved(&s.info.user.id, &filter)?;
    Ok(json_response(StatusCode::OK, &json!({ "items": items })))
}

pub(crate) async fn create_saved(
    State(state): State<AppState>,
    s: Session,
    Body(b): Body<NewSaved>,
) -> Result<Response> {
    let fields = SavedFields {
        kind: SavedKind::parse(&b.kind).ok_or_else(|| AccountsError::invalid("unknown kind"))?,
        title: clean_title(&b.title)?,
        note: clean_note(b.note)?,
        payload: clean_payload(b.payload)?,
        refs: clean_refs(b.refs)?,
        atlas_snapshot: clean_snapshot(b.atlas_snapshot)?,
        org_id: b.org_id.filter(|o| !o.is_empty()),
    };
    let item = state.store.create_saved(&s.info.user.id, fields, now())?;
    Ok(json_response(StatusCode::CREATED, &item))
}

pub(crate) async fn get_saved(State(state): State<AppState>, s: Session, Path(id): Path<String>) -> Result<Response> {
    let item = state
        .store
        .saved(&s.info.user.id, &id)?
        .ok_or(AccountsError::NotFound)?;
    Ok(json_response(StatusCode::OK, &item))
}

pub(crate) async fn update_saved(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
    Body(b): Body<SavedPatch>,
) -> Result<Response> {
    let changes = SavedChanges {
        title: b.title.as_deref().map(clean_title).transpose()?,
        note: match b.note {
            Some(n) => Some(clean_note(n)?),
            None => None,
        },
        payload: b.payload.map(|p| clean_payload(Some(p))).transpose()?,
        refs: b.refs.map(clean_refs).transpose()?,
    };
    let item = state.store.update_saved(&s.info.user.id, &id, changes, now())?;
    Ok(json_response(StatusCode::OK, &item))
}

pub(crate) async fn delete_saved(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
) -> Result<Response> {
    state.store.delete_saved(&s.info.user.id, &id)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<u32>,
}

pub(crate) async fn list_conversations(
    State(state): State<AppState>,
    s: Session,
    Query(q): Query<ListQuery>,
) -> Result<Response> {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let items = state.store.list_conversations(&s.info.user.id, Some(limit))?;
    Ok(json_response(StatusCode::OK, &json!({ "conversations": items })))
}

/// Title: the given one, else the first question (shortened). Messages are optional.
pub(crate) async fn create_conversation(
    State(state): State<AppState>,
    s: Session,
    Body(b): Body<NewConversation>,
) -> Result<Response> {
    if b.messages.len() > MAX_MESSAGES_PER_REQUEST {
        return Err(AccountsError::invalid("too many messages"));
    }
    let messages = b
        .messages
        .into_iter()
        .map(NewMessage::validate)
        .collect::<Result<Vec<_>>>()?;
    let title = match b.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => clean_title(t)?,
        None => {
            let first = messages
                .iter()
                .find(|m| m.role == crate::model::MessageRole::User)
                .ok_or_else(|| AccountsError::invalid("give a title or start with a question"))?;
            shorten(&first.content, 80)
        }
    };
    let locale = clean_locale(b.locale)?;
    let org = b.org_id.filter(|o| !o.is_empty());
    let c = state.store.create_conversation(
        &s.info.user.id,
        &title,
        locale.as_deref(),
        org.as_deref(),
        messages,
        now(),
    )?;
    Ok(json_response(StatusCode::CREATED, &c))
}

pub(crate) async fn get_conversation(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
) -> Result<Response> {
    let c = state
        .store
        .conversation(&s.info.user.id, &id)?
        .ok_or(AccountsError::NotFound)?;
    Ok(json_response(StatusCode::OK, &c))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenameBody {
    title: String,
}

pub(crate) async fn rename_conversation(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
    Body(b): Body<RenameBody>,
) -> Result<Response> {
    let title = clean_title(&b.title)?;
    state.store.rename_conversation(&s.info.user.id, &id, &title, now())?;
    let c = state
        .store
        .conversation(&s.info.user.id, &id)?
        .ok_or(AccountsError::NotFound)?;
    Ok(json_response(StatusCode::OK, &c.summary))
}

pub(crate) async fn delete_conversation(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
) -> Result<Response> {
    state.store.delete_conversation(&s.info.user.id, &id)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppendBody {
    messages: Vec<NewMessage>,
}

pub(crate) async fn append_messages(
    State(state): State<AppState>,
    s: Session,
    Path(id): Path<String>,
    Body(b): Body<AppendBody>,
) -> Result<Response> {
    if b.messages.is_empty() || b.messages.len() > MAX_MESSAGES_PER_REQUEST {
        return Err(AccountsError::invalid("send between 1 and 200 messages"));
    }
    let messages = b
        .messages
        .into_iter()
        .map(NewMessage::validate)
        .collect::<Result<Vec<_>>>()?;
    let stored = state.store.append_messages(&s.info.user.id, &id, messages, now())?;
    Ok(json_response(StatusCode::CREATED, &json!({ "messages": stored })))
}

fn shorten(s: &str, max_chars: usize) -> String {
    let line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max_chars {
        return line;
    }
    let mut out: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortens_on_char_boundaries() {
        assert_eq!(super::shorten("Was  ist\nDravet?", 80), "Was ist Dravet?");
        assert_eq!(super::shorten("äöüäöü", 4), "äöü…");
    }
}
