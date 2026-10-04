//! Single-use email proofs. Raw tokens never cross the storage boundary.
use super::*;
use crate::auth::token_hash;

impl Store {
    #[cfg(test)]
    pub(crate) fn test_expire_email_cooldown(&self, email: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE email_action_limits SET window_started=window_started-61 WHERE key_hash=?1 AND window_seconds=60",
            [token_hash(email)],
        )?;
        Ok(())
    }
    /// A pending re-signup replaces the earlier submitted credential and its proofs.
    /// Existing verified/legacy users never have credentials changed by signup.
    pub(crate) fn signup_pending_with_token(
        &self,
        email: &str,
        name: Option<&str>,
        locale: Option<&str>,
        phc: &str,
        hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<Option<User>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let existing:Option<(User,bool,Option<i64>)>=tx.query_row("SELECT id,email,display_name,locale,created_at,verification_required,email_verified_at FROM users WHERE email=?1",[email],|r|Ok((user_row(r)?,r.get(5)?,r.get(6)?))).optional()?;
        let id = match existing {
            Some((_, false, _)) | Some((_, _, Some(_))) => return Ok(None),
            Some((user, true, None)) => {
                tx.execute(
                    "UPDATE users SET display_name=?2,locale=?3,updated_at=?4 WHERE id=?1",
                    params![user.id, name, locale, now],
                )?;
                user.id
            }
            None => {
                let id = new_id("usr");
                tx.execute("INSERT INTO users(id,email,display_name,locale,created_at,updated_at,verification_required) VALUES(?1,?2,?3,?4,?5,?5,1)",params![id,email,name,locale,now])?;
                id
            }
        };
        tx.execute("INSERT INTO credentials(id,user_id,kind,secret,created_at) VALUES(?1,?2,'password',?3,?4) ON CONFLICT(user_id) WHERE kind='password' DO UPDATE SET secret=excluded.secret,created_at=excluded.created_at,last_used_at=NULL",params![new_id("crd"),id,phc,now])?;
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=?1 OR expires_at<=?2",
            params![id, now],
        )?;
        tx.execute("DELETE FROM sessions WHERE user_id=?1", [&id])?;
        tx.execute("INSERT INTO email_tokens(token_hash,user_id,purpose,created_at,expires_at) VALUES(?1,?2,'verify_email',?3,?4)",params![hash,id,now,expires])?;
        let user = tx.query_row(
            "SELECT id,email,display_name,locale,created_at FROM users WHERE id=?1",
            [&id],
            user_row,
        )?;
        tx.commit()?;
        Ok(Some(user))
    }

    pub(crate) fn user_by_email(&self, email: &str) -> Result<Option<User>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id,email,display_name,locale,created_at FROM users WHERE email=?1",
                [email],
                user_row,
            )
            .optional()?)
    }

    pub(crate) fn email_pending(&self, user_id: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT verification_required=1 AND email_verified_at IS NULL FROM users WHERE id=?1",
                [user_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub(crate) fn email_status(&self, user_id: &str) -> Result<&'static str> {
        let value: Option<(bool, Option<i64>)> = self
            .conn()
            .query_row(
                "SELECT verification_required,email_verified_at FROM users WHERE id=?1",
                [user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(match value {
            Some((_, Some(_))) => "verified",
            Some((true, None)) => "pending",
            _ => "legacy",
        })
    }

    /// Persistent one/minute and five/hour counters, including nonexistent addresses.
    pub(crate) fn reserve_email_action(&self, email: &str, purpose: &str, now: i64) -> Result<()> {
        let fingerprint = token_hash(email);
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM email_action_limits WHERE window_started < ?1",
            [now - 7200],
        )?;
        for (window, max) in [(60_i64, 1_i64), (3600, 5)] {
            let previous: Option<(i64,i64)> = tx.query_row("SELECT window_started,attempts FROM email_action_limits WHERE key_hash=?1 AND purpose=?2 AND window_seconds=?3", params![fingerprint,purpose,window], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let (start, attempts) = previous.filter(|(start, _)| *start + window > now).unwrap_or((now, 0));
            if attempts >= max {
                return Err(AccountsError::RateLimited {
                    retry_after_secs: (start + window - now).max(1) as u64,
                });
            }
            tx.execute("INSERT INTO email_action_limits(key_hash,purpose,window_seconds,window_started,attempts) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(key_hash,purpose,window_seconds) DO UPDATE SET window_started=excluded.window_started,attempts=excluded.attempts", params![fingerprint,purpose,window,start,attempts+1])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn issue_email_token(
        &self,
        user_id: &str,
        purpose: &str,
        hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM email_tokens WHERE expires_at<=?1 OR (user_id=?2 AND purpose=?3)",
            params![now, user_id, purpose],
        )?;
        tx.execute(
            "INSERT INTO email_tokens(token_hash,user_id,purpose,created_at,expires_at) VALUES(?1,?2,?3,?4,?5)",
            params![hash, user_id, purpose, now, expires],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn discard_email_token(&self, hash: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM email_tokens WHERE token_hash=?1", [hash])?;
        Ok(())
    }

    pub(crate) fn verify_email_token(&self, hash: &str, now: i64) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let user: Option<String> = tx
            .query_row(
                "SELECT user_id FROM email_tokens WHERE token_hash=?1 AND purpose='verify_email' AND expires_at>?2",
                params![hash, now],
                |r| r.get(0),
            )
            .optional()?;
        let Some(user) = user else {
            return Ok(false);
        };
        tx.execute(
            "UPDATE users SET email_verified_at=?2,updated_at=?2 WHERE id=?1",
            params![user, now],
        )?;
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=?1 AND purpose='verify_email'",
            [&user],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Password, email ownership, token consumption and session revocation are one transaction.
    pub(crate) fn reset_password_token(&self, hash: &str, phc: &str, now: i64) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let user: Option<String> = tx
            .query_row(
                "SELECT user_id FROM email_tokens WHERE token_hash=?1 AND purpose='reset_password' AND expires_at>?2",
                params![hash, now],
                |r| r.get(0),
            )
            .optional()?;
        let Some(user) = user else {
            return Ok(false);
        };
        let changed = tx.execute(
            "UPDATE credentials SET secret=?2,created_at=?3,last_used_at=NULL WHERE user_id=?1 AND kind='password'",
            params![user, phc, now],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE users SET email_verified_at=COALESCE(email_verified_at,?2),updated_at=?2 WHERE id=?1",
            params![user, now],
        )?;
        tx.execute("DELETE FROM email_tokens WHERE user_id=?1", [&user])?;
        tx.execute("DELETE FROM sessions WHERE user_id=?1", [&user])?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::new_session_token;

    #[test]
    fn tokens_are_hashed_expiring_single_use_and_reset_revokes_every_session() {
        let store = Store::in_memory().unwrap();
        let user = store
            .create_pending_user("proof@example.org", None, Some("de"), "fixture-phc", 100)
            .unwrap();
        let (token, hash) = new_session_token();
        store
            .issue_email_token(&user.id, "verify_email", &hash, 100, 200)
            .unwrap();
        let stored: String = store
            .conn()
            .query_row("SELECT token_hash FROM email_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, hash);
        assert!(!stored.contains(token.expose()));
        assert!(!store.verify_email_token(&hash, 200).unwrap());
        assert!(store.email_pending(&user.id).unwrap());
        store
            .issue_email_token(&user.id, "verify_email", &hash, 201, 300)
            .unwrap();
        assert!(store.verify_email_token(&hash, 202).unwrap());
        assert!(!store.verify_email_token(&hash, 203).unwrap());
        assert_eq!(store.email_status(&user.id).unwrap(), "verified");
        for i in 0..2 {
            store
                .create_session(&user.id, &format!("fixture{i}"), "password", 202, 500)
                .unwrap();
        }
        store
            .issue_email_token(&user.id, "reset_password", &hash, 203, 250)
            .unwrap();
        assert!(store.reset_password_token(&hash, "new-fixture-phc", 204).unwrap());
        assert!(!store.reset_password_token(&hash, "attacker", 205).unwrap());
        assert_eq!(
            store.password_hash(&user.id).unwrap().as_deref(),
            Some("new-fixture-phc")
        );
        assert!(store.session("fixture0", 205).unwrap().is_none());
        assert!(store.session("fixture1", 205).unwrap().is_none());
    }

    #[test]
    fn resend_replaces_proof_and_persistent_limits_also_cover_unknown_emails() {
        let store = Store::in_memory().unwrap();
        let user = store
            .create_pending_user("proof@example.org", None, None, "phc", 100)
            .unwrap();
        let (_, old) = new_session_token();
        let (_, new) = new_session_token();
        store
            .issue_email_token(&user.id, "verify_email", &old, 100, 500)
            .unwrap();
        store
            .issue_email_token(&user.id, "verify_email", &new, 161, 500)
            .unwrap();
        assert!(!store.verify_email_token(&old, 162).unwrap());
        assert!(store.verify_email_token(&new, 162).unwrap());
        for i in 0..5 {
            store
                .reserve_email_action("unknown@example.org", "reset_password", 100 + i * 61)
                .unwrap();
        }
        assert!(matches!(
            store.reserve_email_action("unknown@example.org", "reset_password", 406),
            Err(AccountsError::RateLimited { .. })
        ));
        store
            .reserve_email_action("unknown@example.org", "reset_password", 3700)
            .unwrap();
        let raw: String = store
            .conn()
            .query_row("SELECT key_hash FROM email_action_limits LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw.len(), 64);
        assert!(!raw.contains("example.org"));
    }

    #[test]
    fn reset_rejects_expired_or_wrong_purpose_proof_and_preserves_other_users() {
        let store = Store::in_memory().unwrap();
        let user = store
            .create_user("proof@example.org", None, None, "old-phc", 100)
            .unwrap();
        let other = store
            .create_user("other@example.org", None, None, "other-phc", 100)
            .unwrap();
        store
            .create_session(&other.id, "other-session", "password", 100, 900)
            .unwrap();
        let (_, hash) = new_session_token();
        store
            .issue_email_token(&user.id, "verify_email", &hash, 100, 200)
            .unwrap();
        assert!(!store.reset_password_token(&hash, "attacker", 101).unwrap());
        let (_, hash) = new_session_token();
        store
            .issue_email_token(&user.id, "reset_password", &hash, 100, 200)
            .unwrap();
        assert!(!store.reset_password_token(&hash, "attacker", 200).unwrap());
        assert_eq!(store.password_hash(&user.id).unwrap().as_deref(), Some("old-phc"));
        store
            .issue_email_token(&user.id, "reset_password", &hash, 201, 500)
            .unwrap();
        store.set_password(&user.id, "authenticated-new-phc", 202).unwrap();
        assert!(!store.reset_password_token(&hash, "attacker", 203).unwrap());
        store
            .issue_email_token(&user.id, "reset_password", &hash, 204, 500)
            .unwrap();
        assert!(store.reset_password_token(&hash, "reset-new-phc", 205).unwrap());
        assert!(store.session("other-session", 206).unwrap().is_some());
        assert_eq!(store.email_status(&other.id).unwrap(), "legacy");
        store
            .conn()
            .execute("DELETE FROM credentials WHERE user_id=?1", [&user.id])
            .unwrap();
        store
            .issue_email_token(&user.id, "reset_password", &hash, 207, 500)
            .unwrap();
        assert!(!store.reset_password_token(&hash, "false-success", 208).unwrap());
    }

    #[test]
    fn concurrent_token_consumption_succeeds_exactly_once() {
        let store = std::sync::Arc::new(Store::in_memory().unwrap());
        let user = store
            .create_pending_user("proof@example.org", None, None, "phc", 100)
            .unwrap();
        let (_, hash) = new_session_token();
        store
            .issue_email_token(&user.id, "verify_email", &hash, 100, 300)
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let store = store.clone();
                let hash = hash.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.verify_email_token(&hash, 101).unwrap() as usize
                })
            })
            .collect();
        assert_eq!(workers.into_iter().map(|w| w.join().unwrap()).sum::<usize>(), 1);
    }
}
