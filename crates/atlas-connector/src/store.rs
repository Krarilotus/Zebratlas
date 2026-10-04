//! Durable device grants; only credential hashes are kept on the hosted machine.
use crate::{hash, random_token};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

pub struct Store(Mutex<Connection>);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChoice {
    pub connection: String,
    #[serde(default)]
    pub model: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub id: String,
    pub user_id: String,
    pub label: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}
#[derive(Serialize)]
pub struct Pairing {
    pub device_code: String,
    pub user_code: String,
    pub expires_in: u64,
    pub interval: u64,
}
pub enum Poll {
    Pending,
    Ready { device_id: String },
    Invalid,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::init(Connection::open(path)?)
    }
    pub fn memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }
    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS connector_pairings (
            device_hash TEXT PRIMARY KEY, code_hash TEXT UNIQUE NOT NULL,
            label TEXT NOT NULL, expires_at INTEGER NOT NULL, user_id TEXT, consumed INTEGER NOT NULL DEFAULT 0,
            last_poll INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS connector_devices (
            id TEXT PRIMARY KEY, user_id TEXT NOT NULL, label TEXT NOT NULL,
            credential_hash TEXT UNIQUE NOT NULL, created_at INTEGER NOT NULL, revoked_at INTEGER);
            CREATE TABLE IF NOT EXISTS connector_model_choices (
            user_id TEXT PRIMARY KEY, connection TEXT NOT NULL, model TEXT);",
        )?;
        Ok(Self(Mutex::new(conn)))
    }
    pub fn pair(&self, label: &str, now: i64) -> anyhow::Result<Pairing> {
        anyhow::ensure!(!label.trim().is_empty() && label.len() <= 80, "invalid device label");
        let device_code = random_token()?;
        // 48 bits; online guesses are rate limited separately. Codes are displayed only locally.
        let user_code = random_token()?[..12].to_uppercase();
        let conn = self.0.lock().unwrap();
        conn.execute(
            "DELETE FROM connector_pairings WHERE expires_at <= ?1 OR consumed = 1",
            [now],
        )?;
        let count: i64 = conn.query_row("SELECT count(*) FROM connector_pairings", [], |r| r.get(0))?;
        anyhow::ensure!(count < 1000, "pairing capacity reached");
        conn.execute(
            "INSERT INTO connector_pairings (device_hash, code_hash, label, expires_at)
            VALUES (?1,?2,?3,?4)",
            params![
                hash(device_code.as_bytes()),
                hash(user_code.as_bytes()),
                label,
                now + 600
            ],
        )?;
        Ok(Pairing {
            device_code,
            user_code,
            expires_in: 600,
            interval: 5,
        })
    }
    pub fn approve(&self, code: &str, user: &str, now: i64) -> anyhow::Result<bool> {
        Ok(self.0.lock().unwrap().execute(
            "UPDATE connector_pairings SET user_id = ?1
            WHERE code_hash = ?2 AND user_id IS NULL AND expires_at > ?3 AND consumed = 0",
            params![user, hash(code.trim().to_uppercase().as_bytes()), now],
        )? == 1)
    }
    /// Device code doubles as the future credential; approval returns no secret to the browser.
    /// This lets the server store hashes exclusively, including while polling.
    pub fn poll(&self, code: &str, now: i64) -> anyhow::Result<Poll> {
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction()?;
        let digest = hash(code.as_bytes());
        let row: Option<(String, Option<String>, i64)> = tx
            .query_row(
                "SELECT label,user_id,last_poll FROM connector_pairings WHERE device_hash=?1
                AND expires_at > ?2 AND consumed=0",
                params![digest, now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((label, user, last)) = row else {
            return Ok(Poll::Invalid);
        };
        anyhow::ensure!(now - last >= 5, "slow_down");
        tx.execute(
            "UPDATE connector_pairings SET last_poll=?2 WHERE device_hash=?1",
            params![digest, now],
        )?;
        let Some(user) = user else {
            tx.commit()?;
            return Ok(Poll::Pending);
        };
        let id = format!("dev_{}", &random_token()?[..24]);
        tx.execute(
            "INSERT INTO connector_devices (id,user_id,label,credential_hash,created_at)
            VALUES (?1,?2,?3,?4,?5)",
            params![id, user, label, digest, now],
        )?;
        tx.execute(
            "UPDATE connector_pairings SET consumed=1 WHERE device_hash=?1",
            [digest],
        )?;
        tx.commit()?;
        Ok(Poll::Ready { device_id: id })
    }
    pub fn authenticate(&self, token: &str) -> anyhow::Result<Option<Device>> {
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Ok(None);
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,user_id,label,created_at,revoked_at
            FROM connector_devices WHERE credential_hash=?1 AND revoked_at IS NULL",
                [hash(token.as_bytes())],
                device_row,
            )
            .optional()?)
    }
    pub fn active(&self, id: &str, user: &str) -> anyhow::Result<bool> {
        Ok(self.0.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM connector_devices
            WHERE id=?1 AND user_id=?2 AND revoked_at IS NULL)",
            params![id, user],
            |r| r.get(0),
        )?)
    }
    pub fn list(&self, user: &str) -> anyhow::Result<Vec<Device>> {
        let conn = self.0.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT id,user_id,label,created_at,revoked_at FROM connector_devices
            WHERE user_id=?1 ORDER BY created_at DESC",
        )?;
        Ok(q.query_map([user], device_row)?.collect::<Result<_, _>>()?)
    }
    pub fn revoke(&self, id: &str, user: &str, now: i64) -> anyhow::Result<bool> {
        Ok(self.0.lock().unwrap().execute(
            "UPDATE connector_devices SET revoked_at=?3
            WHERE id=?1 AND user_id=?2 AND revoked_at IS NULL",
            params![id, user, now],
        )? == 1)
    }
    pub fn choice(&self, user: &str) -> anyhow::Result<Option<ModelChoice>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT connection,model FROM connector_model_choices WHERE user_id=?1",
                [user],
                |r| {
                    Ok(ModelChoice {
                        connection: r.get(0)?,
                        model: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }
    /// Called only after the gateway validates ownership and the advertised model.
    pub fn set_choice(&self, user: &str, choice: &ModelChoice) -> anyhow::Result<()> {
        anyhow::ensure!(
            !choice.connection.is_empty() && choice.connection.len() <= 256,
            "invalid connection"
        );
        anyhow::ensure!(
            choice
                .model
                .as_ref()
                .is_none_or(|m| !m.is_empty() && m.len() <= 160 && !m.chars().any(char::is_control)),
            "invalid model"
        );
        self.0.lock().unwrap().execute(
            "INSERT INTO connector_model_choices (user_id,connection,model) VALUES (?1,?2,?3)
             ON CONFLICT(user_id) DO UPDATE SET connection=excluded.connection,model=excluded.model",
            params![user, choice.connection, choice.model],
        )?;
        Ok(())
    }
    pub fn clear_choice(&self, user: &str) -> anyhow::Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM connector_model_choices WHERE user_id=?1", [user])?;
        Ok(())
    }
}
fn device_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Device> {
    Ok(Device {
        id: r.get(0)?,
        user_id: r.get(1)?,
        label: r.get(2)?,
        created_at: r.get(3)?,
        revoked_at: r.get(4)?,
    })
}
