//! Who is asking: an optional signed-in user (resolved by the host, e.g. from the atlas-accounts
//! session cookie), reviewers (bearer token or listed account), and the submission rate limit.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::ConnectInfo;
use axum::http::{Extensions, HeaderMap, header};
use sha2::{Digest, Sha256};

use crate::config::{ContribConfig, Limit};
use crate::error::{ContribError, Result};
use crate::model::Agent;

/// A signed-in account, as the host's resolver reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserRef {
    pub id: String,
    pub name: Option<String>,
    /// The host resolves this from the authenticated account, never request JSON.
    pub email: Option<String>,
}

/// Resolves the signed-in user from request headers (cookie or bearer). Without one, every
/// contribution is anonymous (with an optional contact).
pub type UserResolver = Arc<dyn Fn(&HeaderMap) -> Option<UserRef> + Send + Sync>;

/// The reviewer behind a request, or `Unauthorized`.
pub fn reviewer(config: &ContribConfig, users: Option<&UserResolver>, headers: &HeaderMap) -> Result<Agent> {
    if let Some(token) = bearer(headers) {
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        // Compare every configured hash in full, so timing doesn't reveal which one matched.
        let mut found: Option<&str> = None;
        for t in &config.reviewer_tokens {
            if constant_time_eq(&t.sha256, &digest) {
                found = Some(&t.name);
            }
        }
        if let Some(name) = found {
            return Ok(Agent::person(
                format!("agent:reviewer/{name}"),
                Some(format!("reviewer {name}")),
            ));
        }
    }
    if let Some(user) = users.and_then(|r| r(headers))
        && config.reviewer_users.iter().any(|u| u == &user.id)
    {
        return Ok(Agent::person(
            format!("agent:user/{}", user.id),
            user.name.or(Some("reviewer".into())),
        ));
    }
    Err(ContribError::Unauthorized)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))?.trim();
    (!token.is_empty()).then_some(token)
}

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Rate-limit key: first `X-Forwarded-For` address when trusted, else the socket address.
pub fn client_key(trust_forwarded_for: bool, headers: &HeaderMap, extensions: &Extensions) -> String {
    if trust_forwarded_for
        && let Some(first) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        return first.to_string();
    }
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Sliding-window counter per key, in memory (one server process).
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

    /// Counts one event for `key`, or `RateLimited` when the window is full.
    pub fn hit(&self, key: &str) -> Result<()> {
        let now = Instant::now();
        let window = self.limit.window;
        let mut map = self.events.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() > 50_000 {
            map.retain(|_, q| q.back().is_some_and(|t| now.saturating_duration_since(*t) < window));
        }
        let q = map.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now.saturating_duration_since(*t) >= window) {
            q.pop_front();
        }
        if q.len() >= self.limit.max {
            let oldest = q.front().copied().unwrap_or(now);
            let wait = window.saturating_sub(now.saturating_duration_since(oldest));
            return Err(ContribError::RateLimited {
                retry_after_secs: wait.as_secs().max(1),
            });
        }
        q.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_ignores_forwarded_identity_and_uses_the_socket() {
        let config = ContribConfig::default();
        assert!(!config.trust_forwarded_for);
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.99".parse().unwrap());
        let mut extensions = Extensions::new();
        assert_eq!(client_key(config.trust_forwarded_for, &headers, &extensions), "unknown");
        extensions.insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 10], 12345))));
        assert_eq!(
            client_key(config.trust_forwarded_for, &headers, &extensions),
            "198.51.100.10"
        );
    }

    #[test]
    fn test_clients_have_distinct_forwarded_identities() {
        let config = ContribConfig::for_tests();
        assert!(config.trust_forwarded_for);
        let mut headers = HeaderMap::new();
        let extensions = Extensions::new();
        for ip in ["198.51.100.1", "198.51.100.2"] {
            headers.insert("x-forwarded-for", ip.parse().unwrap());
            assert_eq!(client_key(config.trust_forwarded_for, &headers, &extensions), ip);
        }
        headers.insert("x-forwarded-for", "198.51.100.3, 203.0.113.99".parse().unwrap());
        assert_eq!(
            client_key(config.trust_forwarded_for, &headers, &extensions),
            "198.51.100.3"
        );
    }
}
