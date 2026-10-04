//! Domain types (what the API returns) and input validation (what it accepts).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AccountsError, Result};

/// Commercial plan. A flag only: there is no billing; organisations get it set by an operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    Free,
    Organisation,
    Enterprise,
}

impl Plan {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Organisation => "organisation",
            Self::Enterprise => "enterprise",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "free" => Some(Self::Free),
            "organisation" => Some(Self::Organisation),
            "enterprise" => Some(Self::Enterprise),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }

    pub fn can_manage(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

/// What a saved item is. The payload shape is owned by the screen that saves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedKind {
    /// A condition the person follows (refs: the disease id).
    Condition,
    /// A connection card: a group, study, registry, expert or researcher, with why and how to reach them.
    ConnectionCard,
    /// A connection map / subgraph view (refs: its node and edge ids).
    ConnectionMap,
    /// A ready-to-send message draft.
    MessageDraft,
    /// Any other atlas node (gene, phenotype, organisation, paper ...).
    Node,
    /// A search the person wants to rerun.
    Search,
}

pub const SAVED_KINDS: [SavedKind; 6] = [
    SavedKind::Condition,
    SavedKind::ConnectionCard,
    SavedKind::ConnectionMap,
    SavedKind::MessageDraft,
    SavedKind::Node,
    SavedKind::Search,
];

impl SavedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Condition => "condition",
            Self::ConnectionCard => "connection_card",
            Self::ConnectionMap => "connection_map",
            Self::MessageDraft => "message_draft",
            Self::Node => "node",
            Self::Search => "search",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        SAVED_KINDS.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub locale: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Organisation {
    pub id: String,
    pub name: String,
    pub plan: Plan,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Membership {
    #[serde(flatten)]
    pub organisation: Organisation,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedItem {
    pub id: String,
    pub kind: SavedKind,
    pub title: String,
    pub note: Option<String>,
    /// Screen-owned JSON (e.g. the connection card as shown, the draft text).
    pub payload: Value,
    /// Atlas node / edge ids this item points at, in order.
    pub refs: Vec<String>,
    /// Snapshot or version of the atlas the item was saved from, so it can be re-verified later.
    pub atlas_snapshot: Option<String>,
    /// Organisation workspace the item is shared with (`null` = personal).
    pub org_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub id: String,
    pub seq: i64,
    pub role: MessageRole,
    pub content: String,
    /// Citations behind the answer: objects such as `{"id": "MONDO:...", "label": "...", "url": "..."}`.
    pub citations: Vec<Value>,
    /// Generation metadata (provider, model, prompt hash ...), PROV-O style; `null` for user turns.
    pub meta: Option<Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub locale: Option<String>,
    pub org_id: Option<String>,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Conversation {
    #[serde(flatten)]
    pub summary: ConversationSummary,
    pub messages: Vec<Message>,
}

// ---------------------------------------------------------------------------------------------
// Inputs and validation
// ---------------------------------------------------------------------------------------------

pub const MAX_TITLE: usize = 200;
pub const MAX_NOTE: usize = 4_000;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_REFS: usize = 500;
pub const MAX_REF_LEN: usize = 200;
pub const MAX_MESSAGE: usize = 32 * 1024;
pub const MAX_CITATIONS: usize = 200;
pub const MAX_MESSAGES_PER_REQUEST: usize = 200;
pub const MAX_NAME: usize = 80;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewSaved {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub atlas_snapshot: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPatch {
    pub title: Option<String>,
    /// `Some(None)` clears the note.
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
    pub payload: Option<Value>,
    pub refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub citations: Vec<Value>,
    #[serde(default)]
    pub meta: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewConversation {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<NewMessage>,
}

/// Distinguishes "field absent" (`None`) from "field is null" (`Some(None)`).
pub(crate) fn double_option<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// A validated message, ready to store.
pub struct CleanMessage {
    pub role: MessageRole,
    pub content: String,
    pub citations: Vec<Value>,
    pub meta: Option<Value>,
}

impl NewMessage {
    pub fn validate(self) -> Result<CleanMessage> {
        let role =
            MessageRole::parse(&self.role).ok_or_else(|| AccountsError::invalid("role must be user or assistant"))?;
        if self.content.trim().is_empty() {
            return Err(AccountsError::invalid("message content is empty"));
        }
        if self.content.len() > MAX_MESSAGE {
            return Err(AccountsError::invalid("message is too long"));
        }
        if self.citations.len() > MAX_CITATIONS {
            return Err(AccountsError::invalid("too many citations"));
        }
        if !self.citations.iter().all(Value::is_object) {
            return Err(AccountsError::invalid("each citation must be an object"));
        }
        let size = serde_json::to_vec(&self.citations)
            .map(|v| v.len())
            .unwrap_or(usize::MAX)
            + self
                .meta
                .as_ref()
                .and_then(|m| serde_json::to_vec(m).ok())
                .map_or(0, |v| v.len());
        if size > MAX_PAYLOAD_BYTES {
            return Err(AccountsError::invalid("citations are too large"));
        }
        Ok(CleanMessage {
            role,
            content: self.content,
            citations: self.citations,
            meta: self.meta.filter(|m| !m.is_null()),
        })
    }
}

pub fn clean_title(title: &str) -> Result<String> {
    let t = single_line(title);
    if t.is_empty() {
        return Err(AccountsError::invalid("title is empty"));
    }
    if t.chars().count() > MAX_TITLE {
        return Err(AccountsError::invalid("title is too long"));
    }
    Ok(t)
}

pub fn clean_note(note: Option<String>) -> Result<Option<String>> {
    let Some(n) = note else { return Ok(None) };
    let n = n.trim().to_string();
    if n.is_empty() {
        return Ok(None);
    }
    if n.chars().count() > MAX_NOTE {
        return Err(AccountsError::invalid("note is too long"));
    }
    Ok(Some(n))
}

pub fn clean_payload(payload: Option<Value>) -> Result<Value> {
    let payload = payload.unwrap_or(Value::Object(Default::default()));
    let size = serde_json::to_vec(&payload).map(|v| v.len()).unwrap_or(usize::MAX);
    if size > MAX_PAYLOAD_BYTES {
        return Err(AccountsError::invalid("payload is too large"));
    }
    Ok(payload)
}

/// Atlas ids: CURIE-like tokens (`MONDO:0007915`, `HGNC:11444`, `edge:...`), no whitespace.
pub fn clean_refs(refs: Vec<String>) -> Result<Vec<String>> {
    if refs.len() > MAX_REFS {
        return Err(AccountsError::invalid("too many references"));
    }
    let mut out: Vec<String> = Vec::with_capacity(refs.len());
    for r in refs {
        let r = r.trim().to_string();
        if r.is_empty() || r.len() > MAX_REF_LEN || r.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(AccountsError::invalid("references must be atlas ids without spaces"));
        }
        if !out.contains(&r) {
            out.push(r);
        }
    }
    Ok(out)
}

pub fn clean_snapshot(s: Option<String>) -> Result<Option<String>> {
    let Some(s) = s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if s.len() > MAX_REF_LEN || s.chars().any(char::is_control) {
        return Err(AccountsError::invalid("atlas_snapshot is invalid"));
    }
    Ok(Some(s))
}

pub fn clean_display_name(name: Option<String>) -> Result<Option<String>> {
    let Some(n) = name.map(|n| single_line(&n)).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if n.chars().count() > MAX_NAME {
        return Err(AccountsError::invalid("name is too long"));
    }
    Ok(Some(n))
}

pub fn clean_org_name(name: &str) -> Result<String> {
    clean_display_name(Some(name.to_string()))?.ok_or_else(|| AccountsError::invalid("organisation name is empty"))
}

/// BCP 47-ish language tag (`en`, `zh-Hans`, `pt-BR`).
pub fn clean_locale(locale: Option<String>) -> Result<Option<String>> {
    let Some(l) = locale.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()) else {
        return Ok(None);
    };
    let ok = l.len() <= 16
        && l.split('-')
            .all(|p| !p.is_empty() && p.len() <= 8 && p.chars().all(|c| c.is_ascii_alphanumeric()));
    if !ok {
        return Err(AccountsError::invalid(
            "locale must be a language tag such as en or zh-Hans",
        ));
    }
    Ok(Some(l))
}

/// Normalised email: trimmed, lower-cased; a plausibility check only (no confirmation mail in the demo).
pub fn clean_email(email: &str) -> Result<String> {
    let e = email.trim().to_lowercase();
    let bad = || AccountsError::invalid("enter a valid email address");
    if e.len() < 3 || e.len() > 254 || e.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad());
    }
    let (local, domain) = e.split_once('@').ok_or_else(bad)?;
    if local.is_empty() || local.len() > 64 || domain.contains('@') || !domain.contains('.') {
        return Err(bad());
    }
    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return Err(bad());
    }
    Ok(e)
}

fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalised_and_checked() {
        assert_eq!(clean_email("  Maria@Example.ORG ").unwrap(), "maria@example.org");
        for bad in ["", "no-at", "a@b", "a@@b.c", "a b@c.de", "@x.de", "a@.de", "a@x.de."] {
            assert!(clean_email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refs_are_trimmed_deduplicated_and_rejected_with_spaces() {
        let refs = clean_refs(vec![" MONDO:1 ".into(), "MONDO:1".into(), "HGNC:2".into()]).unwrap();
        assert_eq!(refs, ["MONDO:1", "HGNC:2"]);
        assert!(clean_refs(vec!["MONDO 1".into()]).is_err());
    }

    #[test]
    fn kinds_round_trip() {
        for k in SAVED_KINDS {
            assert_eq!(SavedKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(SavedKind::parse("conversation"), None);
    }

    #[test]
    fn locales_are_language_tags() {
        assert_eq!(
            clean_locale(Some("zh-Hans".into())).unwrap().as_deref(),
            Some("zh-Hans")
        );
        assert!(clean_locale(Some("en_US; drop".into())).is_err());
    }

    #[test]
    fn messages_need_object_citations() {
        let m = NewMessage {
            role: "assistant".into(),
            content: "Answer".into(),
            citations: vec![serde_json::json!("PMID:1")],
            meta: None,
        };
        assert!(m.validate().is_err());
    }
}
