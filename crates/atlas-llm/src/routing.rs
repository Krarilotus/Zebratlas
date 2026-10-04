//! Hosted provider policy. Explicit BYO/local connections never enter this chain.
use crate::{LlmError, QuotaReason};

pub const GEMINI: &str = "gemini-free";
pub const GEMINI_LITE: &str = "gemini-lite-free";
pub const OPENAI: &str = "openai-hosted";
pub const DEFAULT_ORDER: &[&str] = &[crate::HOSTED_FREE, "kisski"];

pub fn is_hosted(name: &str) -> bool {
    matches!(name, GEMINI | GEMINI_LITE | OPENAI) || DEFAULT_ORDER.contains(&name)
}

pub fn fallback_order(first: &str) -> Vec<&str> {
    std::iter::once(first)
        .chain(DEFAULT_ORDER.iter().copied().filter(|name| *name != first))
        .collect()
}

/// Caller limits, spend caps, kill switches and invalid requests are terminal.
pub fn can_fallback(error: &LlmError) -> bool {
    matches!(
        error,
        LlmError::MissingKey { .. }
            | LlmError::Unavailable(_)
            | LlmError::Auth(_)
            | LlmError::RateLimited(_)
            | LlmError::Timeout(_)
            | LlmError::BadOutput(_)
            | LlmError::Provider {
                status: Some(404 | 408 | 429 | 500..=599),
                ..
            }
            | LlmError::FreeQuota {
                reason: QuotaReason::NotConfigured,
                ..
            }
    )
}
