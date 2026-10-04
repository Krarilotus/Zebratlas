//! Saved documents, encrypted at rest (D41), and the hooks account deletion needs (D42).
//!
//! - XChaCha20-Poly1305 with a random 24-byte nonce per record.
//! - Each user has a random 32-byte data key; only its wrapped form (sealed with the server key
//!   `ATLAS_DOC_KEK`) is stored. Deleting the user deletes the wrapped key, so backup copies of their
//!   documents become unreadable (crypto-shredding).
//! - The associated data binds every ciphertext to its user (and document id): rows cannot be swapped.
//! - Without `ATLAS_DOC_KEK` nothing is saved (503); listing and deleting still work.

use std::sync::Arc;

use axum::http::HeaderMap;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::auth::token_hash;
use crate::config::AccountsConfig;
use crate::error::{AccountsError, Result};
use crate::store::{NewDocumentRow, Store};
use crate::util::{now, random_bytes, rfc3339};

/// Largest document text that is saved (characters of extracted text).
pub const MAX_TEXT_CHARS: usize = 400_000;

/// The server's key-encryption key.
#[derive(Clone)]
pub struct Kek(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for Kek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kek(redacted)")
    }
}

impl Kek {
    /// 32 bytes as base64 (standard or URL-safe) or 64 hex characters.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let bytes = Zeroizing::new(if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            (0..32)
                .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()?
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(s)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s))
                .ok()?
        });
        let arr: [u8; 32] = bytes.as_slice().try_into().ok()?;
        Some(Self(Zeroizing::new(arr)))
    }

    /// `ATLAS_DOC_KEK`, if set and valid (an invalid value is reported once and treated as unset).
    pub fn from_env() -> Option<Self> {
        let v = std::env::var("ATLAS_DOC_KEK").ok().filter(|v| !v.trim().is_empty())?;
        let k = Self::parse(&v);
        if k.is_none() {
            eprintln!("atlas-accounts: ATLAS_DOC_KEK is not 32 bytes (base64 or hex); saving documents is off");
        }
        k
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(Key::from_slice(&self.0[..]))
    }
}

fn seal(key: &XChaCha20Poly1305, plain: &[u8], aad: &[u8]) -> Result<([u8; 24], Vec<u8>)> {
    let nonce: [u8; 24] = random_bytes();
    let ct = key
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .map_err(|_| AccountsError::Internal("encryption failed".into()))?;
    Ok((nonce, ct))
}

fn open(key: &XChaCha20Poly1305, nonce: &[u8], ct: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if nonce.len() != 24 {
        return Err(AccountsError::Internal("bad nonce".into()));
    }
    key.decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| AccountsError::Internal("document cannot be decrypted with this server key".into()))
}

/// The user's data key: unwrapped from the store, or created and wrapped on first save.
fn data_key(store: &Store, kek: &Kek, user_id: &str, create: bool) -> Result<Option<XChaCha20Poly1305>> {
    let aad = format!("atlas-dek:{user_id}");
    let wrapped = match store.wrapped_key(user_id)? {
        Some(w) => w,
        None if create => {
            let dek = Zeroizing::new(random_bytes::<32>());
            let (nonce, wrapped) = seal(&kek.cipher(), &dek[..], aad.as_bytes())?;
            store.put_wrapped_key(user_id, &wrapped, &nonce, now())?
        }
        None => return Ok(None),
    };
    let dek = open(&kek.cipher(), &wrapped.1, &wrapped.0, aad.as_bytes())?;
    Ok(Some(XChaCha20Poly1305::new(Key::from_slice(&dek))))
}

/// What is sealed per document.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocumentContent {
    pub title: String,
    pub text: String,
    /// The extracted terms as the intake returned them (the user may have edited them).
    pub terms: Value,
}

