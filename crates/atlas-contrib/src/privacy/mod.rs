//! "Request removal of my data" for everyone (D43, GDPR Art. 12(3), 17, 21).
//!
//! - `POST /api/privacy/requests` works without an account: e-mail (required), what it concerns
//!   (an entry link/id, a name or ORCID, or a description) and the type (`remove` | `correct` |
//!   `object`). It returns a reference; nothing is sent externally (the acknowledgement e-mail is a
//!   stub, see [`Mailer`]).
//! - Reviewers (the contribution reviewers: bearer token or listed account) list, trace and decide.
//!   Approving `remove`/`object` writes a suppression entry through [`PrivacyActions`] (the host
//!   implements it with `atlas_ingest::trace` + `atlas_core::withhold`); without one, approval fails.
//! - Retention: see [`store`].

mod copy;
mod routes;
pub mod store;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::auth::{RateLimiter, UserResolver};
use crate::config::{ContribConfig, Limit};
use crate::error::{ContribError, Result};
use crate::model::{Agent, clean_req};
use crate::util::hex;

pub use routes::router;
pub use store::{PrivacyStore, Retention};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    /// Erasure (Art. 17).
    Remove,
    /// Rectification (Art. 16).
    Correct,
    /// Objection (Art. 21).
    Object,
}

impl RequestType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remove => "remove",
            Self::Correct => "correct",
            Self::Object => "object",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Remove, Self::Correct, Self::Object]
            .into_iter()
            .find(|t| t.as_str() == s)
    }

    /// Suppression reason written on approval.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Remove => "gdpr_art17_erasure",
            Self::Correct => "gdpr_art16_rectification",
            Self::Object => "gdpr_art21_objection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    Received,
    /// Identity check under way (e.g. waiting for a reply from the ORCID-linked address).
    Verifying,
    Approved,
    Rejected,
}

impl RequestState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Verifying => "verifying",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Received, Self::Verifying, Self::Approved, Self::Rejected]
            .into_iter()
            .find(|t| t.as_str() == s)
    }
}

/// The public form, exactly (D42: server validation matches the form).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyInput {
    pub email: String,
    pub concerns: String,
    #[serde(rename = "type")]
    pub kind: RequestType,
    /// Honeypot: hidden in the form; bots fill it. Non-empty = silently accepted, not stored.
    #[serde(default)]
    pub website: String,
}

/// A stored request. `email` and `concerns` are `None` after retention erased them.
#[derive(Clone, Debug, Serialize)]
pub struct PrivacyRequest {
    pub reference: String,
    #[serde(rename = "type")]
    pub kind: RequestType,
    pub state: RequestState,
    pub email: Option<String>,
    pub concerns: Option<String>,
    /// Salted hash of the e-mail (per-address limit, matching follow-ups); never the address.
    #[serde(skip)]
    pub email_hash: String,
    pub created_at: String,
    /// Art. 12(3): one month.
    pub respond_by: String,
    pub decided_at: Option<String>,
    pub reviewer: Option<String>,
    pub decision_reason: Option<String>,
    pub suppression_entry: Option<String>,
    pub redacted_at: Option<String>,
    pub delete_after: Option<String>,
}

/// Explicit trace parameters a reviewer may give (otherwise derived from `concerns`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceParams {
    pub orcid: Option<String>,
    pub name: Option<String>,
    pub affiliation: Option<String>,
    pub email: Option<String>,
    pub node: Option<String>,
}

