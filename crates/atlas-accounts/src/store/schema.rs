//! Schema migrations, applied in order and tracked with `PRAGMA user_version`.

use rusqlite::Connection;

/// Each entry moves the schema one version up. Never edit a shipped entry; append a new one.
const MIGRATIONS: &[&str] = &[
    // v1: users, credentials, sessions, organisations, saved items, conversations.
    r#"
    CREATE TABLE users (
        id           TEXT PRIMARY KEY,
        email        TEXT NOT NULL UNIQUE COLLATE NOCASE,
        display_name TEXT,
        locale       TEXT,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL
    );

    -- One row per sign-in method. 'password' holds an argon2id PHC string; 'passkey' (WebAuthn public
    -- key + credential id) and 'magic_link' are reserved so new methods need no schema change.
    CREATE TABLE credentials (
        id           TEXT PRIMARY KEY,
        user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        kind         TEXT NOT NULL CHECK (kind IN ('password', 'passkey', 'magic_link')),
        secret       TEXT NOT NULL,
        label        TEXT,
        created_at   INTEGER NOT NULL,
        last_used_at INTEGER
    );
    CREATE UNIQUE INDEX credentials_one_password ON credentials(user_id) WHERE kind = 'password';

    -- Only the SHA-256 of a session token is stored.
    CREATE TABLE sessions (
        token_hash   TEXT PRIMARY KEY,
        id           TEXT NOT NULL UNIQUE,
        user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        auth_method  TEXT NOT NULL,
        created_at   INTEGER NOT NULL,
        expires_at   INTEGER NOT NULL,
        last_seen_at INTEGER NOT NULL
    );
    CREATE INDEX sessions_user ON sessions(user_id);
    CREATE INDEX sessions_expiry ON sessions(expires_at);

    -- Paid-tier groundwork: a plan flag, no billing.
    CREATE TABLE organisations (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        plan       TEXT NOT NULL DEFAULT 'free' CHECK (plan IN ('free', 'organisation', 'enterprise')),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE memberships (
        org_id     TEXT NOT NULL REFERENCES organisations(id) ON DELETE CASCADE,
        user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        role       TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
        created_at INTEGER NOT NULL,
        PRIMARY KEY (org_id, user_id)
    );
    CREATE INDEX memberships_user ON memberships(user_id);

    CREATE TABLE saved_items (
        id             TEXT PRIMARY KEY,
        user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        org_id         TEXT REFERENCES organisations(id) ON DELETE SET NULL,
        kind           TEXT NOT NULL CHECK (kind IN ('condition', 'connection_card', 'connection_map',
                                                     'message_draft', 'node', 'search')),
        title          TEXT NOT NULL,
        note           TEXT,
        payload        TEXT NOT NULL CHECK (json_valid(payload)),
        atlas_snapshot TEXT,
        created_at     INTEGER NOT NULL,
        updated_at     INTEGER NOT NULL
    );
    CREATE INDEX saved_items_user ON saved_items(user_id, updated_at);
    CREATE INDEX saved_items_org ON saved_items(org_id) WHERE org_id IS NOT NULL;

    -- Atlas node / edge ids per item, so "is this condition saved?" is an index lookup.
    CREATE TABLE saved_item_refs (
        item_id  TEXT NOT NULL REFERENCES saved_items(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        ref_id   TEXT NOT NULL,
        PRIMARY KEY (item_id, position)
    );
    CREATE INDEX saved_item_refs_ref ON saved_item_refs(ref_id);

    CREATE TABLE conversations (
        id         TEXT PRIMARY KEY,
        user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        org_id     TEXT REFERENCES organisations(id) ON DELETE SET NULL,
        title      TEXT NOT NULL,
        locale     TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX conversations_user ON conversations(user_id, updated_at);

    CREATE TABLE conversation_messages (
        id              TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        seq             INTEGER NOT NULL,
        role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
        content         TEXT NOT NULL,
        citations       TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(citations)),
        meta            TEXT CHECK (meta IS NULL OR json_valid(meta)),
        created_at      INTEGER NOT NULL,
        UNIQUE (conversation_id, seq)
    );
    "#,
    // 2: encrypted documents (D41). The per-user data key is stored only wrapped by the server key;
    // deleting the user deletes the wrapped key, so backup copies of their documents become unreadable.
    r#"
    CREATE TABLE user_keys (
        user_id     TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        wrapped_key BLOB NOT NULL,
        nonce       BLOB NOT NULL,
        created_at  INTEGER NOT NULL
    );
    CREATE TABLE documents (
        id         TEXT PRIMARY KEY,
        user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        format     TEXT NOT NULL,
        bytes      INTEGER NOT NULL,
        sha256     TEXT NOT NULL,
        terms      INTEGER NOT NULL,
        nonce      BLOB NOT NULL,
        ciphertext BLOB NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX documents_user ON documents(user_id, created_at);
    "#,
    // v3: new signups require email proof; existing users retain explicit legacy access.
    r#"
    ALTER TABLE users ADD COLUMN verification_required INTEGER NOT NULL DEFAULT 1 CHECK (verification_required IN (0,1));
    UPDATE users SET verification_required=0;
    ALTER TABLE users ADD COLUMN email_verified_at INTEGER;
    CREATE TABLE email_tokens (
        token_hash TEXT PRIMARY KEY CHECK (length(token_hash) = 64),
        user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        purpose TEXT NOT NULL CHECK (purpose IN ('verify_email','reset_password')),
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );
    CREATE INDEX email_tokens_user ON email_tokens(user_id,purpose);
    CREATE INDEX email_tokens_expiry ON email_tokens(expires_at);
    -- Email fingerprints only: no tokens, recipients or IP addresses in this counter.
    CREATE TABLE email_action_limits (
        key_hash TEXT NOT NULL CHECK (length(key_hash) = 64),
        purpose TEXT NOT NULL,
        window_seconds INTEGER NOT NULL,
        window_started INTEGER NOT NULL,
        attempts INTEGER NOT NULL,
        PRIMARY KEY (key_hash,purpose,window_seconds)
    );
    "#,
];

#[cfg(test)]
pub const VERSION: i64 = MIGRATIONS.len() as i64;

pub fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    // deleted rows (documents, keys) are overwritten on disk, not just unlinked
    conn.pragma_update(None, "secure_delete", true)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod email_migration_tests {
    use super::*;
    #[test]
    fn only_preexisting_users_are_legacy_and_old_writer_sessions_cannot_bypass_new_backend() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        for migration in &MIGRATIONS[..2] {
            conn.execute_batch(migration).unwrap();
        }
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute(
            "INSERT INTO users(id,email,created_at,updated_at) VALUES('legacy','legacy@example.org',1,1)",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let legacy: (i64, Option<i64>) = conn
            .query_row(
                "SELECT verification_required,email_verified_at FROM users WHERE id='legacy'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(legacy, (0, None));
        conn.execute(
            "INSERT INTO users(id,email,created_at,updated_at) VALUES('old-writer','new@example.org',2,2)",
            [],
        )
        .unwrap();
        let required: i64 = conn
            .query_row(
                "SELECT verification_required FROM users WHERE id='old-writer'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(required, 1);
        conn.execute("INSERT INTO sessions(token_hash,id,user_id,auth_method,created_at,expires_at,last_seen_at) VALUES('fixture-hash','session','old-writer','password',2,100,2)",[]).unwrap();
        let store = super::super::Store {
            conn: std::sync::Mutex::new(conn),
        };
        assert!(store.session("fixture-hash", 3).unwrap().is_none());
        assert_eq!(store.email_status("legacy").unwrap(), "legacy");
    }
}