/// Clear metadata of a document to save.
#[derive(Clone, Debug)]
pub struct DocumentMeta {
    pub format: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedDocument {
    pub id: String,
    pub created_at: String,
}

/// Encrypt and store a document for the user.
pub fn save(
    store: &Store,
    kek: &Kek,
    user_id: &str,
    meta: &DocumentMeta,
    content: &DocumentContent,
) -> Result<SavedDocument> {
    if content.text.chars().count() > MAX_TEXT_CHARS {
        return Err(AccountsError::invalid("document too long to save"));
    }
    let key = data_key(store, kek, user_id, true)?.ok_or_else(|| AccountsError::Internal("no data key".into()))?;
    let id = Store::new_document_id();
    let plain = Zeroizing::new(serde_json::to_vec(content).map_err(|e| AccountsError::Internal(e.to_string()))?);
    let (nonce, ct) = seal(&key, &plain, format!("atlas-doc:{user_id}:{id}").as_bytes())?;
    let terms = content.terms.as_array().map_or(0, Vec::len) as i64;
    let t = now();
    store.insert_document(
        user_id,
        NewDocumentRow {
            id: &id,
            format: &meta.format,
            bytes: meta.bytes as i64,
            sha256: &meta.sha256,
            terms,
            nonce: &nonce,
            ciphertext: &ct,
        },
        t,
    )?;
    Ok(SavedDocument {
        id,
        created_at: rfc3339(t),
    })
}

/// List entries (title decrypted when the key is available, else `null`).
pub fn list(store: &Store, kek: Option<&Kek>, user_id: &str) -> Result<Vec<Value>> {
    let rows = store.documents(user_id)?;
    let key = match kek {
        Some(k) if !rows.is_empty() => data_key(store, k, user_id, false)?,
        _ => None,
    };
    Ok(rows
        .iter()
        .map(|r| {
            let title = key.as_ref().and_then(|k| {
                open(
                    k,
                    &r.nonce,
                    &r.ciphertext,
                    format!("atlas-doc:{user_id}:{}", r.id).as_bytes(),
                )
                .ok()
                .and_then(|p| serde_json::from_slice::<DocumentContent>(&p).ok())
                .map(|c| c.title)
            });
            json!({ "id": r.id, "title": title, "format": r.format, "bytes": r.bytes, "sha256": r.sha256,
                    "terms": r.terms, "created_at": r.created_at })
        })
        .collect())
}

/// One document, decrypted for its owner.
pub fn get(store: &Store, kek: Option<&Kek>, user_id: &str, id: &str) -> Result<Value> {
    let r = store.document(user_id, id)?.ok_or(AccountsError::NotFound)?;
    let kek = kek.ok_or(AccountsError::Unavailable)?;
    let key = data_key(store, kek, user_id, false)?.ok_or(AccountsError::NotFound)?;
    let plain = open(
        &key,
        &r.nonce,
        &r.ciphertext,
        format!("atlas-doc:{user_id}:{}", r.id).as_bytes(),
    )?;
    let c: DocumentContent = serde_json::from_slice(&plain).map_err(|e| AccountsError::Internal(e.to_string()))?;
    Ok(
        json!({ "id": r.id, "title": c.title, "format": r.format, "bytes": r.bytes, "sha256": r.sha256,
               "created_at": r.created_at, "text": c.text, "terms": c.terms }),
    )
}

/// Every document of the user, decrypted (the data export).
pub fn export(store: &Store, kek: Option<&Kek>, user_id: &str) -> Result<Vec<Value>> {
    store
        .documents(user_id)?
        .iter()
        .map(|r| match kek {
            Some(_) => get(store, kek, user_id, &r.id),
            None => Ok(
                json!({ "id": r.id, "format": r.format, "bytes": r.bytes, "sha256": r.sha256,
                               "created_at": r.created_at, "note": "encrypted; the server key is not configured" }),
            ),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Contributions (atlas-contrib), through the host
// ---------------------------------------------------------------------------------------------

/// What anonymising a user's contributions did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymiseReport {
    /// Accepted contributions that stay in the graph (anonymous, or with the name if `keep_credit`).
    pub accepted_kept: u64,
    /// Contributions whose personal fields (name, e-mail, account link) were removed or deleted.
    pub personal_fields_removed: u64,
}

/// atlas-contrib's account hooks (D42), wired by the server. atlas-accounts does not depend on
/// atlas-contrib (it sits lower in the crate graph).
pub trait ContributionHooks: Send + Sync {
    /// All of the user's contributions as JSON.
    fn export_for_user(&self, user_id: &str) -> std::result::Result<Value, String>;
    /// Remove name, e-mail and account link from the user's contributions; accepted ones stay in the
    /// graph, anonymous unless `keep_credit`.
    fn anonymise_for_user(&self, user_id: &str, keep_credit: bool) -> std::result::Result<AnonymiseReport, String>;
}

pub type Hooks = Option<Arc<dyn ContributionHooks>>;

/// What the host adds to the accounts router: contribution hooks and the document key.
#[derive(Clone, Default)]
pub struct AccountsExtras {
    pub hooks: Hooks,
    pub kek: Option<Kek>,
}

impl AccountsExtras {
    /// `kek` from `ATLAS_DOC_KEK`.
    pub fn from_env(hooks: Hooks) -> Self {
        Self {
            hooks,
            kek: Kek::from_env(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// For the host: save from another router (POST /api/intake with save=true)
// ---------------------------------------------------------------------------------------------

/// A handle the server's intake route uses to find the signed-in user and save a document.
pub struct DocumentVault {
    store: Store,
    kek: Option<Kek>,
    secure_cookies: bool,
}

impl DocumentVault {
    /// Opens its own connection to the accounts database (WAL: safe next to the accounts router's).
    pub fn open(config: &AccountsConfig) -> Result<Self> {
        Self::open_with(config, Kek::from_env())
    }

    pub fn open_with(config: &AccountsConfig, kek: Option<Kek>) -> Result<Self> {
        Ok(Self {
            store: Store::open(&config.database)?,
            kek,
            secure_cookies: config.secure_cookies,
        })
    }

    pub fn with_store(store: Store, kek: Option<Kek>) -> Self {
        Self {
            store,
            kek,
            secure_cookies: true,
        }
    }

    pub fn can_save(&self) -> bool {
        self.kek.is_some()
    }

    /// The signed-in user (id, e-mail, display name) from the session cookie or bearer token.
    pub fn user(&self, headers: &HeaderMap) -> Result<Option<crate::model::User>> {
        let Some(token) = crate::routes::presented_token(headers, self.secure_cookies) else {
            return Ok(None);
        };
        Ok(self.store.session(&token_hash(&token), now())?.map(|s| s.user))
    }

    pub fn save(&self, user_id: &str, meta: &DocumentMeta, content: &DocumentContent) -> Result<SavedDocument> {
        let kek = self.kek.as_ref().ok_or(AccountsError::Unavailable)?;
        save(&self.store, kek, user_id, meta, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_user() -> (Store, String) {
        let store = Store::in_memory().unwrap();
        let u = store
            .create_user(
                "a@example.org",
                None,
                None,
                "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$aGFzaA",
                0,
            )
            .unwrap();
        (store, u.id)
    }

    fn kek() -> Kek {
        Kek::parse(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn vault_uses_configured_cookie_mode_and_accepts_bearer_sessions() {
        for secure in [true, false] {
            let mut config = AccountsConfig::for_tests();
            config.secure_cookies = secure;
            let vault = DocumentVault::open_with(&config, None).unwrap();
            let t = now();
            let user = vault
                .store
                .create_user("synthetic@example.invalid", None, None, "unused", t)
                .unwrap();
            let token = "a".repeat(43);
            vault
                .store
                .create_session(&user.id, &token_hash(&token), "synthetic", t, t + 60)
                .unwrap();
            let mut headers = HeaderMap::new();
            let expected = if secure {
                crate::SESSION_COOKIE
            } else {
                crate::INSECURE_SESSION_COOKIE
            };
            let wrong = if secure {
                crate::INSECURE_SESSION_COOKIE
            } else {
                crate::SESSION_COOKIE
            };
            headers.insert(axum::http::header::COOKIE, format!("{wrong}={token}").parse().unwrap());
            assert!(vault.user(&headers).unwrap().is_none());
            headers.insert(
                axum::http::header::COOKIE,
                format!("{expected}={token}").parse().unwrap(),
            );
            assert_eq!(vault.user(&headers).unwrap().unwrap().id, user.id);
            headers.remove(axum::http::header::COOKIE);
            headers.insert(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            assert_eq!(vault.user(&headers).unwrap().unwrap().id, user.id);
        }
    }

    #[test]
    fn kek_parsing() {
        assert!(Kek::parse(&"00".repeat(32)).is_some());
        assert!(Kek::parse(&base64::engine::general_purpose::STANDARD.encode([7u8; 32])).is_some());
        assert!(Kek::parse("short").is_none());
        assert!(Kek::parse(&base64::engine::general_purpose::STANDARD.encode([7u8; 16])).is_none());
    }

    #[test]
    fn save_list_get_delete_roundtrip_and_ciphertext_is_opaque() {
        let (store, uid) = store_with_user();
        let k = kek();
        let meta = DocumentMeta {
            format: "txt".into(),
            bytes: 30,
            sha256: "0".repeat(64),
        };
        let content = DocumentContent {
            title: "Arztbrief Neuropädiatrie".into(),
            text: "Synthetic letter: STXBP1 c.1162C>T".into(),
            terms: json!([{ "kind": "gene", "text": "STXBP1" }]),
        };
        let saved = save(&store, &k, &uid, &meta, &content).unwrap();
        let rows = store.documents(&uid).unwrap();
        assert_eq!(rows.len(), 1);
        let raw = String::from_utf8_lossy(&rows[0].ciphertext);
        assert!(!raw.contains("STXBP1") && !raw.contains("Arztbrief"));
        let listed = list(&store, Some(&k), &uid).unwrap();
        assert_eq!(listed[0]["title"], "Arztbrief Neuropädiatrie");
        assert_eq!(listed[0]["terms"], 1);
        let doc = get(&store, Some(&k), &uid, &saved.id).unwrap();
        assert_eq!(doc["text"], content.text);
        // a different server key cannot read it
        let other = Kek::parse(&"cd".repeat(32)).unwrap();
        assert!(get(&store, Some(&other), &uid, &saved.id).is_err());
        assert!(store.delete_document(&uid, &saved.id).unwrap());
        assert!(store.documents(&uid).unwrap().is_empty());
    }

    #[test]
    fn deleting_the_user_removes_documents_and_the_wrapped_key() {
        let (store, uid) = store_with_user();
        let meta = DocumentMeta {
            format: "paste".into(),
            bytes: 5,
            sha256: "1".repeat(64),
        };
        save(&store, &kek(), &uid, &meta, &DocumentContent::default()).unwrap();
        let f = store.footprint(&uid).unwrap();
        assert_eq!((f.documents, f.data_key), (1, true));
        assert!(store.delete_user(&uid).unwrap());
        assert!(store.wrapped_key(&uid).unwrap().is_none());
        assert!(store.documents(&uid).unwrap().is_empty());
    }
}
