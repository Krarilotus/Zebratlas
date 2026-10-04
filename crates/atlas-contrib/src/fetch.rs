//! Fetching evidence and contact pages for the auto-checks.
//!
//! Contributions carry URLs typed by anyone, so the fetcher is defensive: `http`/`https` only, no
//! private, loopback or link-local addresses (resolved and checked on every
//! redirect), at most 5 redirects, a 10 s deadline and a 3 MiB body cap. The body's SHA-256 and the
//! retrieval time are recorded so the check itself can be re-checked (D10).
//!
//! Each hop connects only to its validated addresses through a pinned resolver. Ambient proxies
//! are disabled so they cannot bypass the address checks. The deadline and byte cap cover all hops.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::model::FetchRecord;
use crate::util::{hex, now_rfc3339};

/// A fetched page: the record (kept in the check report) plus the body text (used, not stored).
#[derive(Clone, Debug)]
pub struct Fetched {
    pub record: FetchRecord,
    /// Successful bounded response bytes, retained by data-source checks for replay.
    pub body: Option<Vec<u8>>,
    /// First 64 KiB of UTF-8 response for structured metadata checks; not stored in receipts.
    pub sample: Option<String>,
    /// Visible text when the response was 2xx HTML or plain text.
    pub text: Option<String>,
}

impl Fetched {
    pub fn ok(&self) -> bool {
        self.record.outcome == "ok"
    }

    pub fn failed(url: &str, outcome: &str, status: Option<u16>) -> Self {
        Self {
            record: FetchRecord {
                url: url.to_string(),
                final_url: None,
                status,
                retrieved_at: now_rfc3339(),
                sha256: None,
                bytes: 0,
                content_type: None,
                outcome: outcome.to_string(),
            },
            text: None,
            sample: None,
            body: None,
        }
    }
}

/// Fetches one URL. The HTTP implementation is [`HttpFetcher`]; tests may supply their own.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Fetched;
}

#[derive(Clone, Copy, Debug)]
pub struct FetchPolicy {
    /// Allow loopback/private targets in unit tests. Ignored in non-test builds.
    pub allow_private: bool,
    pub timeout: Duration,
    pub max_bytes: usize,
    pub max_redirects: usize,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            allow_private: false,
            timeout: Duration::from_secs(10),
            max_bytes: 3 * 1024 * 1024,
            max_redirects: 5,
        }
    }
}

pub struct HttpFetcher {
    resolver: Arc<dyn Lookup>,
    policy: FetchPolicy,
}

#[async_trait]
trait Lookup: Send + Sync {
    async fn lookup(&self, name: &str, port: u16) -> Result<Vec<SocketAddr>, &'static str>;
}

struct SystemLookup;

#[async_trait]
impl Lookup for SystemLookup {
    async fn lookup(&self, name: &str, port: u16) -> Result<Vec<SocketAddr>, &'static str> {
        tokio::net::lookup_host((name, port))
            .await
            .map(|a| a.collect())
            .map_err(|_| "network")
    }
}

struct Pinned {
    host: String,
    addrs: Vec<SocketAddr>,
}

