//! The contribution service: submit, auto-check, review, overlay, PROV. Handlers in
//! [`crate::routes`] are thin wrappers around these methods.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::auth::{RateLimiter, UserRef, UserResolver};
use crate::checks;
use crate::config::ContribConfig;
use crate::error::{ContribError, Result};
use crate::fetch::{Fetcher, HttpFetcher};
use crate::graph::GraphLookup;
use crate::model::{Agent, CheckStatus, Contribution, Contributor, Decision, Review, ReviewInput, State, Submission};
use crate::overlay::Overlay;
use crate::prov::{self, ProvEvent};
use crate::state::{self, Event};
use crate::store::{ListFilter, Store};
use crate::util::{new_id, now_rfc3339};

/// Shared state of the router.
pub type ContribState = Arc<Contrib>;

pub struct Contrib {
    pub(crate) config: ContribConfig,
    store: Store,
    lookup: Arc<dyn GraphLookup>,
    fetcher: Arc<dyn Fetcher>,
    pub(crate) users: Option<UserResolver>,
    pub(crate) limiter: RateLimiter,
    overlay_lock: Mutex<()>,
}

impl Contrib {
    pub fn new(config: ContribConfig, lookup: Arc<dyn GraphLookup>) -> Result<Self> {
        let store = Store::open(&config.database)?;
        Ok(Self {
            fetcher: Arc::new(HttpFetcher::new(config.fetch)),
            limiter: RateLimiter::new(config.submit_limit),
            config,
            store,
            lookup,
            users: None,
            overlay_lock: Mutex::new(()),
        })
    }

    pub fn with_fetcher(mut self, fetcher: Arc<dyn Fetcher>) -> Self {
        self.fetcher = fetcher;
        self
    }

    /// Lets signed-in users contribute under their account (and listed accounts review).
    pub fn with_user_resolver(mut self, users: UserResolver) -> Self {
        self.users = Some(users);
        self
    }

    pub fn into_state(self) -> ContribState {
        Arc::new(self)
    }

