//! Saving conversations for signed-in users (H2), through atlas-accounts.
//!
//! [`ConversationStore`] is the seam (tests use an in-memory one); [`AccountsStore`] opens the
//! accounts SQLite database (the same file the `/api/account` router uses; WAL lets both
//! connections work side by side) and stores each question with its cited answer.

use atlas_accounts::model::{NewMessage, clean_title};
use atlas_accounts::{AccountsConfig, AccountsError, Store};
use axum::http::HeaderMap;
use serde_json::{Value, json};

use crate::converse::AskResponse;

pub trait ConversationStore: Send + Sync {
    /// User id behind a session token, if the session is valid.
    fn user_for_token(&self, token: &str) -> Option<String>;
    /// Append the question and its answer; creates the conversation when `id` is `None`.
    /// Returns the conversation id.
    fn save(&self, user: &str, id: Option<&str>, answer: &AskResponse) -> Result<String, String>;
}

/// The session token the accounts crate issued: `Authorization: Bearer`, else the cookie.
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    atlas_accounts::presented_token(headers, AccountsConfig::from_env().secure_cookies)
}

/// Stored form of an answer: text, the cited facts as citation objects, generation metadata.
pub fn stored_answer(a: &AskResponse) -> (String, Vec<Value>, Value) {
    let text = a.answer.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ");
    let cited: Vec<&str> = a
        .answer
        .iter()
        .flat_map(|s| s.cites.iter().map(String::as_str))
        .collect();
    let citations = a
        .facts
        .iter()
        .filter(|f| cited.contains(&f.key.as_str()))
        .map(|f| json!({"key": f.key, "id": f.id, "edge": f.edge, "text": f.text, "source": f.source, "url": f.url}))
        .collect();
    let chips: Vec<Value> = a
        .chips
        .iter()
        .map(|c| json!({"intent": c.intent, "slots": c.slots, "resolved": c.resolved.iter().map(|(k, r)| (k.clone(), r.node.id.clone())).collect::<std::collections::BTreeMap<_, _>>()}))
        .collect();
    let meta = json!({
        "sentences": a.answer,
        "chips": chips,
        "origin": a.origin,
        "lang": a.lang,
        "validator_passed": a.validation.passed,
        "calls": a.calls,
    });
    (text, citations, meta)
}

pub struct AccountsStore {
    store: Store,
}

impl AccountsStore {
    pub fn open(config: &AccountsConfig) -> Result<Self, AccountsError> {
        Ok(Self {
            store: Store::open(&config.database)?,
        })
    }

    /// Same database settings as the accounts router (`AccountsConfig::from_env`).
    pub fn from_env() -> Result<Self, AccountsError> {
        Self::open(&AccountsConfig::from_env())
    }

    pub fn from_store(store: Store) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl ConversationStore for AccountsStore {
    fn user_for_token(&self, token: &str) -> Option<String> {
        let hash = atlas_accounts::auth::token_hash(token);
        self.store.session(&hash, now()).ok().flatten().map(|s| s.user.id)
    }

    fn save(&self, user: &str, id: Option<&str>, answer: &AskResponse) -> Result<String, String> {
        let (text, citations, meta) = stored_answer(answer);
        let msg = |role: &str, content: String, citations: Vec<Value>, meta: Option<Value>| {
            NewMessage {
                role: role.into(),
                content,
                citations,
                meta,
            }
            .validate()
            .map_err(|e| e.to_string())
        };
        let messages = vec![
            msg("user", answer.question.clone(), vec![], None)?,
            msg("assistant", text, citations, Some(meta))?,
        ];
        let t = now();
        match id {
            Some(id) => {
                self.store
                    .append_messages(user, id, messages, t)
                    .map_err(|e| e.to_string())?;
                Ok(id.to_owned())
            }
            None => {
                let title: String = answer.question.chars().take(120).collect();
                let title = clean_title(&title).map_err(|e| e.to_string())?;
                self.store
                    .create_conversation(user, &title, Some(&answer.requested_lang), None, messages, t)
                    .map(|c| c.summary.id)
                    .map_err(|e| e.to_string())
            }
        }
    }
}
