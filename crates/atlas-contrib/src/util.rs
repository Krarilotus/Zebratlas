//! Small helpers: ids, clock, hex.

use std::time::SystemTime;

/// `c_` plus 96 random bits in hex: short enough for a URL, unguessable.
pub fn new_id() -> String {
    let mut buf = [0u8; 12];
    // The OS generator failing is unrecoverable; refusing to continue is the only safe option.
    getrandom::fill(&mut buf).expect("operating system random generator failed");
    format!("c_{}", hex(&buf))
}

/// Current time as RFC 3339 with seconds (`2026-10-03T21:00:00Z`).
pub fn now_rfc3339() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
