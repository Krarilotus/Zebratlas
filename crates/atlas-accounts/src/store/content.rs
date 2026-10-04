//! What signed-in people keep: saved items, conversations with the atlas, and the full data export.
//!
//! Access rule: an item is readable by its creator and by members of the organisation it is shared
//! with; writable by its creator and by that organisation's owners and admins.

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::{Value, json};

use super::Store;
use crate::error::{AccountsError, Result};
use crate::model::{CleanMessage, Conversation, ConversationSummary, Message, MessageRole, SavedItem, SavedKind};
use crate::util::{new_id, rfc3339};

/// Filters for listing saved items.
#[derive(Clone, Debug, Default)]
pub struct SavedFilter {
    pub kind: Option<SavedKind>,
    /// Only items that reference this atlas id.
    pub reference: Option<String>,
    /// Items shared with this organisation instead of the user's own.
    pub org_id: Option<String>,
    /// `None` = all.
    pub limit: Option<u32>,
}

/// Validated fields of a new saved item.
pub struct SavedFields {
    pub kind: SavedKind,
    pub title: String,
    pub note: Option<String>,
    pub payload: Value,
    pub refs: Vec<String>,
    pub atlas_snapshot: Option<String>,
    pub org_id: Option<String>,
}

pub struct SavedChanges {
    pub title: Option<String>,
    pub note: Option<Option<String>>,
    pub payload: Option<Value>,
    pub refs: Option<Vec<String>>,
}

const SAVED_COLUMNS: &str =
    "s.id, s.kind, s.title, s.note, s.payload, s.atlas_snapshot, s.org_id, s.created_at, s.updated_at";

/// Visible to the user: own items or items shared with one of their organisations.
const CAN_READ: &str = "(s.user_id = ?1 OR s.org_id IN (SELECT org_id FROM memberships WHERE user_id = ?1))";
const CAN_WRITE: &str = "(s.user_id = ?1 OR s.org_id IN (SELECT org_id FROM memberships WHERE user_id = ?1 AND role IN ('owner', 'admin')))";

impl Store {
    // -----------------------------------------------------------------------------------------
    // Saved items
    // -----------------------------------------------------------------------------------------

