//! Passwords (argon2id), session tokens and sign-in rate limits.
//!
//! - Passwords are hashed with argon2id into a PHC string (salt and parameters included), so the cost
//!   can be raised later without invalidating existing hashes.
//! - A session token is 256 random bits, given to the client once (cookie or bearer); the database only
//!   keeps its SHA-256, so a leaked database can't be replayed as sessions.
//! - Unknown emails still run one argon2 verification against a dummy hash, so response time doesn't
//!   reveal which addresses have accounts.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::config::{Limit, PasswordCost};
use crate::error::{AccountsError, Result};
use crate::util::random_bytes;

pub const MIN_PASSWORD_CHARS: usize = 10;
/// Upper bound so a huge body can't be used to burn CPU in the hash.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// Admission to CPU-heavy password work. Production configurations share one process-wide gate.
/// Clones share capacity; independent test configurations each retain the same four-job cap.
#[derive(Clone, Debug)]
pub struct PasswordWork(Arc<tokio::sync::Semaphore>);

impl Default for PasswordWork {
    fn default() -> Self {
        static GATE: OnceLock<PasswordWork> = OnceLock::new();
        GATE.get_or_init(Self::isolated).clone()
    }
}

impl PasswordWork {
    pub(crate) fn isolated() -> Self {
        Self(Arc::new(tokio::sync::Semaphore::new(4)))
    }

    /// Reserve before spawning, and retain the permit in the worker even if its caller is cancelled.
    pub(crate) async fn run<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
        // Keep password hashing under 80 MiB at the default cost, even under a distributed flood.
        let permit = self
            .0
            .clone()
            .try_acquire_owned()
            .map_err(|_| AccountsError::RateLimited { retry_after_secs: 1 })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f()
        })
        .await
        .map_err(|e| AccountsError::Internal(format!("blocking task: {e}")))
    }
}

/// A password or token. `Debug` and `Display` are redacted so it can't end up in a log line.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Best effort: the request body buffer it was parsed from is outside our control.
        self.0.zeroize();
    }
}

