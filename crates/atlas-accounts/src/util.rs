//! Small helpers: random ids, clock, RFC 3339 rendering.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Fills `buf` from the operating system's CSPRNG.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    // The OS generator failing is unrecoverable; refusing to continue is the only safe option.
    getrandom::fill(&mut buf).expect("operating system random generator failed");
    buf
}

/// `prefix_` plus 128 random bits in hex, e.g. `usr_3f9a...`.
pub fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = random_bytes();
    let mut s = String::with_capacity(prefix.len() + 1 + 32);
    s.push_str(prefix);
    s.push('_');
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Seconds since the Unix epoch.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Unix seconds as RFC 3339 (`2026-10-03T21:00:00Z`).
pub fn rfc3339(secs: i64) -> String {
    let t = UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64);
    humantime::format_rfc3339_seconds(t).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = new_id("usr");
        let b = new_id("usr");
        assert!(a.starts_with("usr_") && a.len() == 36);
        assert_ne!(a, b);
    }

    #[test]
    fn renders_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    }
}
