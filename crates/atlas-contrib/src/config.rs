//! Configuration: database and overlay location, reviewers, rate limit, fetch policy.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{ContribError, Result};
use crate::fetch::FetchPolicy;
use crate::store::Database;

/// Four concurrent contribution checks, shared process-wide by production configurations.
/// Independent test configurations isolate capacity; clones always share it.
#[derive(Clone, Debug)]
pub struct CheckWork(Arc<tokio::sync::Semaphore>);

impl Default for CheckWork {
    fn default() -> Self {
        static GATE: OnceLock<CheckWork> = OnceLock::new();
        GATE.get_or_init(Self::isolated).clone()
    }
}

impl CheckWork {
    fn isolated() -> Self {
        Self(Arc::new(tokio::sync::Semaphore::new(4)))
    }

    pub(crate) fn acquire(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.0
            .clone()
            .try_acquire_owned()
            .map_err(|_| ContribError::RateLimited { retry_after_secs: 10 })
    }
}

/// At most `max` submissions per `window` for one client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub max: usize,
    pub window: Duration,
}

/// A reviewer who signs in with a bearer token. Only the token's SHA-256 is kept.
#[derive(Clone, PartialEq, Eq)]
pub struct ReviewerToken {
    pub name: String,
    pub sha256: [u8; 32],
}

impl ReviewerToken {
    pub fn new(name: &str, token: &str) -> Self {
        Self {
            name: name.to_string(),
            sha256: Sha256::digest(token.as_bytes()).into(),
        }
    }
}

impl std::fmt::Debug for ReviewerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReviewerToken({}, ***)", self.name)
    }
}

#[derive(Clone, Debug)]
pub struct ContribConfig {
    pub database: Database,
    /// Where accepted contributions are written as an overlay (`None`: only served over HTTP).
    pub overlay_path: Option<PathBuf>,
    /// Accepted data sources, for offline discovery; never a graph overlay.
    pub discovery_candidates_path: Option<PathBuf>,
    /// Reviewers with a bearer token (`Authorization: Bearer <token>`).
    pub reviewer_tokens: Vec<ReviewerToken>,
    /// Account ids (from the user resolver, e.g. atlas-accounts) allowed to review.
    pub reviewer_users: Vec<String>,
    pub submit_limit: Limit,
    /// Use the first `X-Forwarded-For` address as the client key (behind the web server / proxy).
    pub trust_forwarded_for: bool,
    pub fetch: FetchPolicy,
    /// Run the auto-checks in the background right after a submission.
    pub check_on_submit: bool,
    /// Admission for expensive checks; production defaults share a four-job process-wide cap.
    pub check_work: CheckWork,
}

impl Default for ContribConfig {
    fn default() -> Self {
        let db = data_root().join("app").join("contrib.sqlite");
        Self {
            discovery_candidates_path: Some(db.with_file_name("discovery-candidates.json")),
            overlay_path: Some(overlay_next_to(&db)),
            database: Database::File(db),
            reviewer_tokens: Vec::new(),
            reviewer_users: Vec::new(),
            submit_limit: Limit {
                max: 20,
                window: Duration::from_secs(3600),
            },
            trust_forwarded_for: false,
            fetch: FetchPolicy::default(),
            check_on_submit: true,
            check_work: CheckWork::default(),
        }
    }
}

