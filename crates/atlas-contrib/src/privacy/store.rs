//! SQLite table of privacy requests (own connection; same database file as contributions).
//!
//! Retention (design/API.md, "Privacy requests"): the e-mail and the `concerns` text are needed to
//! verify identity and reply, so they are kept until the decision plus [`Retention::reply_days`];
//! then they are erased and only the compliance record stays (reference, type, dates, outcome,
//! reviewer, suppression entry id, salted e-mail hash). The compliance record is deleted
//! [`Retention::record_days`] after the decision.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OptionalExtension, params};

use super::{PrivacyRequest, RequestState, RequestType};
use crate::error::{ContribError, Result};
use crate::store::Database;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS privacy_requests (
    reference          TEXT PRIMARY KEY,
    kind               TEXT NOT NULL,
    state              TEXT NOT NULL,
    email              TEXT,
    concerns           TEXT,
    email_hash         TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    respond_by         TEXT NOT NULL,
    decided_at         TEXT,
    reviewer           TEXT,
    decision_reason    TEXT,
    suppression_entry  TEXT,
    redacted_at        TEXT,
    delete_after       TEXT
);
CREATE INDEX IF NOT EXISTS privacy_requests_state ON privacy_requests(state, created_at);
CREATE INDEX IF NOT EXISTS privacy_requests_email ON privacy_requests(email_hash, created_at);
";

/// How long personal fields and compliance records are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    /// After the decision: e-mail + concerns kept for the reply and a follow-up question.
    pub reply_days: u64,
    /// After the decision: the minimal compliance record (proof the request was handled).
    pub record_days: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            reply_days: 30,
            record_days: 3 * 365,
        }
    }
}

pub struct PrivacyStore {
    conn: Mutex<Connection>,
}

fn rfc(t: SystemTime) -> String {
    humantime::format_rfc3339_seconds(t).to_string()
}

fn days(n: u64) -> Duration {
    Duration::from_secs(n * 86_400)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PrivacyRequest> {
    let kind: String = r.get("kind")?;
    let state: String = r.get("state")?;
    Ok(PrivacyRequest {
        reference: r.get("reference")?,
        kind: RequestType::parse(&kind).unwrap_or(RequestType::Remove),
        state: RequestState::parse(&state).unwrap_or(RequestState::Received),
        email: r.get("email")?,
        concerns: r.get("concerns")?,
        email_hash: r.get("email_hash")?,
        created_at: r.get("created_at")?,
        respond_by: r.get("respond_by")?,
        decided_at: r.get("decided_at")?,
        reviewer: r.get("reviewer")?,
        decision_reason: r.get("decision_reason")?,
        suppression_entry: r.get("suppression_entry")?,
        redacted_at: r.get("redacted_at")?,
        delete_after: r.get("delete_after")?,
    })
}

impl PrivacyStore {
    pub fn open(db: &Database) -> Result<Self> {
        let conn = match db {
            Database::InMemory => Connection::open_in_memory()?,
            Database::File(path) => {
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| ContribError::Internal(format!("creating {}: {e}", dir.display())))?;
                }
                let c = Connection::open(path)?;
                c.pragma_update(None, "journal_mode", "WAL")?;
                c
            }
        };
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn insert(&self, r: &PrivacyRequest) -> Result<()> {
        self.lock().execute(
            "INSERT INTO privacy_requests (reference, kind, state, email, concerns, email_hash, created_at, respond_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                r.reference,
                r.kind.as_str(),
                r.state.as_str(),
                r.email,
                r.concerns,
                r.email_hash,
                r.created_at,
                r.respond_by
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, reference: &str) -> Result<PrivacyRequest> {
        self.lock()
            .query_row("SELECT * FROM privacy_requests WHERE reference = ?1", [reference], row)
            .optional()?
            .ok_or(ContribError::NotFound)
    }

    pub fn list(&self, states: &[RequestState], limit: u32) -> Result<Vec<PrivacyRequest>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM privacy_requests ORDER BY created_at ASC LIMIT ?1")?;
        let all = stmt
            .query_map([i64::from(limit) * 4], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(all
            .into_iter()
            .filter(|r| states.is_empty() || states.contains(&r.state))
            .take(limit as usize)
            .collect())
    }

    /// Requests from the same e-mail hash since `since` (RFC 3339), for the per-address limit.
    pub fn count_since(&self, email_hash: &str, since: &str) -> Result<u32> {
        Ok(self.lock().query_row(
            "SELECT COUNT(*) FROM privacy_requests WHERE email_hash = ?1 AND created_at >= ?2",
            params![email_hash, since],
            |r| r.get(0),
        )?)
    }

    pub fn count_all_since(&self, since: &str) -> Result<u32> {
        Ok(self.lock().query_row(
            "SELECT COUNT(*) FROM privacy_requests WHERE created_at >= ?1",
            [since],
            |r| r.get(0),
        )?)
    }

    pub fn decide(
        &self,
        reference: &str,
        state: RequestState,
        reviewer: &str,
        reason: Option<&str>,
        entry: Option<&str>,
    ) -> Result<()> {
        let now = rfc(SystemTime::now());
        let n = self.lock().execute(
            "UPDATE privacy_requests SET state = ?2, reviewer = ?3, decision_reason = ?4,
                 suppression_entry = COALESCE(?5, suppression_entry),
                 decided_at = CASE WHEN ?2 IN ('approved', 'rejected') THEN ?6 ELSE decided_at END
             WHERE reference = ?1",
            params![reference, state.as_str(), reviewer, reason, entry, now],
        )?;
        if n == 0 {
            return Err(ContribError::NotFound);
        }
        Ok(())
    }

    /// Apply retention at `now`: erase e-mail + concerns `reply_days` after the decision, delete the
    /// compliance record `record_days` after it. Returns (redacted, deleted).
    pub fn purge(&self, retention: Retention, now: SystemTime) -> Result<(usize, usize)> {
        let redact_before = rfc(now - days(retention.reply_days));
        let delete_before = rfc(now - days(retention.record_days));
        let now_s = rfc(now);
        let conn = self.lock();
        let redacted = conn.execute(
            "UPDATE privacy_requests SET email = NULL, concerns = NULL, redacted_at = ?2,
                 delete_after = ?3
             WHERE decided_at IS NOT NULL AND decided_at < ?1 AND redacted_at IS NULL",
            params![
                redact_before,
                now_s,
                rfc(now + days(retention.record_days - retention.reply_days))
            ],
        )?;
        let deleted = conn.execute(
            "DELETE FROM privacy_requests WHERE decided_at IS NOT NULL AND decided_at < ?1",
            [delete_before],
        )?;
        Ok((redacted, deleted))
    }

    /// Test hook: move a request's decision into the past.
    #[cfg(test)]
    pub fn backdate_decision(&self, reference: &str, decided_at: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE privacy_requests SET decided_at = ?2 WHERE reference = ?1",
            params![reference, decided_at],
        )?;
        Ok(())
    }
}