    pub fn config(&self) -> &ContribConfig {
        &self.config
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Account owner hook: all private submissions, checks, reviews and PROV history as JSON.
    /// Authenticate `user_id` in the accounts service; never expose this as a public route.
    pub fn export_for_user(&self, user_id: &str) -> Result<Value> {
        self.store.export_for_user(user_id)
    }

    /// Account deletion hook. Idempotent; keeps accepted graph data and optional accepted credit.
    /// The transaction removes account links, contacts and historical personal attribution.
    /// Public files are rebuilt even on retry after a failed export write.
    pub fn anonymise_for_user(&self, user_id: &str, keep_credit: bool) -> Result<usize> {
        let count = self.store.anonymise_for_user(user_id, keep_credit)?;
        self.write_overlay()?;
        self.write_discovery_candidates()?;
        Ok(count)
    }

    /// Stores a new contribution in `submitted` and records the submission activity.
    pub fn submit(&self, submission: Submission, user: Option<UserRef>) -> Result<Contribution> {
        let mut submission = submission;
        if let Some(email) = user.as_ref().and_then(|u| u.email.clone()) {
            submission.contributor.contact = Some(email);
        }
        let mut sub = submission.clean()?;
        let input = std::mem::take(&mut sub.contributor);
        let now = now_rfc3339();
        let c = Contribution {
            id: new_id(),
            kind: sub.kind,
            state: State::Submitted,
            version: 1,
            contributor: Contributor {
                user_id: user.as_ref().map(|u| u.id.clone()),
                name: input.name.or_else(|| user.as_ref().and_then(|u| u.name.clone())),
                organisation: input.organisation,
                contact: input.contact,
            },
            submission: sub,
            created_at: now.clone(),
            updated_at: now.clone(),
            checks: None,
            review: None,
        };
        let mut event = ProvEvent::new("submission", c.contributor_agent(), None, &c, now);
        if let Some(url) = &c.submission.evidence_url {
            event = event.using(prov::page_entity(url, None, None, None));
        }
        if let Some(source) = &c.submission.data_source
            && !source.url.is_empty()
        {
            event = event.using(prov::page_entity(&source.url, None, None, None));
        }
        if let Some(page) = &c.submission.found_via.page {
            event = event.note(format!("started from {page}"));
        }
        if let Some(assistant) = &c.submission.found_via.assistant {
            event = event.delegate(Agent {
                id: format!(
                    "agent:assistant/{}",
                    crate::text::normalize(assistant).replace(' ', "-")
                ),
                kind: "prov:SoftwareAgent".into(),
                label: Some(format!(
                    "{assistant} (AI assistant that found the lead, as reported by the contributor)"
                )),
            });
        }
        self.store.insert(&c, &event)?;
        Ok(c)
    }

    pub fn get(&self, id: &str) -> Result<Contribution> {
        self.store.get(id)?.ok_or(ContribError::NotFound)
    }

    pub fn list(&self, filter: &ListFilter) -> Result<Vec<Contribution>> {
        self.store.list(filter)
    }

    /// Runs the auto-checks and moves the contribution to `auto_checked`.
    pub async fn run_checks(&self, id: &str) -> Result<Contribution> {
        // An IP-distributed submission flood must not spawn unbounded outbound fetches.
        // The stored submission stays pending and can be rechecked by a reviewer.
        let _permit = self.config.check_work.acquire()?;
        let before = self.get(id)?;
        // Fail fast on final states, before spending time on fetches.
        let dummy = Default::default();
        state::next(before.state, Event::Checked(&dummy))?;
        let started = now_rfc3339();
        let others = self.store.list(&ListFilter {
            states: vec![State::Submitted, State::AutoChecked, State::Accepted],
            ..Default::default()
        })?;
        let report = checks::run(
            &before.id,
            &before.submission,
            self.lookup.as_ref(),
            self.fetcher.as_ref(),
            &others,
        )
        .await;
        let to = state::next(before.state, Event::Checked(&report))?;
        let mut after = before.clone();
        after.state = to;
        after.version += 1;
        after.updated_at = now_rfc3339();
        let summary = summarize(&report);
        let used: Vec<Value> = report
            .fetched
            .iter()
            .map(|f| {
                prov::page_entity(
                    f.final_url.as_deref().unwrap_or(&f.url),
                    f.sha256.as_deref(),
                    Some(&f.retrieved_at),
                    f.status,
                )
            })
            .collect();
        after.checks = Some(report);
        let mut event = ProvEvent::new("auto_check", Agent::software(), Some(&before), &after, started).note(summary);
        for u in used {
            event = event.using(u);
        }
        if let Some(snapshots) = after.checks.as_ref().and_then(|r| r.find("source_snapshots"))
            && let Some(records) = snapshots.detail["records"].as_array()
        {
            for record in records {
                let mut entity = prov::page_entity(
                    record["url"].as_str().unwrap_or_default(),
                    record["sha256"].as_str(),
                    record["retrieved_at"].as_str(),
                    None,
                );
                entity["record_locator"] = record["record_locator"].clone();
                entity["snapshot"] = record["snapshot"].clone();
                event = event.using(entity);
            }
        }
        if let Some(known) = after.checks.as_ref().and_then(|r| r.find("known_source"))
            && let Some(inputs) = known.detail["inputs"].as_array()
        {
            for input in inputs {
                let mut entity = prov::page_entity(
                    input["url"].as_str().unwrap_or_default(),
                    input["sha256"].as_str(),
                    input["retrieved_at"].as_str(),
                    None,
                );
                entity["version"] = input["version"].clone();
                entity["record_locator"] = input["record_locator"].clone();
                event = event.using(entity);
            }
        }
        self.store.update(&after, before.version, &event)?;
        Ok(after)
    }

    /// Accepts or rejects, with a reason; accepting rewrites the overlay file.
    pub fn review(&self, id: &str, input: &ReviewInput, reviewer: Agent) -> Result<Contribution> {
        let before = self.get(id)?;
        let reason = input.reason.trim();
        let event = match input.decision {
            Decision::Accept => Event::Accept { reason },
            Decision::Reject => Event::Reject { reason },
        };
        let to = state::next(before.state, event)?;
        if input.decision == Decision::Accept {
            state::check_accept(before.checks.as_ref())?;
        }
        let started = now_rfc3339();
        let mut after = before.clone();
        after.state = to;
        after.version += 1;
        after.updated_at = now_rfc3339();
        after.review = Some(Review {
            decision: input.decision,
            reason: reason.to_string(),
            reviewer: reviewer.clone(),
            at: after.updated_at.clone(),
        });
        let note = format!(
            "{}: {reason}",
            match input.decision {
                Decision::Accept => "accepted",
                Decision::Reject => "rejected",
            }
        );
        let ev = ProvEvent::new("review", reviewer, Some(&before), &after, started).note(note);
        self.store.update(&after, before.version, &ev)?;
        if after.state == State::Accepted {
            if after.kind == crate::model::ContributionKind::DataSource {
                self.write_discovery_candidates()?;
            } else {
                self.write_overlay()?;
            }
        }
        Ok(after)
    }

    pub fn overlay(&self) -> Result<Overlay> {
        let accepted = self.store.list(&ListFilter {
            states: vec![State::Accepted],
            ..Default::default()
        })?;
        Ok(Overlay::build(&accepted))
    }

    /// Rewrites the overlay file, if one is configured.
    pub fn write_overlay(&self) -> Result<()> {
        let Some(path) = &self.config.overlay_path else {
            return Ok(());
        };
        let _guard = self.overlay_lock.lock().unwrap_or_else(|p| p.into_inner());
        self.overlay()?.write(path)
    }

    pub fn prov_events(&self, id: &str) -> Result<Vec<ProvEvent>> {
        self.store.prov(id)
    }

    pub fn discovery_candidates(&self) -> Result<Value> {
        let accepted: Vec<_> = self
            .store
            .list(&ListFilter {
                states: vec![State::Accepted],
                ..Default::default()
            })?
            .into_iter()
            .filter(|c| c.kind == crate::model::ContributionKind::DataSource)
            .collect();
        let histories = accepted
            .iter()
            .map(|c| self.prov_jsonld(&c.id))
            .collect::<Result<Vec<_>>>()?;
        Ok(crate::datasource::export(&accepted, &histories))
    }

    /// Rebuild from committed SQLite state, including after a failed file write.
    pub fn write_discovery_candidates(&self) -> Result<()> {
        let Some(path) = &self.config.discovery_candidates_path else {
            return Ok(());
        };
        let _guard = self.overlay_lock.lock().unwrap_or_else(|p| p.into_inner());
        crate::datasource::write(&self.discovery_candidates()?, path)
    }

    /// PROV-O JSON-LD of one contribution's history.
    pub fn prov_jsonld(&self, id: &str) -> Result<Value> {
        let c = self.get(id)?;
        let events = self.store.prov(id)?;
        Ok(prov::jsonld(&c, &events))
    }

    pub fn counts(&self) -> Result<Value> {
        let mut out = json!({ "submitted": 0, "auto_checked": 0, "accepted": 0, "rejected": 0 });
        for (s, n) in self.store.counts()? {
            out[s.as_str()] = json!(n);
        }
        Ok(out)
    }
}

fn summarize(report: &crate::model::CheckReport) -> String {
    let count = |s: CheckStatus| report.checks.iter().filter(|c| c.status == s).count();
    format!(
        "{} passed, {} warnings, {} failed, {} skipped",
        count(CheckStatus::Pass),
        count(CheckStatus::Warn),
        count(CheckStatus::Fail),
        count(CheckStatus::Skipped)
    )
}
