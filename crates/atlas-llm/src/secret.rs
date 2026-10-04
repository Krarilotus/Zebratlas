//! API keys: never logged, never serialised, never part of a cache key or provenance record.

use std::fmt;

/// A bring-your-own key (from the UI, per request) or a server-side key read from env.
///
/// `Debug`/`Display` print `ApiKey(***)`; there is no `Serialize` impl on purpose.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Read from an environment variable; empty values count as absent.
    pub fn from_env(var: &str) -> Option<Self> {
        std::env::var(var).ok().filter(|v| !v.trim().is_empty()).map(Self)
    }

    /// The raw secret, only for building the provider request.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_redacted() {
        let k = ApiKey::new("sk-very-secret");
        assert_eq!(format!("{k:?}"), "ApiKey(***)");
        assert_eq!(format!("{k}"), "***");
        assert!(!format!("{:?}", Some(&k)).contains("secret"));
    }
}