impl Resolve for Pinned {
    fn resolve(&self, name: Name) -> Resolving {
        let allowed = name.as_str() == self.host;
        let addrs = self.addrs.clone();
        Box::pin(async move {
            if !allowed {
                return Err(std::io::Error::other("unvalidated host").into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

impl HttpFetcher {
    pub fn new(policy: FetchPolicy) -> Self {
        Self {
            resolver: Arc::new(SystemLookup),
            policy,
        }
    }

    fn client(&self, url: &Url, addrs: Vec<SocketAddr>) -> Result<reqwest::Client, &'static str> {
        reqwest::Client::builder()
            .user_agent(concat!(
                "rare-disease-atlas-contrib/",
                env!("CARGO_PKG_VERSION"),
                " (evidence check for a user contribution)"
            ))
            .timeout(self.policy.timeout)
            .connect_timeout(Duration::from_secs(5))
            .redirect(Policy::none())
            .no_proxy()
            .dns_resolver(Arc::new(Pinned {
                host: url.host_str().ok_or("invalid_url")?.into(),
                addrs,
            }))
            .build()
            .map_err(|_| "network")
    }

    /// `Err(outcome)` when the URL must not be fetched.
    async fn guard(&self, url: &Url) -> Result<Vec<SocketAddr>, &'static str> {
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            return Err("invalid_url");
        }
        let allow_private = cfg!(test) && self.policy.allow_private;
        let Some(host) = url.host() else {
            return Err("invalid_url");
        };
        if !allow_private && host_literal_blocked(&host) {
            return Err("blocked");
        }
        let port = url.port_or_known_default().ok_or("invalid_url")?;
        let addrs = match host {
            Host::Domain(name) => self.resolver.lookup(name, port).await?,
            Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
            Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
        };
        if addrs.is_empty() {
            return Err("network");
        }
        if !allow_private && addrs.iter().any(|a| blocked_ip(a.ip())) {
            return Err("blocked");
        }
        Ok(addrs)
    }
}

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new(FetchPolicy::default())
    }
}

#[async_trait]
impl Fetcher for HttpFetcher {
    async fn fetch(&self, raw: &str) -> Fetched {
        match tokio::time::timeout(self.policy.timeout, self.fetch_bounded(raw)).await {
            Ok(fetched) => fetched,
            Err(_) => Fetched::failed(raw, "timeout", None),
        }
    }
}

impl HttpFetcher {
    async fn fetch_bounded(&self, raw: &str) -> Fetched {
        let Ok(mut url) = Url::parse(raw) else {
            return Fetched::failed(raw, "invalid_url", None);
        };
        let retrieved_at = now_rfc3339();
        let mut redirects = 0;
        let mut total_bytes = 0usize;
        loop {
            let addrs = match self.guard(&url).await {
                Ok(addrs) => addrs,
                Err(outcome) => return Fetched::failed(raw, outcome, None),
            };
            let client = match self.client(&url, addrs) {
                Ok(client) => client,
                Err(outcome) => return Fetched::failed(raw, outcome, None),
            };
            let mut res = match client.get(url.clone()).send().await {
                Ok(r) => r,
                Err(e) => {
                    let outcome = if e.is_timeout() {
                        "timeout"
                    } else if e.is_redirect() {
                        "blocked"
                    } else {
                        "network"
                    };
                    return Fetched::failed(raw, outcome, None);
                }
            };
            let status = res.status();
            let location = res
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let final_url = res.url().to_string();
            let content_type = res
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let mut body: Vec<u8> = Vec::new();
            let mut too_large = false;
            loop {
                match res.chunk().await {
                    Ok(Some(chunk)) => {
                        if chunk.len() > self.policy.max_bytes.saturating_sub(total_bytes) {
                            too_large = true;
                            break;
                        }
                        total_bytes += chunk.len();
                        body.extend_from_slice(&chunk);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let outcome = if e.is_timeout() { "timeout" } else { "network" };
                        return Fetched::failed(raw, outcome, Some(status.as_u16()));
                    }
                }
            }
            if !too_large
                && matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
                && let Some(location) = location
            {
                if redirects >= self.policy.max_redirects {
                    return Fetched::failed(raw, "blocked", Some(status.as_u16()));
                }
                url = match url.join(&location) {
                    Ok(next) => next,
                    Err(_) => return Fetched::failed(raw, "invalid_url", Some(status.as_u16())),
                };
                redirects += 1;
                continue;
            }
            let outcome = if too_large {
                "too_large"
            } else if status.is_success() {
                "ok"
            } else {
                "http_status"
            };
            let textual = content_type.as_deref().is_none_or(|c| {
                let c = c.to_ascii_lowercase();
                c.starts_with("text/")
                    || c.contains("html")
                    || c.contains("xml")
                    || c.contains("json")
                    || c.contains("rdf")
                    || c.contains("turtle")
                    || c.contains("csv")
                    || c.contains("yaml")
            });
            let text = (outcome == "ok" && textual).then(|| {
                let s = String::from_utf8_lossy(&body);
                let html = content_type.as_deref().is_none_or(|c| c.contains("html")) || s.contains("<html");
                if html {
                    crate::text::html_to_text(&s)
                } else {
                    s.into_owned()
                }
            });
            return Fetched {
                sample: (outcome == "ok" && textual)
                    .then(|| std::str::from_utf8(&body).ok())
                    .flatten()
                    .map(|s| {
                        let mut end = s.len().min(64 * 1024);
                        while !s.is_char_boundary(end) {
                            end -= 1;
                        }
                        s[..end].to_owned()
                    }),
                record: FetchRecord {
                    url: raw.to_string(),
                    final_url: (final_url != raw).then_some(final_url),
                    status: Some(status.as_u16()),
                    retrieved_at,
                    sha256: (!too_large).then(|| hex(&Sha256::digest(&body))),
                    bytes: body.len() as u64,
                    content_type,
                    outcome: outcome.to_string(),
                },
                text,
                body: (outcome == "ok").then_some(body),
            };
        }
    }
}

fn host_literal_blocked(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => blocked_ip(IpAddr::V4(*ip)),
        Host::Ipv6(ip) => blocked_ip(IpAddr::V6(*ip)),
        Host::Domain(name) => {
            let n = name.trim_end_matches('.').to_ascii_lowercase();
            n == "localhost" || n.ends_with(".localhost") || n.ends_with(".local") || n.ends_with(".internal")
        }
    }
}

/// Addresses a contribution must never make the server talk to.
pub fn blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => blocked_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return blocked_v4(v4);
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                // Transition mechanisms can embed IPv4 destinations. Do not permit bypasses
                // through NAT64, 6to4 or Teredo; public evidence sites have ordinary A/AAAA.
                || v6.segments()[..6] == [0x64, 0xff9b, 0, 0, 0, 0]
                || v6.segments()[..3] == [0x64, 0xff9b, 1]
                || first == 0x2002
                || v6.segments()[..2] == [0x2001, 0]
                || first == 0 // IPv4-compatible/special addresses
                || (first & 0xffc0) == 0xfec0 // deprecated site-local
                || v6.segments()[..2] == [0x2001, 0xdb8] // documentation
        }
    }
}

