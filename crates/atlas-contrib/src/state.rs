//! The review state machine, as a pure function so every rule is testable on its own.
//!
//! ```text
//! submitted ──checked──▶ auto_checked ──checked──▶ auto_checked   (re-check)
//!     │                      ├──accept──▶ accepted   (no failed blocking check; reason required)
//!     └──reject──▶ rejected ◀┴──reject──             (reason required; spam may skip the checks)
//! ```
//! Accepted and rejected are final: a changed mind is a new contribution, so history is never
//! rewritten.

use crate::error::{ContribError, Result};
use crate::model::{CheckReport, MAX_REASON_CHARS, State};

/// What happens to a contribution.
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    Checked(&'a CheckReport),
    Accept { reason: &'a str },
    Reject { reason: &'a str },
}

impl Event<'_> {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Checked(_) => "check",
            Self::Accept { .. } => "accept",
            Self::Reject { .. } => "reject",
        }
    }
}

/// The next state, or why the event is not allowed in `from`.
pub fn next(from: State, event: Event<'_>) -> Result<State> {
    let not_allowed = || ContribError::Transition {
        from,
        action: event.action(),
    };
    match (from, event) {
        (State::Submitted | State::AutoChecked, Event::Checked(_)) => Ok(State::AutoChecked),
        (State::AutoChecked, Event::Accept { reason }) => {
            check_reason(reason)?;
            Ok(State::Accepted)
        }
        (State::Submitted | State::AutoChecked, Event::Reject { reason }) => {
            check_reason(reason)?;
            Ok(State::Rejected)
        }
        _ => Err(not_allowed()),
    }
}

/// Accepting additionally needs a report without failed blocking checks.
pub fn check_accept(report: Option<&CheckReport>) -> Result<()> {
    let Some(report) = report else {
        return Err(ContribError::Blocked("the auto-checks have not run".into()));
    };
    let blockers = report.blockers();
    if blockers.is_empty() {
        Ok(())
    } else {
        Err(ContribError::Blocked(blockers.join("; ")))
    }
}

pub fn check_reason(reason: &str) -> Result<()> {
    let r = reason.trim();
    if r.is_empty() {
        return Err(ContribError::invalid("a reason is required"));
    }
    if r.chars().count() > MAX_REASON_CHARS {
        return Err(ContribError::invalid(format!(
            "the reason is longer than {MAX_REASON_CHARS} characters"
        )));
    }
    Ok(())
}
