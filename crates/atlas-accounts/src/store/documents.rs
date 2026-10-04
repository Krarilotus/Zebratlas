//! Rows of encrypted documents and wrapped data keys (D41). This module stores and deletes bytes;
//! encryption lives in [`crate::documents`].

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::error::Result;
use crate::util::{new_id, rfc3339};

/// A stored document: clear metadata plus the sealed payload (title, text, terms).
#[derive(Clone, Debug)]
pub struct DocumentRow {
    pub id: String,
    pub format: String,
    pub bytes: i64,
    pub sha256: String,
    pub terms: i64,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub created_at: String,
}

pub struct NewDocumentRow<'a> {
    pub id: &'a str,
    pub format: &'a str,
    pub bytes: i64,
    pub sha256: &'a str,
    pub terms: i64,
    pub nonce: &'a [u8],
    pub ciphertext: &'a [u8],
}

impl Store {
    /// The user's wrapped data key and its nonce.
    pub fn wrapped_key(&self, user_id: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT wrapped_key, nonce FROM user_keys WHERE user_id = ?1",
                [user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Stores a wrapped key unless the user already has one; returns the key that is stored.
    pub fn put_wrapped_key(&self, user_id: &str, wrapped: &[u8], nonce: &[u8], now: i64) -> Result<(Vec<u8>, Vec<u8>)> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO user_keys (user_id, wrapped_key, nonce, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![user_id, wrapped, nonce, now],
        )?;
        Ok(conn.query_row(
            "SELECT wrapped_key, nonce FROM user_keys WHERE user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    pub fn new_document_id() -> String {
        new_id("doc")
    }

    pub fn insert_document(&self, user_id: &str, d: NewDocumentRow<'_>, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO documents (id, user_id, format, bytes, sha256, terms, nonce, ciphertext, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                d.id,
                user_id,
                d.format,
                d.bytes,
                d.sha256,
                d.terms,
                d.nonce,
                d.ciphertext,
                now
            ],
        )?;
        Ok(())
    }

    fn document_rows(&self, user_id: &str, id: Option<&str>) -> Result<Vec<DocumentRow>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, format, bytes, sha256, terms, nonce, ciphertext, created_at FROM documents
             WHERE user_id = ?1 AND (?2 IS NULL OR id = ?2) ORDER BY created_at DESC, id",
        )?;
        let rows = st
            .query_map(params![user_id, id], |r| {
                Ok(DocumentRow {
                    id: r.get(0)?,
                    format: r.get(1)?,
                    bytes: r.get(2)?,
                    sha256: r.get(3)?,
                    terms: r.get(4)?,
                    nonce: r.get(5)?,
                    ciphertext: r.get(6)?,
                    created_at: rfc3339(r.get(7)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn documents(&self, user_id: &str) -> Result<Vec<DocumentRow>> {
        self.document_rows(user_id, None)
    }

    pub fn document(&self, user_id: &str, id: &str) -> Result<Option<DocumentRow>> {
        Ok(self.document_rows(user_id, Some(id))?.into_iter().next())
    }

    /// Deletes one document of the user; `false` if there was none.
    pub fn delete_document(&self, user_id: &str, id: &str) -> Result<bool> {
        Ok(self.conn().execute(
            "DELETE FROM documents WHERE user_id = ?1 AND id = ?2",
            params![user_id, id],
        )? > 0)
    }

    /// What account deletion will remove, counted before it happens.
    pub fn footprint(&self, user_id: &str) -> Result<Footprint> {
        let conn = self.conn();
        let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [user_id], |r| r.get(0))?) };
        Ok(Footprint {
            sessions: count("SELECT count(*) FROM sessions WHERE user_id = ?1")?,
            saved_items: count("SELECT count(*) FROM saved_items WHERE user_id = ?1")?,
            conversations: count("SELECT count(*) FROM conversations WHERE user_id = ?1")?,
            documents: count("SELECT count(*) FROM documents WHERE user_id = ?1")?,
            data_key: count("SELECT count(*) FROM user_keys WHERE user_id = ?1")? > 0,
        })
    }
}

/// Counts of a user's stored data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Footprint {
    pub sessions: i64,
    pub saved_items: i64,
    pub conversations: i64,
    pub documents: i64,
    pub data_key: bool,
}