impl TraceParams {
    /// Best effort from free text: an ORCID, an entry link (`…/person/<id>`, `?id=`), else a
    /// short text as a name. The reviewer confirms or overrides before approving.
    pub fn from_concerns(concerns: &str, email: Option<&str>) -> Self {
        let mut p = Self {
            email: email.map(str::to_owned),
            ..Self::default()
        };
        let t = concerns.trim();
        let orcid_re = regex::Regex::new(r"\b\d{4}-\d{4}-\d{4}-\d{3}[\dXx]\b").expect("static regex");
        if let Some(m) = orcid_re.find(t) {
            p.orcid = Some(m.as_str().to_uppercase());
        }
        if let Ok(u) = url::Url::parse(t) {
            let id = u
                .query_pairs()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.into_owned())
                .or_else(|| u.path_segments().and_then(|mut s| s.next_back()).map(str::to_owned));
            p.node = id
                .map(|s| percent_decode(&s))
                .filter(|s| !s.is_empty() && p.orcid.is_none());
        } else if p.orcid.is_none() && t.chars().count() <= 80 && !t.chars().any(|c| c.is_ascii_digit()) {
            p.name = Some(t.to_owned());
        }
        p
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What approval asks the host to do.
#[derive(Clone, Debug)]
pub struct SuppressOrder<'a> {
    pub reference: &'a str,
    pub kind: RequestType,
    pub trace: &'a TraceParams,
    /// Extra node ids the reviewer confirmed.
    pub nodes: &'a [String],
    pub reviewer: &'a str,
}

/// Host side effects: provenance trace and suppression list (atlas-server implements both).
pub trait PrivacyActions: Send + Sync {
    /// Trace report for a reviewer (counts + details). Personal data: reviewer-only responses.
    fn trace(&self, params: &TraceParams) -> Value;
    /// Write one suppression entry; returns its id. Errors stop the approval (fail-closed).
    fn suppress(&self, order: &SuppressOrder<'_>) -> std::result::Result<String, String>;
    /// Salted hash of an e-mail address.
    fn email_hash(&self, email: &str) -> String;
}

/// No host: tracing unavailable, approvals of removal fail, e-mails hashed unsalted.
pub struct NoActions;

impl PrivacyActions for NoActions {
    fn trace(&self, _: &TraceParams) -> Value {
        json!({ "available": false })
    }
    fn suppress(&self, _: &SuppressOrder<'_>) -> std::result::Result<String, String> {
        Err("suppression is not configured on this server".into())
    }
    fn email_hash(&self, email: &str) -> String {
        hex(&Sha256::digest(email.trim().to_lowercase().as_bytes()))
    }
}

/// Outgoing e-mail. TODO(D43): send the acknowledgement and the decision through the operator's
/// mail relay once one is configured (SMTP credentials, sender address, DPA). Until then nothing
/// leaves the server: reviewers reply by hand from the privacy mailbox.
pub trait Mailer: Send + Sync {
    fn acknowledge(&self, request: &PrivacyRequest);
}

/// Sends nothing; logs the reference only (never the address or the text).
pub struct StubMailer;

impl Mailer for StubMailer {
    fn acknowledge(&self, request: &PrivacyRequest) {
        eprintln!(
            "privacy: acknowledgement for {} not sent (mail stub, TODO)",
            request.reference
        );
    }
}

pub struct PrivacyConfig {
    /// Per client (IP) submissions.
    pub per_client: Limit,
    /// Per e-mail address in 24 h.
    pub per_email_per_day: u32,
    /// Whole server in 24 h (abuse brake; beyond it the form says "try again later").
    pub global_per_day: u32,
    pub retention: Retention,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            per_client: Limit {
                max: 5,
                window: Duration::from_secs(3600),
            },
            per_email_per_day: 3,
            global_per_day: 500,
            retention: Retention::default(),
        }
    }
}

pub struct Privacy {
    pub(crate) contrib: ContribConfig,
    pub(crate) config: PrivacyConfig,
    store: PrivacyStore,
    pub(crate) limiter: RateLimiter,
    pub(crate) users: Option<UserResolver>,
    actions: Arc<dyn PrivacyActions>,
    mailer: Arc<dyn Mailer>,
}

pub type PrivacyState = Arc<Privacy>;

fn rfc(t: SystemTime) -> String {
    humantime::format_rfc3339_seconds(t).to_string()
}

fn new_reference() -> String {
    let mut buf = [0u8; 10];
    getrandom::fill(&mut buf).expect("operating system random generator failed");
    format!("pr_{}", hex(&buf))
}

fn valid_email(email: &str) -> bool {
    !email.contains(char::is_whitespace)
        && email.split_once('@').is_some_and(|(a, b)| {
            !a.is_empty() && b.contains('.') && !b.contains('@') && b.split('.').all(|p| !p.is_empty())
        })
}