/// Length-only policy (NIST SP 800-63B): at least 10 characters, no composition rules.
pub fn check_new_password(password: &Secret) -> Result<()> {
    let p = password.expose();
    if p.chars().count() < MIN_PASSWORD_CHARS {
        return Err(AccountsError::invalid(format!(
            "password needs at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    if p.len() > MAX_PASSWORD_BYTES {
        return Err(AccountsError::invalid("password is too long"));
    }
    Ok(())
}

#[derive(Clone)]
pub struct Hasher {
    params: Params,
}

impl Hasher {
    pub fn new(cost: PasswordCost) -> Result<Self> {
        let params = Params::new(cost.memory_kib, cost.iterations, cost.parallelism, None)
            .map_err(|e| AccountsError::Internal(format!("argon2 parameters: {e}")))?;
        Ok(Self { params })
    }

    fn argon2(&self) -> Argon2<'static> {
        Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone())
    }

    /// PHC string `$argon2id$v=19$m=...,t=...,p=...$salt$hash`.
    pub fn hash(&self, password: &Secret) -> Result<String> {
        self.argon2()
            .hash_password(password.expose().as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| AccountsError::Internal(format!("argon2 hash: {e}")))
    }

    /// Constant-time comparison inside argon2; parameters come from the stored hash.
    pub fn verify(&self, password: &Secret, phc: &str) -> bool {
        if password.expose().len() > MAX_PASSWORD_BYTES {
            return false;
        }
        self.argon2().verify_password(password.expose().as_bytes(), phc).is_ok()
    }

    /// Spends the same work as a real verification, for unknown accounts.
    pub fn verify_dummy(&self, password: &Secret) {
        static DUMMY: OnceLock<String> = OnceLock::new();
        let phc = DUMMY.get_or_init(|| {
            self.hash(&Secret::new("dummy password for timing equalisation"))
                .unwrap_or_default()
        });
        let _ = self.verify(password, phc);
    }
}

/// A fresh session token (base64url, 43 chars) and the hash stored for it.
pub fn new_session_token() -> (Secret, String) {
    let raw: [u8; 32] = random_bytes();
    let token = URL_SAFE_NO_PAD.encode(raw);
    let hash = token_hash(&token);
    (Secret::new(token), hash)
}

/// Hex SHA-256 of a token: the only form that reaches the database.
pub fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Sliding-window counter per key, in memory. Good enough for one server process; a cluster would
/// move this into a shared store.
pub struct RateLimiter {
    limit: Limit,
    events: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: Limit) -> Self {
        Self {
            limit,
            events: Mutex::new(HashMap::new()),
        }
    }

    /// Atomically reserve an attempt BEFORE expensive work. Concurrent failures cannot bypass it.
    pub fn hit(&self, key: &str) -> Result<()> {
        let now = Instant::now();
        let mut map = self.lock();
        if map.len() >= 50_000 {
            map.retain(|_, q| {
                prune(q, now, self.limit.window);
                !q.is_empty()
            });
            if map.len() >= 50_000 && !map.contains_key(key) {
                return Err(AccountsError::RateLimited { retry_after_secs: 60 });
            }
        }
        let q = map.entry(key.to_owned()).or_default();
        prune(q, now, self.limit.window);
        if q.len() >= self.limit.max {
            return Err(AccountsError::RateLimited {
                retry_after_secs: self.limit.window.as_secs().max(1),
            });
        }
        q.push_back(now);
        Ok(())
    }

    /// `Err(RateLimited)` when the key already used up its window.
    pub fn check(&self, key: &str) -> Result<()> {
        self.check_at(key, Instant::now())
    }

    pub fn record(&self, key: &str) {
        self.record_at(key, Instant::now());
    }

    pub fn reset(&self, key: &str) {
        self.lock().remove(key);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, VecDeque<Instant>>> {
        self.events.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<()> {
        let mut map = self.lock();
        let Some(q) = map.get_mut(key) else { return Ok(()) };
        prune(q, now, self.limit.window);
        if q.len() >= self.limit.max {
            let oldest = q.front().copied().unwrap_or(now);
            let wait = self.limit.window.saturating_sub(now.saturating_duration_since(oldest));
            return Err(AccountsError::RateLimited {
                retry_after_secs: wait.as_secs().max(1),
            });
        }
        Ok(())
    }

    fn record_at(&self, key: &str, now: Instant) {
        let mut map = self.lock();
        // Keep memory bounded: drop idle keys once the map grows.
        if map.len() > 50_000 {
            let window = self.limit.window;
            map.retain(|_, q| {
                prune(q, now, window);
                !q.is_empty()
            });
        }
        let q = map.entry(key.to_string()).or_default();
        prune(q, now, self.limit.window);
        q.push_back(now);
    }
}

fn prune(q: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while q.front().is_some_and(|t| now.saturating_duration_since(*t) >= window) {
        q.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_password_capacity_is_shared_and_test_capacity_is_isolated() {
        let a = crate::AccountsConfig::default();
        let b = crate::AccountsConfig::default();
        let from_env = crate::AccountsConfig::from_env();
        assert!(Arc::ptr_eq(&a.password_work.0, &b.password_work.0));
        assert!(Arc::ptr_eq(&a.password_work.0, &from_env.password_work.0));

        let test = crate::AccountsConfig::for_tests();
        let clone = test.clone();
        let other = crate::AccountsConfig::for_tests();
        assert!(Arc::ptr_eq(&test.password_work.0, &clone.password_work.0));
        assert!(!Arc::ptr_eq(&test.password_work.0, &a.password_work.0));
        assert!(!Arc::ptr_eq(&test.password_work.0, &other.password_work.0));
    }

    #[tokio::test]
    async fn account_router_returns_429_when_password_capacity_is_full() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode, header};
        use tower::ServiceExt;

        let config = crate::AccountsConfig::for_tests();
        let app = crate::try_router(config.clone()).unwrap();
        let full = config.password_work.0.clone().try_acquire_many_owned(4).unwrap();
        let request = || {
            Request::post("/api/account/signup")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"capacity@example.org","password":"a long enough passphrase"}"#,
                ))
                .unwrap()
        };
        let response = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
        drop(full);
        assert_eq!(app.oneshot(request()).await.unwrap().status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn password_work_rejects_a_fifth_job_before_spawning_and_recovers() {
        let config = crate::AccountsConfig::for_tests();
        let mut workers = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..4 {
            let work = config.clone().password_work;
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            workers.push(tokio::spawn(async move {
                work.run(move || {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                })
                .await
            }));
            releases.push(release_tx);
            started_rx.await.unwrap();
        }
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        assert!(matches!(
            config
                .password_work
                .run(move || flag.store(true, std::sync::atomic::Ordering::SeqCst))
                .await,
            Err(AccountsError::RateLimited { retry_after_secs: 1 })
        ));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        // Another test's router still has capacity while this one is saturated.
        assert_eq!(
            crate::AccountsConfig::for_tests()
                .password_work
                .run(|| 7)
                .await
                .unwrap(),
            7
        );
        drop(releases);
        for worker in workers {
            worker.await.unwrap().unwrap();
        }
        assert_eq!(config.password_work.0.available_permits(), 4);
        assert_eq!(config.password_work.run(|| 8).await.unwrap(), 8);
    }

    #[tokio::test]
    async fn cancelling_password_work_keeps_capacity_reserved_until_the_worker_finishes() {
        let work = PasswordWork::isolated();
        let worker_work = work.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let caller = tokio::spawn(async move {
            worker_work
                .run(move || {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                })
                .await
        });
        started_rx.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert_eq!(work.0.available_permits(), 3);
        drop(release_tx);
        // Wait for the uncancellable blocking worker, not merely the aborted async caller.
        let recovered = work.0.clone().acquire_many_owned(4).await.unwrap();
        drop(recovered);
        assert_eq!(work.run(|| 9).await.unwrap(), 9);
    }

    #[test]
    fn hashes_are_argon2id_and_verify() {
        let h = Hasher::new(PasswordCost::insecure_fast()).unwrap();
        let pw = Secret::new("correct horse battery");
        let phc = h.hash(&pw).unwrap();
        assert!(phc.starts_with("$argon2id$v=19$"));
        assert!(!phc.contains("correct horse"));
        assert!(h.verify(&pw, &phc));
        assert!(!h.verify(&Secret::new("wrong horse battery"), &phc));
        // Two hashes of the same password differ (random salt).
        assert_ne!(phc, h.hash(&pw).unwrap());
    }

    #[test]
    fn secrets_are_redacted() {
        let s = Secret::new("hunter2hunter2");
        assert_eq!(format!("{s:?} {s}"), "Secret(***) ***");
    }

    #[test]
    fn password_policy_is_length_only() {
        assert!(check_new_password(&Secret::new("short")).is_err());
        assert!(check_new_password(&Secret::new("ten chars!")).is_ok());
        assert!(check_new_password(&Secret::new("x".repeat(MAX_PASSWORD_BYTES + 1))).is_err());
    }

    #[test]
    fn tokens_are_random_and_only_hashes_are_kept() {
        let (a, ha) = new_session_token();
        let (b, _) = new_session_token();
        assert_eq!(a.expose().len(), 43);
        assert_ne!(a.expose(), b.expose());
        assert_eq!(ha, token_hash(a.expose()));
        assert_eq!(ha.len(), 64);
    }

    #[test]
    fn concurrent_attempts_cannot_exceed_the_limit() {
        let limiter = std::sync::Arc::new(RateLimiter::new(Limit {
            max: 5,
            window: Duration::from_secs(60),
        }));
        let joins: Vec<_> = (0..32)
            .map(|_| {
                let limiter = limiter.clone();
                std::thread::spawn(move || limiter.hit("one-account").is_ok())
            })
            .collect();
        assert_eq!(
            joins.into_iter().map(|j| usize::from(j.join().unwrap())).sum::<usize>(),
            5
        );
    }

    #[test]
    fn rate_limiter_blocks_then_recovers() {
        let rl = RateLimiter::new(Limit {
            max: 2,
            window: Duration::from_secs(60),
        });
        let t0 = Instant::now();
        rl.record_at("k", t0);
        rl.record_at("k", t0);
        match rl.check_at("k", t0 + Duration::from_secs(10)) {
            Err(AccountsError::RateLimited { retry_after_secs }) => assert_eq!(retry_after_secs, 50),
            other => panic!("expected rate limit, got {other:?}"),
        }
        assert!(rl.check_at("other", t0).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(61)).is_ok());
    }
}
