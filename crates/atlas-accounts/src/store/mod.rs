//! SQLite storage: one connection behind a mutex (writes are tiny; SQLite serialises them anyway).
//!
//! Identity, sessions and organisations live here; saved items, conversations and the data export in
//! [`content`].

mod content;
mod documents;
mod email_tokens;
mod schema;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, Row, params};

pub use content::{SavedChanges, SavedFields, SavedFilter};
pub use documents::{DocumentRow, Footprint, NewDocumentRow};

use crate::config::Database;
use crate::error::{AccountsError, Result};
use crate::model::{Membership, Organisation, Plan, Role, User};
use crate::util::{new_id, rfc3339};

pub struct Store {
    conn: Mutex<Connection>,
}

/// A live session with its user.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub id: String,
    pub user: User,
    pub expires_at: i64,
}

impl Store {
    pub fn open(db: &Database) -> Result<Self> {
        let mut conn = match db {
            Database::InMemory => Connection::open_in_memory()?,
            Database::File(path) => {
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| AccountsError::Internal(format!("creating {}: {e}", dir.display())))?;
                }
                let conn = Connection::open(path)?;
                // WAL: readers don't block the writer. Not available for in-memory databases.
                conn.pragma_update(None, "journal_mode", "WAL")?;
                conn
            }
        };
        schema::configure(&conn)?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_file(path: &Path) -> Result<Self> {
        Self::open(&Database::File(path.to_path_buf()))
    }

    pub fn in_memory() -> Result<Self> {
        Self::open(&Database::InMemory)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn().pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (transactions roll back on drop).
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // -----------------------------------------------------------------------------------------
    // Users and credentials
    // -----------------------------------------------------------------------------------------

    /// Creates a user with a password credential. `Conflict` if the email is taken.
    pub fn create_user(
        &self,
        email: &str,
        display_name: Option<&str>,
        locale: Option<&str>,
        password_phc: &str,
        now: i64,
    ) -> Result<User> {
        self.create_user_with_verification(email, display_name, locale, password_phc, now, false)
    }

    #[cfg(test)]
    pub(crate) fn create_pending_user(
        &self,
        email: &str,
        display_name: Option<&str>,
        locale: Option<&str>,
        password_phc: &str,
        now: i64,
    ) -> Result<User> {
        self.create_user_with_verification(email, display_name, locale, password_phc, now, true)
    }

    fn create_user_with_verification(
        &self,
        email: &str,
        display_name: Option<&str>,
        locale: Option<&str>,
        password_phc: &str,
        now: i64,
        required: bool,
    ) -> Result<User> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = new_id("usr");
        let inserted = tx.execute(
            "INSERT INTO users (id, email, display_name, locale, created_at, updated_at, verification_required)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6) ON CONFLICT(email) DO NOTHING",
            params![id, email, display_name, locale, now, required],
        )?;
        if inserted == 0 {
            return Err(AccountsError::Conflict(
                "an account with this email already exists".into(),
            ));
        }
        tx.execute(
            "INSERT INTO credentials (id, user_id, kind, secret, created_at) VALUES (?1, ?2, 'password', ?3, ?4)",
            params![new_id("crd"), id, password_phc, now],
        )?;
        tx.commit()?;
        Ok(User {
            id,
            email: email.to_string(),
            display_name: display_name.map(str::to_string),
            locale: locale.map(str::to_string),
            created_at: rfc3339(now),
        })
    }

    pub fn user(&self, user_id: &str) -> Result<Option<User>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, email, display_name, locale, created_at FROM users WHERE id = ?1",
                [user_id],
                user_row,
            )
            .optional()?)
    }

    /// The user and their password hash, for sign-in.
    pub fn password_login(&self, email: &str) -> Result<Option<(User, String)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT u.id, u.email, u.display_name, u.locale, u.created_at, c.secret
                 FROM users u JOIN credentials c ON c.user_id = u.id AND c.kind = 'password'
                 WHERE u.email = ?1",
                [email],
                |r| Ok((user_row(r)?, r.get::<_, String>(5)?)),
            )
            .optional()?)
    }

    pub fn password_hash(&self, user_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT secret FROM credentials WHERE user_id = ?1 AND kind = 'password'",
                [user_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn mark_password_used(&self, user_id: &str, now: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE credentials SET last_used_at = ?2 WHERE user_id = ?1 AND kind = 'password'",
            params![user_id, now],
        )?;
        Ok(())
    }

    pub fn set_password(&self, user_id: &str, password_phc: &str, now: i64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO credentials (id, user_id, kind, secret, created_at) VALUES (?1, ?2, 'password', ?3, ?4)
             ON CONFLICT(user_id) WHERE kind = 'password' DO UPDATE SET secret = excluded.secret, created_at = excluded.created_at",
            params![new_id("crd"), user_id, password_phc, now],
        )?;
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=?1 AND purpose='reset_password'",
            [user_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_profile(
        &self,
        user_id: &str,
        display_name: Option<Option<&str>>,
        locale: Option<Option<&str>>,
        now: i64,
    ) -> Result<User> {
        {
            let conn = self.conn();
            if let Some(name) = display_name {
                conn.execute(
                    "UPDATE users SET display_name = ?2, updated_at = ?3 WHERE id = ?1",
                    params![user_id, name, now],
                )?;
            }
            if let Some(locale) = locale {
                conn.execute(
                    "UPDATE users SET locale = ?2, updated_at = ?3 WHERE id = ?1",
                    params![user_id, locale, now],
                )?;
            }
        }
        self.user(user_id)?.ok_or(AccountsError::NotFound)
    }

    /// Deletes the user and everything they created (cascades). Organisations they were the only
    /// member of are deleted; where they were the last owner, the longest-standing member becomes owner.
    pub fn delete_user(&self, user_id: &str) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let orgs: Vec<(String, String)> = {
            let mut st = tx.prepare("SELECT org_id, role FROM memberships WHERE user_id = ?1")?;
            st.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (org_id, role) in orgs {
            let others: i64 = tx.query_row(
                "SELECT count(*) FROM memberships WHERE org_id = ?1 AND user_id <> ?2",
                params![org_id, user_id],
                |r| r.get(0),
            )?;
            if others == 0 {
                tx.execute("DELETE FROM organisations WHERE id = ?1", [&org_id])?;
                continue;
            }
            let other_owners: i64 = tx.query_row(
                "SELECT count(*) FROM memberships WHERE org_id = ?1 AND user_id <> ?2 AND role = 'owner'",
                params![org_id, user_id],
                |r| r.get(0),
            )?;
            if role == "owner" && other_owners == 0 {
                tx.execute(
                    "UPDATE memberships SET role = 'owner' WHERE org_id = ?1 AND user_id = (
                         SELECT user_id FROM memberships WHERE org_id = ?1 AND user_id <> ?2
                         ORDER BY CASE role WHEN 'admin' THEN 0 ELSE 1 END, created_at, user_id LIMIT 1)",
                    params![org_id, user_id],
                )?;
            }
        }
        let n = tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
        tx.commit()?;
        Ok(n > 0)
    }

    // -----------------------------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------------------------

    pub fn create_session(
        &self,
        user_id: &str,
        token_hash: &str,
        auth_method: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<String> {
        let id = new_id("ses");
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (token_hash, id, user_id, auth_method, created_at, expires_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5)",
            params![token_hash, id, user_id, auth_method, now, expires_at],
        )?;
        // Opportunistic clean-up keeps the table small without a background task.
        conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
        Ok(id)
    }

    /// A reset/password change during async verification cannot mint a stale-password session.
    pub(crate) fn create_password_session(
        &self,
        user_id: &str,
        expected_phc: &str,
        hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let created=tx.execute("INSERT INTO sessions(token_hash,id,user_id,auth_method,created_at,expires_at,last_seen_at) SELECT ?1,?2,u.id,'password',?4,?5,?4 FROM users u JOIN credentials c ON c.user_id=u.id AND c.kind='password' WHERE u.id=?3 AND c.secret=?6 AND (u.verification_required=0 OR u.email_verified_at IS NOT NULL)",params![hash,new_id("ses"),user_id,now,expires,expected_phc])?;
        if created == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE credentials SET last_used_at=?2 WHERE user_id=?1 AND kind='password'",
            params![user_id, now],
        )?;
        tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now])?;
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn rotate_password_session(
        &self,
        user_id: &str,
        session_id: &str,
        expected_phc: &str,
        new_phc: &str,
        hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<Option<usize>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let changed=tx.execute("UPDATE credentials SET secret=?2,created_at=?3,last_used_at=NULL WHERE user_id=?1 AND kind='password' AND secret=?4 AND EXISTS(SELECT 1 FROM sessions WHERE user_id=?1 AND id=?5 AND expires_at>?3)",params![user_id,new_phc,now,expected_phc,session_id])?;
        if changed != 1 {
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=?1 AND purpose='reset_password'",
            [user_id],
        )?;
        let ended = tx.execute("DELETE FROM sessions WHERE user_id=?1", [user_id])?;
        tx.execute("INSERT INTO sessions(token_hash,id,user_id,auth_method,created_at,expires_at,last_seen_at) VALUES(?1,?2,?3,'password',?4,?5,?4)",params![hash,new_id("ses"),user_id,now,expires])?;
        tx.commit()?;
        Ok(Some(ended))
    }

    /// The session for a token hash, if it exists and hasn't expired.
    pub fn session(&self, token_hash: &str, now: i64) -> Result<Option<SessionInfo>> {
        let conn = self.conn();
        let found = conn
            .query_row(
                "SELECT s.id, s.expires_at, s.last_seen_at, u.id, u.email, u.display_name, u.locale, u.created_at
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2
                 AND (u.verification_required = 0 OR u.email_verified_at IS NOT NULL)",
                params![token_hash, now],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        User {
                            id: r.get(3)?,
                            email: r.get(4)?,
                            display_name: r.get(5)?,
                            locale: r.get(6)?,
                            created_at: rfc3339(r.get(7)?),
                        },
                    ))
                },
            )
            .optional()?;
        let Some((id, expires_at, last_seen, user)) = found else {
            return Ok(None);
        };
        if now - last_seen > 300 {
            conn.execute("UPDATE sessions SET last_seen_at = ?2 WHERE id = ?1", params![id, now])?;
        }
        Ok(Some(SessionInfo { id, user, expires_at }))
    }

    pub fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    /// Ends every session of the user except `keep` (a session id), e.g. after a password change.
    pub fn delete_sessions_of(&self, user_id: &str, keep: Option<&str>) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND (?2 IS NULL OR id <> ?2)",
            params![user_id, keep],
        )?)
    }

    pub fn session_count(&self, user_id: &str) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM sessions WHERE user_id = ?1", [user_id], |r| {
                r.get(0)
            })?)
    }

    // -----------------------------------------------------------------------------------------
    // Organisations
    // -----------------------------------------------------------------------------------------

    /// Creates an organisation on the free plan with `user_id` as owner.
    pub fn create_org(&self, user_id: &str, name: &str, now: i64) -> Result<Membership> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let id = new_id("org");
        tx.execute(
            "INSERT INTO organisations (id, name, plan, created_at, updated_at) VALUES (?1, ?2, 'free', ?3, ?3)",
            params![id, name, now],
        )?;
        tx.execute(
            "INSERT INTO memberships (org_id, user_id, role, created_at) VALUES (?1, ?2, 'owner', ?3)",
            params![id, user_id, now],
        )?;
        tx.commit()?;
        Ok(Membership {
            organisation: Organisation {
                id,
                name: name.to_string(),
                plan: Plan::Free,
                created_at: rfc3339(now),
            },
            role: Role::Owner,
        })
    }

    pub fn memberships(&self, user_id: &str) -> Result<Vec<Membership>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT o.id, o.name, o.plan, o.created_at, m.role
             FROM memberships m JOIN organisations o ON o.id = m.org_id
             WHERE m.user_id = ?1 ORDER BY o.created_at, o.id",
        )?;
        let rows = st.query_map([user_id], |r| {
            Ok(Membership {
                organisation: Organisation {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    plan: Plan::parse(&r.get::<_, String>(2)?).unwrap_or(Plan::Free),
                    created_at: rfc3339(r.get(3)?),
                },
                role: Role::parse(&r.get::<_, String>(4)?).unwrap_or(Role::Member),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn role_in(&self, user_id: &str, org_id: &str) -> Result<Option<Role>> {
        let role: Option<String> = self
            .conn()
            .query_row(
                "SELECT role FROM memberships WHERE user_id = ?1 AND org_id = ?2",
                params![user_id, org_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(role.as_deref().and_then(Role::parse))
    }

    /// Adds a member (operator / future invitation flow).
    pub fn add_member(&self, org_id: &str, user_id: &str, role: Role, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO memberships (org_id, user_id, role, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(org_id, user_id) DO UPDATE SET role = excluded.role",
            params![org_id, user_id, role.as_str(), now],
        )?;
        Ok(())
    }

    /// Sets an organisation's plan. Operator-only (no route): billing is out of scope.
    pub fn set_plan(&self, org_id: &str, plan: Plan, now: i64) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE organisations SET plan = ?2, updated_at = ?3 WHERE id = ?1",
            params![org_id, plan.as_str(), now],
        )? > 0)
    }

    /// The best plan among the user's organisations; `free` without any.
    pub fn effective_plan(&self, user_id: &str) -> Result<Plan> {
        Ok(self
            .memberships(user_id)?
            .into_iter()
            .map(|m| m.organisation.plan)
            .max()
            .unwrap_or(Plan::Free))
    }
}

fn user_row(r: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        email: r.get(1)?,
        display_name: r.get(2)?,
        locale: r.get(3)?,
        created_at: rfc3339(r.get(4)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_to_the_latest_version() {
        let s = Store::in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), schema::VERSION);
    }

    #[test]
    fn emails_are_unique_case_insensitively() {
        let s = Store::in_memory().unwrap();
        s.create_user("a@b.de", None, None, "phc", 1).unwrap();
        let err = s.create_user("A@B.DE", None, None, "phc", 2).unwrap_err();
        assert!(matches!(err, AccountsError::Conflict(_)));
    }

    #[test]
    fn sessions_expire_and_can_be_revoked() {
        let s = Store::in_memory().unwrap();
        let u = s.create_user("a@b.de", None, None, "phc", 1).unwrap();
        s.create_session(&u.id, "h1", "password", 10, 100).unwrap();
        let keep = s.create_session(&u.id, "h2", "password", 10, 100).unwrap();
        assert_eq!(s.session("h1", 50).unwrap().unwrap().user.id, u.id);
        assert!(s.session("h1", 100).unwrap().is_none(), "expired at expires_at");
        assert_eq!(s.delete_sessions_of(&u.id, Some(&keep)).unwrap(), 1);
        assert!(s.session("h1", 50).unwrap().is_none());
        assert!(s.session("h2", 50).unwrap().is_some());
    }

    #[test]
    fn deleting_a_user_hands_shared_orgs_over_and_drops_solo_orgs() {
        let s = Store::in_memory().unwrap();
        let a = s.create_user("a@b.de", None, None, "phc", 1).unwrap();
        let b = s.create_user("b@b.de", None, None, "phc", 1).unwrap();
        let shared = s.create_org(&a.id, "Shared", 2).unwrap().organisation.id;
        let solo = s.create_org(&a.id, "Solo", 2).unwrap().organisation.id;
        s.add_member(&shared, &b.id, Role::Member, 3).unwrap();
        s.set_plan(&shared, Plan::Organisation, 4).unwrap();
        assert_eq!(s.effective_plan(&b.id).unwrap(), Plan::Organisation);

        assert!(s.delete_user(&a.id).unwrap());
        assert!(s.user(&a.id).unwrap().is_none());
        assert_eq!(s.role_in(&b.id, &shared).unwrap(), Some(Role::Owner));
        let left: i64 = s
            .conn()
            .query_row("SELECT count(*) FROM organisations WHERE id = ?1", [&solo], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }
}