fn blocked_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 192 && b == 0 && ip.octets()[2] == 0) // special-use protocol addresses
        || ip == Ipv4Addr::new(168, 63, 129, 16) // Azure platform virtual address
        || a >= 240
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct MockLookup {
        answers: Vec<Vec<SocketAddr>>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Lookup for MockLookup {
        async fn lookup(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, &'static str> {
            let i = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.answers[i.min(self.answers.len() - 1)].clone())
        }
    }

    fn mock_lookup(answers: &[&[&str]]) -> Arc<MockLookup> {
        Arc::new(MockLookup {
            answers: answers
                .iter()
                .map(|a| a.iter().map(|s| s.parse().unwrap()).collect())
                .collect(),
            calls: AtomicUsize::new(0),
        })
    }

    #[tokio::test]
    async fn rejects_private_redirect_domain_and_mixed_dns_answers() {
        let resolver = mock_lookup(&[&["8.8.8.8:80"], &["169.254.169.254:80"]]);
        let mut f = HttpFetcher {
            resolver: resolver.clone(),
            ..Default::default()
        };
        let origin = Url::parse("http://public.example/").unwrap();
        assert!(f.guard(&origin).await.is_ok());
        let redirect = origin.join("http://metadata.example/latest/meta-data").unwrap();
        assert_eq!(f.guard(&redirect).await, Err("blocked"));
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
        f.resolver = mock_lookup(&[&["8.8.8.8:80", "10.0.0.1:80"]]);
        assert_eq!(f.fetch("http://mixed.example/").await.record.outcome, "blocked");
        f.resolver = mock_lookup(&[&[]]);
        assert_eq!(f.fetch("http://empty.example/").await.record.outcome, "network");
    }

    #[tokio::test]
    async fn pins_validated_addresses_instead_of_resolving_again_at_connect() {
        let server = MockServer::start().await;
        Mock::given(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_string("evidence"))
            .mount(&server)
            .await;
        let origin = Url::parse(&server.uri()).unwrap();
        let good = format!("127.0.0.1:{}", origin.port().unwrap());
        let resolver = mock_lookup(&[&[&good], &["127.0.0.1:9"]]);
        let mut f = HttpFetcher::new(FetchPolicy {
            allow_private: true,
            ..FetchPolicy::default()
        });
        f.resolver = resolver.clone();
        let out = f.fetch("http://rebind.example/page").await;
        assert_eq!(out.record.outcome, "ok");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pinned_resolver_fails_closed_for_another_hostname() {
        let pinned = Pinned {
            host: "public.example".into(),
            addrs: vec!["8.8.8.8:80".parse().unwrap()],
        };
        assert!(pinned.resolve("private.example".parse().unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn redirects_share_deadline_and_byte_budget() {
        let server = MockServer::start().await;
        Mock::given(path("/start"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "/end")
                    .set_body_string("1234")
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        Mock::given(path("/end"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("5678")
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        let url = format!("{}/start", server.uri());
        let mut policy = FetchPolicy {
            allow_private: true,
            max_bytes: 6,
            ..FetchPolicy::default()
        };
        assert_eq!(HttpFetcher::new(policy).fetch(&url).await.record.outcome, "too_large");
        policy.max_bytes = 100;
        policy.timeout = Duration::from_millis(250);
        assert_eq!(HttpFetcher::new(policy).fetch(&url).await.record.outcome, "timeout");
        policy.timeout = Duration::from_secs(2);
        policy.max_redirects = 0;
        assert_eq!(HttpFetcher::new(policy).fetch(&url).await.record.outcome, "blocked");
        policy.max_redirects = 1;
        let fetched = HttpFetcher::new(policy).fetch(&url).await;
        assert!(fetched.ok());
        assert_eq!(fetched.record.bytes, 4);
        assert_eq!(fetched.record.sha256, Some(hex(&Sha256::digest(b"5678"))));
        assert_eq!(fetched.record.final_url, Some(format!("{}/end", server.uri())));
    }

    #[test]
    fn blocks_private_and_local_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.5.4",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
        ] {
            assert!(blocked_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "2002:7f00:1::1",
            "fec0::1",
        ] {
            assert!(blocked_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "151.101.1.69", "2606:4700::1111"] {
            assert!(!blocked_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn refuses_local_targets_by_default() {
        let f = HttpFetcher::default();
        for url in [
            "http://127.0.0.1:9/x",
            "http://localhost/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data",
        ] {
            assert_eq!(f.fetch(url).await.record.outcome, "blocked", "{url}");
        }
        assert_eq!(f.fetch("ftp://example.org/x").await.record.outcome, "invalid_url");
    }
}