impl ContribConfig {
    /// Defaults plus the environment:
    /// - `RARE_ATLAS_CONTRIB_DB`: database file (`:memory:` for an in-memory one); otherwise
    ///   `$RARE_ATLAS_DATA/app/contrib.sqlite`, otherwise `<nearest ./data>/app/contrib.sqlite`;
    /// - `RARE_ATLAS_CONTRIB_OVERLAY`: overlay file (default `contrib-overlay.json` next to the db);
    /// - `RARE_ATLAS_DISCOVERY_CANDIDATES`: accepted source queue (default next to the db);
    /// - `RARE_ATLAS_REVIEW_TOKENS`: `name:token,name:token` (reviewers by bearer token);
    /// - `RARE_ATLAS_REVIEWERS`: account ids allowed to review (with a user resolver);
    /// - `RARE_ATLAS_TRUST_FORWARDED_FOR=1`: explicitly trust a proxy that overwrites `X-Forwarded-For`.
    pub fn from_env() -> Self {
        let mut c = Self::default();
        match std::env::var("RARE_ATLAS_CONTRIB_DB") {
            Ok(v) if v == ":memory:" => {
                c.database = Database::InMemory;
                c.overlay_path = None;
                c.discovery_candidates_path = None;
            }
            Ok(v) if !v.trim().is_empty() => {
                let p = PathBuf::from(v);
                c.overlay_path = Some(overlay_next_to(&p));
                c.discovery_candidates_path = Some(p.with_file_name("discovery-candidates.json"));
                c.database = Database::File(p);
            }
            _ => {}
        }
        if let Ok(v) = std::env::var("RARE_ATLAS_CONTRIB_OVERLAY")
            && !v.trim().is_empty()
        {
            c.overlay_path = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("RARE_ATLAS_REVIEW_TOKENS") {
            c.reviewer_tokens = parse_tokens(&v);
        }
        if let Ok(v) = std::env::var("RARE_ATLAS_DISCOVERY_CANDIDATES")
            && !v.trim().is_empty()
        {
            c.discovery_candidates_path = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("RARE_ATLAS_REVIEWERS") {
            c.reviewer_users = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        if let Ok(v) = std::env::var("RARE_ATLAS_TRUST_FORWARDED_FOR") {
            c.trust_forwarded_for = !matches!(v.as_str(), "0" | "false" | "no");
        }
        c
    }

    /// In-memory, no overlay file, reviewer `tester` with token `test-token`, checks run only
    /// when called. Trusts the mock client's forwarding header; production defaults do not.
    /// Private HTTP targets are permitted only in the crate's unit-test build.
    pub fn for_tests() -> Self {
        Self {
            database: Database::InMemory,
            overlay_path: None,
            discovery_candidates_path: None,
            reviewer_tokens: vec![ReviewerToken::new("tester", "test-token")],
            reviewer_users: Vec::new(),
            submit_limit: Limit {
                max: 1000,
                window: Duration::from_secs(60),
            },
            trust_forwarded_for: true,
            fetch: FetchPolicy {
                allow_private: true,
                ..FetchPolicy::default()
            },
            check_on_submit: false,
            check_work: CheckWork::isolated(),
        }
    }
}

/// `name:token` pairs; a token without a name gets `reviewer-<n>`. Tokens shorter than 16
/// characters are ignored (too easy to guess).
pub fn parse_tokens(v: &str) -> Vec<ReviewerToken> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
        .filter_map(|(i, pair)| {
            let (name, token) = match pair.split_once(':') {
                Some((n, t)) => (n.trim().to_string(), t.trim()),
                None => (format!("reviewer-{}", i + 1), pair),
            };
            (token.len() >= 16).then(|| ReviewerToken::new(&name, token))
        })
        .collect()
}

fn overlay_next_to(db: &Path) -> PathBuf {
    db.with_file_name("contrib-overlay.json")
}

fn data_root() -> PathBuf {
    if let Ok(v) = std::env::var("RARE_ATLAS_DATA")
        && !v.trim().is_empty()
    {
        return PathBuf::from(v);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.ancestors()
        .map(|dir| dir.join("data"))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| cwd.join("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_check_capacity_is_shared_and_test_capacity_is_isolated() {
        let a = ContribConfig::default();
        let b = ContribConfig::default();
        let from_env = ContribConfig::from_env();
        assert!(Arc::ptr_eq(&a.check_work.0, &b.check_work.0));
        assert!(Arc::ptr_eq(&a.check_work.0, &from_env.check_work.0));
        let test = ContribConfig::for_tests();
        let clone = test.clone();
        let other = ContribConfig::for_tests();
        assert!(Arc::ptr_eq(&test.check_work.0, &clone.check_work.0));
        assert!(!Arc::ptr_eq(&test.check_work.0, &other.check_work.0));
        assert!(!Arc::ptr_eq(&test.check_work.0, &a.check_work.0));
    }

    #[test]
    fn contribution_check_admission_rejects_the_fifth_and_recovers() {
        let test = ContribConfig::for_tests();
        let permits: Vec<_> = (0..4).map(|_| test.clone().check_work.acquire().unwrap()).collect();
        assert!(matches!(
            test.check_work.acquire(),
            Err(ContribError::RateLimited { retry_after_secs: 10 })
        ));
        assert!(ContribConfig::for_tests().check_work.acquire().is_ok());
        drop(permits);
        assert_eq!(test.check_work.0.available_permits(), 4);
        assert!(test.check_work.acquire().is_ok());
    }
}