/// Decision body of `POST /api/privacy/review/{reference}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    /// `verifying` | `approve` | `reject`.
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// Trace parameters confirmed by the reviewer (default: derived from the request).
    #[serde(default)]
    pub trace: Option<TraceParams>,
    /// Extra node ids to suppress.
    #[serde(default)]
    pub nodes: Vec<String>,
    /// For `correct`: also suppress while the correction is made.
    #[serde(default)]
    pub suppress: bool,
}

impl Privacy {
    /// Reuses the contribution reviewers and database (`ContribConfig`).
    pub fn new(contrib: ContribConfig, config: PrivacyConfig) -> Result<Self> {
        let store = PrivacyStore::open(&contrib.database)?;
        Ok(Self {
            limiter: RateLimiter::new(config.per_client),
            contrib,
            config,
            store,
            users: None,
            actions: Arc::new(NoActions),
            mailer: Arc::new(StubMailer),
        })
    }

    pub fn with_actions(mut self, actions: Arc<dyn PrivacyActions>) -> Self {
        self.actions = actions;
        self
    }

    pub fn with_mailer(mut self, mailer: Arc<dyn Mailer>) -> Self {
        self.mailer = mailer;
        self
    }

    pub fn with_user_resolver(mut self, users: UserResolver) -> Self {
        self.users = Some(users);
        self
    }

    pub fn into_state(self) -> PrivacyState {
        Arc::new(self)
    }

    pub fn store(&self) -> &PrivacyStore {
        &self.store
    }

    /// Validate and store a request. `Ok(None)` = honeypot hit (answered like a success).
    pub fn submit(&self, input: PrivacyInput) -> Result<Option<PrivacyRequest>> {
        if !input.website.trim().is_empty() {
            return Ok(None);
        }
        let email = clean_req(input.email, "email", 254)?.to_lowercase();
        if !valid_email(&email) {
            return Err(ContribError::invalid("email must be an e-mail address"));
        }
        let concerns = clean_req(input.concerns, "concerns", 2000)?;
        let now = SystemTime::now();
        let day_ago = rfc(now - Duration::from_secs(86_400));
        if self.store.count_all_since(&day_ago)? >= self.config.global_per_day {
            return Err(ContribError::RateLimited { retry_after_secs: 3600 });
        }
        let email_hash = self.actions.email_hash(&email);
        if self.store.count_since(&email_hash, &day_ago)? >= self.config.per_email_per_day {
            return Err(ContribError::RateLimited {
                retry_after_secs: 86_400,
            });
        }
        let _ = self.store.purge(self.config.retention, now)?;
        let r = PrivacyRequest {
            reference: new_reference(),
            kind: input.kind,
            state: RequestState::Received,
            email: Some(email),
            concerns: Some(concerns),
            email_hash,
            created_at: rfc(now),
            respond_by: rfc(now + Duration::from_secs(30 * 86_400)),
            decided_at: None,
            reviewer: None,
            decision_reason: None,
            suppression_entry: None,
            redacted_at: None,
            delete_after: None,
        };
        self.store.insert(&r)?;
        self.mailer.acknowledge(&r);
        Ok(Some(r))
    }

    pub fn queue(&self, states: &[RequestState], limit: u32) -> Result<Vec<PrivacyRequest>> {
        let _ = self.store.purge(self.config.retention, SystemTime::now())?;
        self.store.list(states, limit)
    }

    fn params_of(r: &PrivacyRequest) -> TraceParams {
        TraceParams::from_concerns(r.concerns.as_deref().unwrap_or(""), r.email.as_deref())
    }

    /// The request with its trace (reviewer view).
    pub fn detail(&self, reference: &str, params: Option<TraceParams>) -> Result<Value> {
        let r = self.store.get(reference)?;
        let p = params.unwrap_or_else(|| Self::params_of(&r));
        let trace = if r.redacted_at.is_some() {
            json!({ "available": false, "why": "request record redacted after retention" })
        } else {
            self.actions.trace(&p)
        };
        Ok(json!({ "request": r, "trace_params": p, "trace": trace }))
    }