    pub fn create_saved(&self, user_id: &str, f: SavedFields, now: i64) -> Result<SavedItem> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if let Some(org) = &f.org_id {
            require_member(&tx, user_id, org)?;
        }
        let id = new_id("sav");
        tx.execute(
            "INSERT INTO saved_items (id, user_id, org_id, kind, title, note, payload, atlas_snapshot, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id,
                user_id,
                f.org_id,
                f.kind.as_str(),
                f.title,
                f.note,
                f.payload.to_string(),
                f.atlas_snapshot,
                now
            ],
        )?;
        write_refs(&tx, &id, &f.refs)?;
        tx.commit()?;
        Ok(SavedItem {
            id,
            kind: f.kind,
            title: f.title,
            note: f.note,
            payload: f.payload,
            refs: f.refs,
            atlas_snapshot: f.atlas_snapshot,
            org_id: f.org_id,
            created_at: rfc3339(now),
            updated_at: rfc3339(now),
        })
    }

    pub fn list_saved(&self, user_id: &str, filter: &SavedFilter) -> Result<Vec<SavedItem>> {
        let conn = self.conn();
        if let Some(org) = &filter.org_id {
            require_member(&conn, user_id, org)?;
        }
        let scope = if filter.org_id.is_some() {
            "s.org_id = ?2 AND s.org_id IN (SELECT org_id FROM memberships WHERE user_id = ?1)"
        } else {
            "s.user_id = ?1 AND ?2 IS NULL"
        };
        let sql = format!(
            "SELECT {SAVED_COLUMNS} FROM saved_items s
             WHERE {scope}
               AND (?3 IS NULL OR s.kind = ?3)
               AND (?4 IS NULL OR EXISTS (SELECT 1 FROM saved_item_refs r WHERE r.item_id = s.id AND r.ref_id = ?4))
             ORDER BY s.updated_at DESC, s.id LIMIT ?5"
        );
        let mut st = conn.prepare(&sql)?;
        // SQLite: LIMIT -1 means no limit (the export wants everything).
        let limit = filter.limit.map_or(-1, i64::from);
        let rows = st.query_map(
            params![
                user_id,
                filter.org_id,
                filter.kind.map(SavedKind::as_str),
                filter.reference,
                limit
            ],
            saved_row,
        )?;
        let mut items: Vec<SavedItem> = rows.collect::<rusqlite::Result<_>>()?;
        for item in &mut items {
            item.refs = read_refs(&conn, &item.id)?;
        }
        Ok(items)
    }

    pub fn saved(&self, user_id: &str, id: &str) -> Result<Option<SavedItem>> {
        let conn = self.conn();
        let sql = format!("SELECT {SAVED_COLUMNS} FROM saved_items s WHERE s.id = ?2 AND {CAN_READ}");
        let Some(mut item) = conn.query_row(&sql, params![user_id, id], saved_row).optional()? else {
            return Ok(None);
        };
        item.refs = read_refs(&conn, &item.id)?;
        Ok(Some(item))
    }

    pub fn update_saved(&self, user_id: &str, id: &str, c: SavedChanges, now: i64) -> Result<SavedItem> {
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            check_access(&tx, "saved_items", user_id, id)?;
            if let Some(title) = &c.title {
                tx.execute("UPDATE saved_items SET title = ?2 WHERE id = ?1", params![id, title])?;
            }
            if let Some(note) = &c.note {
                tx.execute("UPDATE saved_items SET note = ?2 WHERE id = ?1", params![id, note])?;
            }
            if let Some(payload) = &c.payload {
                tx.execute(
                    "UPDATE saved_items SET payload = ?2 WHERE id = ?1",
                    params![id, payload.to_string()],
                )?;
            }
            if let Some(refs) = &c.refs {
                tx.execute("DELETE FROM saved_item_refs WHERE item_id = ?1", [id])?;
                write_refs(&tx, id, refs)?;
            }
            tx.execute("UPDATE saved_items SET updated_at = ?2 WHERE id = ?1", params![id, now])?;
            tx.commit()?;
        }
        self.saved(user_id, id)?.ok_or(AccountsError::NotFound)
    }

    pub fn delete_saved(&self, user_id: &str, id: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_access(&tx, "saved_items", user_id, id)?;
        tx.execute("DELETE FROM saved_items WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Conversations
    // -----------------------------------------------------------------------------------------

    pub fn create_conversation(
        &self,
        user_id: &str,
        title: &str,
        locale: Option<&str>,
        org_id: Option<&str>,
        messages: Vec<CleanMessage>,
        now: i64,
    ) -> Result<Conversation> {
        let id = new_id("cnv");
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            if let Some(org) = org_id {
                require_member(&tx, user_id, org)?;
            }
            tx.execute(
                "INSERT INTO conversations (id, user_id, org_id, title, locale, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![id, user_id, org_id, title, locale, now],
            )?;
            for (seq, m) in messages.into_iter().enumerate() {
                insert_message(&tx, &id, seq as i64 + 1, m, now)?;
            }
            tx.commit()?;
        }
        self.conversation(user_id, &id)?.ok_or(AccountsError::NotFound)
    }

    pub fn list_conversations(&self, user_id: &str, limit: Option<u32>) -> Result<Vec<ConversationSummary>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT c.id, c.title, c.locale, c.org_id, c.created_at, c.updated_at,
                    (SELECT count(*) FROM conversation_messages m WHERE m.conversation_id = c.id)
             FROM conversations c WHERE c.user_id = ?1
             ORDER BY c.updated_at DESC, c.id LIMIT ?2",
        )?;
        let rows = st.query_map(params![user_id, limit.map_or(-1, i64::from)], summary_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn conversation(&self, user_id: &str, id: &str) -> Result<Option<Conversation>> {
        let conn = self.conn();
        let summary = conn
            .query_row(
                "SELECT s.id, s.title, s.locale, s.org_id, s.created_at, s.updated_at,
                        (SELECT count(*) FROM conversation_messages m WHERE m.conversation_id = s.id)
                 FROM conversations s WHERE s.id = ?2 AND (s.user_id = ?1 OR s.org_id IN
                     (SELECT org_id FROM memberships WHERE user_id = ?1))",
                params![user_id, id],
                summary_row,
            )
            .optional()?;
        let Some(summary) = summary else { return Ok(None) };
        let messages = read_messages(&conn, id)?;
        Ok(Some(Conversation { summary, messages }))
    }

    /// Appends messages; returns the stored ones.
    pub fn append_messages(
        &self,
        user_id: &str,
        id: &str,
        messages: Vec<CleanMessage>,
        now: i64,
    ) -> Result<Vec<Message>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_access(&tx, "conversations", user_id, id)?;
        let last: i64 = tx.query_row(
            "SELECT coalesce(max(seq), 0) FROM conversation_messages WHERE conversation_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        let mut ids = Vec::with_capacity(messages.len());
        for (i, m) in messages.into_iter().enumerate() {
            ids.push(insert_message(&tx, id, last + 1 + i as i64, m, now)?);
        }
        tx.execute(
            "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        let all = read_messages(&tx, id)?;
        tx.commit()?;
        Ok(all.into_iter().filter(|m| ids.contains(&m.id)).collect())
    }

    pub fn rename_conversation(&self, user_id: &str, id: &str, title: &str, now: i64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_access(&tx, "conversations", user_id, id)?;
        tx.execute(
            "UPDATE conversations SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_conversation(&self, user_id: &str, id: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_access(&tx, "conversations", user_id, id)?;
        tx.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Export (GDPR Art. 15 / 20): everything stored about the user, machine-readable.
    // -----------------------------------------------------------------------------------------

    /// All data held about the user. Secrets are left out: password hashes and session tokens (only
    /// their hashes are stored) are described, not exported.
    pub fn export(&self, user_id: &str, now: i64) -> Result<Value> {
        let user = self.user(user_id)?.ok_or(AccountsError::NotFound)?;
        let memberships = self.memberships(user_id)?;
        let saved = self.list_saved(user_id, &SavedFilter::default())?;
        let summaries = self.list_conversations(user_id, None)?;
        let mut conversations = Vec::with_capacity(summaries.len());
        for s in summaries {
            if let Some(c) = self.conversation(user_id, &s.id)? {
                conversations.push(c);
            }
        }
        let conn = self.conn();
        let credentials: Vec<Value> = {
            let mut st = conn.prepare(
                "SELECT kind, label, created_at, last_used_at FROM credentials WHERE user_id = ?1 ORDER BY created_at",
            )?;
            st.query_map([user_id], |r| {
                Ok(json!({
                    "kind": r.get::<_, String>(0)?,
                    "label": r.get::<_, Option<String>>(1)?,
                    "created_at": rfc3339(r.get(2)?),
                    "last_used_at": r.get::<_, Option<i64>>(3)?.map(rfc3339),
                }))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let sessions: Vec<Value> = {
            let mut st = conn.prepare(
                "SELECT id, auth_method, created_at, expires_at, last_seen_at FROM sessions WHERE user_id = ?1 ORDER BY created_at",
            )?;
            st.query_map([user_id], |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "auth_method": r.get::<_, String>(1)?,
                    "created_at": rfc3339(r.get(2)?),
                    "expires_at": rfc3339(r.get(3)?),
                    "last_seen_at": rfc3339(r.get(4)?),
                }))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        Ok(json!({
            "format": "rare-disease-atlas/account-export",
            "format_version": 1,
            "exported_at": rfc3339(now),
            "notes": [
                crate::copy::msg("account.export.credentials", json!({}))["fallback"],
                crate::copy::msg("account.export.public_data", json!({}))["fallback"],
                crate::copy::msg("account.export.network_data", json!({}))["fallback"]
            ],
            "notes_msg": [
                crate::copy::msg("account.export.credentials", json!({})),
                crate::copy::msg("account.export.public_data", json!({})),
                crate::copy::msg("account.export.network_data", json!({}))
            ],
            "user": user,
            "credentials": credentials,
            "sessions": sessions,
            "organisations": memberships,
            "saved_items": saved,
            "conversations": conversations,
        }))
    }
}

fn require_member(conn: &Connection, user_id: &str, org_id: &str) -> Result<()> {
    let ok: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM memberships WHERE user_id = ?1 AND org_id = ?2)",
        params![user_id, org_id],
        |r| r.get(0),
    )?;
    if ok { Ok(()) } else { Err(AccountsError::Forbidden) }
}

/// `NotFound` if the user can't see the row, `Forbidden` if they can see but not change it.
fn check_access(tx: &Transaction<'_>, table: &str, user_id: &str, id: &str) -> Result<()> {
    let (readable, writable): (bool, bool) = tx
        .query_row(
            &format!("SELECT {CAN_READ}, {CAN_WRITE} FROM {table} s WHERE s.id = ?2"),
            params![user_id, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((false, false));
    match (readable, writable) {
        (_, true) => Ok(()),
        (true, false) => Err(AccountsError::Forbidden),
        (false, false) => Err(AccountsError::NotFound),
    }
}

fn write_refs(tx: &Transaction<'_>, item_id: &str, refs: &[String]) -> Result<()> {
    let mut st = tx.prepare("INSERT INTO saved_item_refs (item_id, position, ref_id) VALUES (?1, ?2, ?3)")?;
    for (i, r) in refs.iter().enumerate() {
        st.execute(params![item_id, i as i64, r])?;
    }
    Ok(())
}

fn read_refs(conn: &Connection, item_id: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare_cached("SELECT ref_id FROM saved_item_refs WHERE item_id = ?1 ORDER BY position")?;
    let rows = st.query_map([item_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn saved_row(r: &Row<'_>) -> rusqlite::Result<SavedItem> {
    let kind: String = r.get(1)?;
    let payload: String = r.get(4)?;
    Ok(SavedItem {
        id: r.get(0)?,
        kind: SavedKind::parse(&kind).unwrap_or(SavedKind::Node),
        title: r.get(2)?,
        note: r.get(3)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        refs: Vec::new(),
        atlas_snapshot: r.get(5)?,
        org_id: r.get(6)?,
        created_at: rfc3339(r.get(7)?),
        updated_at: rfc3339(r.get(8)?),
    })
}

fn summary_row(r: &Row<'_>) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        locale: r.get(2)?,
        org_id: r.get(3)?,
        created_at: rfc3339(r.get(4)?),
        updated_at: rfc3339(r.get(5)?),
        message_count: r.get(6)?,
    })
}

fn insert_message(tx: &Transaction<'_>, conversation_id: &str, seq: i64, m: CleanMessage, now: i64) -> Result<String> {
    let id = new_id("msg");
    tx.execute(
        "INSERT INTO conversation_messages (id, conversation_id, seq, role, content, citations, meta, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            conversation_id,
            seq,
            m.role.as_str(),
            m.content,
            Value::Array(m.citations).to_string(),
            m.meta.map(|v| v.to_string()),
            now
        ],
    )?;
    Ok(id)
}

fn read_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<Message>> {
    let mut st = conn.prepare_cached(
        "SELECT id, seq, role, content, citations, meta, created_at FROM conversation_messages
         WHERE conversation_id = ?1 ORDER BY seq",
    )?;
    let rows = st.query_map([conversation_id], |r| {
        let citations: String = r.get(4)?;
        let meta: Option<String> = r.get(5)?;
        Ok(Message {
            id: r.get(0)?,
            seq: r.get(1)?,
            role: MessageRole::parse(&r.get::<_, String>(2)?).unwrap_or(MessageRole::User),
            content: r.get(3)?,
            citations: serde_json::from_str::<Vec<Value>>(&citations).unwrap_or_default(),
            meta: meta.and_then(|m| serde_json::from_str(&m).ok()),
            created_at: rfc3339(r.get(6)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
