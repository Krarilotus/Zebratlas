//! SQLite storage: contributions (one JSON body per row plus indexed columns) and their PROV
//! events. One connection behind a mutex; every state change is one transaction that updates the
//! row only if its version is unchanged and appends the PROV event.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::checks::asserted_edge;
use crate::error::{ContribError, Result};
use crate::model::{Agent, Contribution, ContributorInput, State};
use crate::prov::ProvEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Database {
    /// A SQLite file; parent directories are created on open.
    File(PathBuf),
    InMemory,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS contributions (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    state       TEXT NOT NULL,
    version     INTEGER NOT NULL,
    subject_id  TEXT,
    target_id   TEXT,
    edge        TEXT,
    user_id     TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    body        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS contributions_state ON contributions(state, created_at);
CREATE INDEX IF NOT EXISTS contributions_subject ON contributions(subject_id);
CREATE INDEX IF NOT EXISTS contributions_target ON contributions(target_id);
CREATE INDEX IF NOT EXISTS contributions_edge ON contributions(edge);
CREATE TABLE IF NOT EXISTS prov_events (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    contribution TEXT NOT NULL REFERENCES contributions(id),
    body         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS prov_events_contribution ON prov_events(contribution, seq);
PRAGMA user_version = 1;
";

/// Filter for listings. Empty `states` = any state.
#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub states: Vec<State>,
    /// Contributions whose subject or target is this node id.
    pub node: Option<String>,
    /// Contributions about (or asserting) this edge id.
    pub edge: Option<String>,
    pub limit: Option<u32>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(db: &Database) -> Result<Self> {
        let conn = match db {
            Database::InMemory => Connection::open_in_memory()?,
            Database::File(path) => {
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| ContribError::Internal(format!("creating {}: {e}", dir.display())))?;
                }
                let conn = Connection::open(path)?;
                conn.pragma_update(None, "journal_mode", "WAL")?;
                conn
            }
        };
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "secure_delete", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> Result<Self> {
        Self::open(&Database::InMemory)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn insert(&self, c: &Contribution, event: &ProvEvent) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (subject, target, edge) = index_columns(c);
        tx.execute(
            "INSERT INTO contributions (id, kind, state, version, subject_id, target_id, edge, user_id, created_at, updated_at, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                c.id,
                c.kind.as_str(),
                c.state.as_str(),
                c.version,
                subject,
                target,
                edge,
                c.contributor.user_id,
                c.created_at,
                c.updated_at,
                to_json(c)?,
            ],
        )?;
        tx.execute(
            "INSERT INTO prov_events (contribution, body) VALUES (?1, ?2)",
            params![c.id, to_json(event)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Writes `c` if the stored version is still `expected_version`, and appends `event`.
    pub fn update(&self, c: &Contribution, expected_version: u32, event: &ProvEvent) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (subject, target, edge) = index_columns(c);
        let n = tx.execute(
            "UPDATE contributions SET state = ?2, version = ?3, subject_id = ?4, target_id = ?5, edge = ?6,
                 updated_at = ?7, body = ?8, user_id = ?10
             WHERE id = ?1 AND version = ?9",
            params![
                c.id,
                c.state.as_str(),
                c.version,
                subject,
                target,
                edge,
                c.updated_at,
                to_json(c)?,
                expected_version,
                c.contributor.user_id
            ],
        )?;
        if n == 0 {
            return Err(ContribError::Stale);
        }
        tx.execute(
            "INSERT INTO prov_events (contribution, body) VALUES (?1, ?2)",
            params![c.id, to_json(event)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Contribution>> {
        let conn = self.lock();
        let body: Option<String> = conn
            .query_row("SELECT body FROM contributions WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        body.map(|b| from_json(&b)).transpose()
    }

    /// Full private export; the caller must authenticate ownership before returning it.
    pub fn export_for_user(&self, user_id: &str) -> Result<serde_json::Value> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare("SELECT body FROM contributions WHERE user_id = ?1 ORDER BY created_at, id")?;
        let rows = stmt.query_map([user_id], |r| r.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            let c: Contribution = from_json(&row?)?;
            let mut events = tx.prepare("SELECT body FROM prov_events WHERE contribution = ?1 ORDER BY seq")?;
            let history = events
                .query_map([&c.id], |r| r.get::<_, String>(0))?
                .map(|row| from_json::<ProvEvent>(&row?))
                .collect::<Result<Vec<_>>>()?;
            items.push(serde_json::json!({"contribution": c, "history": history}));
        }
        Ok(
            serde_json::json!({"schema": "atlas.contribute.user-export", "version": 1,
            "exported_at": crate::util::now_rfc3339(), "contributions": items}),
        )
    }

    /// Remove structured personal fields and historical account attribution in one transaction.
    /// Credit is retained only on accepted contributions, and never retains the account link.
    pub fn anonymise_for_user(&self, user_id: &str, keep_credit: bool) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let account_agent = format!("agent:user/{user_id}");
        let bodies = {
            let mut stmt = tx.prepare("SELECT body FROM contributions ORDER BY id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut count = 0;
        for body in bodies {
            let before: Contribution = from_json(&body)?;
            let owned = before.contributor.user_id.as_deref() == Some(user_id);
            let mut after = before.clone();
            if owned {
                count += 1;
                after.contributor.user_id = None;
                after.contributor.contact = None;
                after.contributor.organisation = None;
                if !keep_credit || after.state != State::Accepted {
                    after.contributor.name = None;
                }
                after.submission.contributor = ContributorInput::default();
            }
            let anonymous = if owned {
                after.contributor_agent()
            } else {
                Agent::person(
                    format!("agent:anonymous/{}", before.id),
                    Some("anonymous reviewer".into()),
                )
            };
            if let Some(review) = after.review.as_mut()
                && review.reviewer.id == account_agent
            {
                review.reviewer = Agent::person(
                    format!("agent:anonymous-reviewer/{}", before.id),
                    Some("anonymous reviewer".into()),
                );
            }
            let history = {
                let mut stmt = tx.prepare("SELECT seq, body FROM prov_events WHERE contribution = ?1 ORDER BY seq")?;
                stmt.query_map([&before.id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            let mut history_changed = false;
            for (seq, body) in history {
                let mut event: ProvEvent = from_json(&body)?;
                let original = event.clone();
                if event.agent.id == account_agent || (owned && event.activity_type == "submission") {
                    event.agent = anonymous.clone();
                }
                for delegate in &mut event.delegates {
                    if delegate.id == account_agent {
                        *delegate = anonymous.clone();
                    }
                }
                if event != original {
                    history_changed = true;
                    tx.execute(
                        "UPDATE prov_events SET body = ?1 WHERE seq = ?2",
                        params![to_json(&event)?, seq],
                    )?;
                }
            }
            if owned || after != before || history_changed {
                after.version += 1;
                after.updated_at = crate::util::now_rfc3339();
                let event = ProvEvent::new(
                    "account_anonymisation",
                    Agent::software(),
                    Some(&before),
                    &after,
                    after.updated_at.clone(),
                );
                tx.execute(
                    "UPDATE contributions SET user_id = ?1, version = ?2, updated_at = ?3, body = ?4 WHERE id = ?5",
                    params![
                        after.contributor.user_id,
                        after.version,
                        after.updated_at,
                        to_json(&after)?,
                        after.id
                    ],
                )?;
                tx.execute(
                    "INSERT INTO prov_events (contribution, body) VALUES (?1, ?2)",
                    params![after.id, to_json(&event)?],
                )?;
            }
        }
        tx.commit()?;
        // secure_delete clears replaced cells; truncate WAL copies containing the old values.
        let busy = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0))?;
        if busy != 0 {
            return Err(ContribError::Internal(
                "privacy checkpoint is busy; retry anonymisation".into(),
            ));
        }
        Ok(count)
    }

    /// Newest first.
    pub fn list(&self, f: &ListFilter) -> Result<Vec<Contribution>> {
        let mut sql = String::from("SELECT body FROM contributions WHERE 1 = 1");
        let mut args: Vec<String> = Vec::new();
        if !f.states.is_empty() {
            let marks: Vec<String> = f
                .states
                .iter()
                .map(|s| {
                    args.push(s.as_str().to_string());
                    format!("?{}", args.len())
                })
                .collect();
            sql.push_str(&format!(" AND state IN ({})", marks.join(", ")));
        }
        if let Some(node) = &f.node {
            args.push(node.clone());
            let n = args.len();
            sql.push_str(&format!(" AND (subject_id = ?{n} OR target_id = ?{n})"));
        }
        if let Some(edge) = &f.edge {
            args.push(edge.clone());
            sql.push_str(&format!(" AND edge = ?{}", args.len()));
        }
        sql.push_str(" ORDER BY created_at DESC, id");
        if let Some(limit) = f.limit {
            sql.push_str(&format!(" LIMIT {}", limit.min(1000)));
        }
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for body in rows {
            out.push(from_json(&body?)?);
        }
        Ok(out)
    }

    /// Number of contributions per state.
    pub fn counts(&self) -> Result<Vec<(State, u64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT state, COUNT(*) FROM contributions GROUP BY state")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (s, n) = row?;
            if let Some(state) = State::parse(&s) {
                out.push((state, n.max(0) as u64));
            }
        }
        Ok(out)
    }

    /// PROV events of one contribution, oldest first.
    pub fn prov(&self, id: &str) -> Result<Vec<ProvEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT body FROM prov_events WHERE contribution = ?1 ORDER BY seq")?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for body in rows {
            out.push(from_json(&body?)?);
        }
        Ok(out)
    }
}

/// Indexed copies of the subject id, target id and edge (resolved ids once checked).
fn index_columns(c: &Contribution) -> (Option<String>, Option<String>, Option<String>) {
    let report = c.checks.as_ref();
    let subject = report
        .and_then(|r| r.subject.as_ref().map(|h| h.id.clone()))
        .or_else(|| c.submission.subject.id.clone());
    let target = report
        .and_then(|r| r.target.as_ref().map(|h| h.id.clone()))
        .or_else(|| c.submission.target.as_ref().and_then(|t| t.id.clone()));
    let edge = c.submission.edge.clone().or_else(|| asserted_edge(c));
    (subject, target, edge)
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| ContribError::Internal(format!("serialise: {e}")))
}

fn from_json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|e| ContribError::Internal(format!("stored row: {e}")))
}