    pub fn decide(&self, reference: &str, input: DecisionInput, agent: &Agent) -> Result<PrivacyRequest> {
        let r = self.store.get(reference)?;
        if matches!(r.state, RequestState::Approved | RequestState::Rejected) {
            return Err(ContribError::Forbidden(format!(
                "the request is already {}",
                r.state.as_str()
            )));
        }
        let reason = input
            .reason
            .map(|s| s.trim().chars().take(500).collect::<String>())
            .filter(|s| !s.is_empty());
        let (state, entry) = match input.decision.as_str() {
            "verifying" => (RequestState::Verifying, None),
            "reject" => {
                if reason.is_none() {
                    return Err(ContribError::invalid(
                        "a rejection needs a reason (it is sent to the requester)",
                    ));
                }
                (RequestState::Rejected, None)
            }
            "approve" => {
                let suppress = matches!(r.kind, RequestType::Remove | RequestType::Object) || input.suppress;
                let entry = if suppress {
                    let p = input.trace.unwrap_or_else(|| Self::params_of(&r));
                    let order = SuppressOrder {
                        reference,
                        kind: r.kind,
                        trace: &p,
                        nodes: &input.nodes,
                        reviewer: &agent.id,
                    };
                    Some(self.actions.suppress(&order).map_err(ContribError::Blocked)?)
                } else {
                    None
                };
                (RequestState::Approved, entry)
            }
            other => return Err(ContribError::invalid(format!("unknown decision {other}"))),
        };
        self.store
            .decide(reference, state, &agent.id, reason.as_deref(), entry.as_deref())?;
        self.store.get(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concerns_to_trace_params() {
        let p = TraceParams::from_concerns("https://orcid.org/0000-0002-1825-009x", None);
        assert_eq!(p.orcid.as_deref(), Some("0000-0002-1825-009X"));
        let p = TraceParams::from_concerns("https://atlas.example/person/person%3Aa-b", Some("a@b.org"));
        assert_eq!(p.node.as_deref(), Some("person:a-b"));
        assert_eq!(p.email.as_deref(), Some("a@b.org"));
        let p = TraceParams::from_concerns("Placeholder Name", None);
        assert_eq!(p.name.as_deref(), Some("Placeholder Name"));
        let p = TraceParams::from_concerns("my entry from 2024 lists a wrong lab", None);
        assert!(p.name.is_none() && p.orcid.is_none());
    }

    #[test]
    fn retention_erases_personal_fields_then_deletes() {
        let p = Privacy::new(ContribConfig::for_tests(), PrivacyConfig::default()).unwrap();
        let r = p
            .submit(PrivacyInput {
                email: "Someone@Example.org".into(),
                concerns: "Placeholder Name".into(),
                kind: RequestType::Correct,
                website: String::new(),
            })
            .unwrap()
            .unwrap();
        let agent = Agent::person("agent:reviewer/tester".to_string(), None::<String>);
        let input = DecisionInput {
            decision: "approve".into(),
            reason: Some("corrected affiliation".into()),
            trace: None,
            nodes: vec![],
            suppress: false,
        };
        p.decide(&r.reference, input, &agent).unwrap();
        let now = SystemTime::now();
        assert_eq!(
            p.store().purge(Retention::default(), now).unwrap(),
            (0, 0),
            "inside the reply window"
        );
        p.store()
            .backdate_decision(&r.reference, "2026-01-01T00:00:00Z")
            .unwrap();
        let later = humantime::parse_rfc3339("2026-03-01T00:00:00Z").unwrap();
        assert_eq!(p.store().purge(Retention::default(), later).unwrap(), (1, 0));
        let kept = p.store().get(&r.reference).unwrap();
        assert!(
            kept.email.is_none() && kept.concerns.is_none(),
            "personal fields erased"
        );
        assert_eq!(kept.state, RequestState::Approved);
        assert_eq!(kept.reviewer.as_deref(), Some("agent:reviewer/tester"));
        let much_later = humantime::parse_rfc3339("2029-06-01T00:00:00Z").unwrap();
        assert_eq!(p.store().purge(Retention::default(), much_later).unwrap(), (0, 1));
        assert!(p.store().get(&r.reference).is_err());
    }
}
