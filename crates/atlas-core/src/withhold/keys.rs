//! Salted identifier keys and withholding result types.
use crate::graph::Person;
use crate::text::normalize_label;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Suppression list, relative to the data dir.
pub const SUPPRESSION_FILE: &str = "suppression.json";
/// Quarantine list, relative to the data dir.
pub const QUARANTINE_FILE: &str = "cache/quarantine.json";
/// Salt used when `ATLAS_SUPPRESSION_SALT` is unset. Public, so only for development: production
/// sets its own salt (a dictionary attack on ORCIDs is cheap with a known salt).
pub const DEV_SALT: &str = "zebratlas-dev-suppression-salt-v1";
/// Environment variable with the production salt.
pub const SALT_ENV: &str = "ATLAS_SUPPRESSION_SALT";
pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Quarantine,
    Suppression,
}

/// Why an item is withheld.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Withheld<'a> {
    pub list: ListKind,
    pub reason: &'a str,
    /// Entry id (`sup_…`, a quarantine entry id) or `closed`.
    pub entry: &'a str,
}

/// Identifier kinds that can be hashed into suppression keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// `0000-0002-1825-0097` (URL prefixes and case are normalised).
    Orcid,
    /// Normalised full name + `|` + normalised affiliation (affiliation may be empty).
    NameAff,
    /// Lower-cased e-mail address.
    Email,
    /// A graph node id (`ORCID:…`, `person:…`).
    Node,
}

impl KeyKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Orcid => "orcid",
            Self::NameAff => "name_aff",
            Self::Email => "email",
            Self::Node => "node",
        }
    }
}

/// `0000-0002-1825-0097` from an ORCID in any common spelling, or `None`.
pub fn normalise_orcid(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s
        .rsplit('/')
        .next()
        .unwrap_or(s)
        .trim_start_matches("ORCID:")
        .trim_start_matches("orcid:");
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let ok = digits.len() == 16
        && digits[..15].bytes().all(|b| b.is_ascii_digit())
        && (digits.as_bytes()[15].is_ascii_digit() || digits.as_bytes()[15] == b'X');
    ok.then(|| {
        format!(
            "{}-{}-{}-{}",
            &digits[0..4],
            &digits[4..8],
            &digits[8..12],
            &digits[12..16]
        )
    })
}

/// Normalised value for a key, or `None` when the input cannot identify anyone.
fn normalise(kind: KeyKind, value: &str) -> Option<String> {
    let v = match kind {
        KeyKind::Orcid => normalise_orcid(value)?,
        KeyKind::Email => {
            let e = value.trim().to_lowercase();
            (e.contains('@') && e.len() > 3).then_some(e)?
        }
        KeyKind::Node => value.trim().to_owned(),
        KeyKind::NameAff => {
            let (name, aff) = value.split_once('|').unwrap_or((value, ""));
            let name = normalize_label(name);
            (!name.is_empty()).then(|| format!("{name}|{}", normalize_label(aff)))?
        }
    };
    (!v.is_empty()).then_some(v)
}

/// The secret that makes suppression keys unguessable without it.
#[derive(Clone)]
pub struct Salt {
    bytes: Vec<u8>,
    id: String,
    dev: bool,
}

impl fmt::Debug for Salt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Salt({}, dev: {})", self.id, self.dev)
    }
}

impl Salt {
    pub fn new(secret: &str) -> Self {
        let id: String = hex(&Sha256::digest(secret.as_bytes()))[..8].to_owned();
        Self {
            bytes: secret.as_bytes().to_vec(),
            dev: secret == DEV_SALT,
            id,
        }
    }

    /// From an optional configured value (e.g. the environment); empty or missing = [`DEV_SALT`].
    pub fn from_config(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => Self::new(v),
            None => Self::new(DEV_SALT),
        }
    }

    /// First 8 hex chars of `sha256(salt)`: recorded with each entry, never the salt itself.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_dev(&self) -> bool {
        self.dev
    }

    /// `hex(sha256(salt ‖ 0x1f ‖ kind ‖ ":" ‖ normalised value))`, or `None` for an unusable value.
    pub fn key(&self, kind: KeyKind, value: &str) -> Option<String> {
        let v = normalise(kind, value)?;
        let mut h = Sha256::new();
        h.update(&self.bytes);
        h.update([0x1f]);
        h.update(kind.tag().as_bytes());
        h.update(b":");
        h.update(v.as_bytes());
        Some(hex(&h.finalize()))
    }

    /// Every key of a graph person: node id, ORCIDs, (name or variant) × (affiliation or none).
    pub fn person_keys(&self, p: &Person) -> Vec<String> {
        let mut out = Vec::new();
        out.extend(self.key(KeyKind::Node, &p.id));
        if let Some(o) = p.id.strip_prefix("ORCID:") {
            out.extend(self.key(KeyKind::Orcid, o));
        }
        for o in &p.orcids {
            out.extend(self.key(KeyKind::Orcid, o));
        }
        for name in std::iter::once(&p.name).chain(&p.name_variants) {
            out.extend(self.key(KeyKind::NameAff, &format!("{name}|")));
            for aff in &p.affiliations {
                out.extend(self.key(KeyKind::NameAff, &format!("{name}|{aff}")));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
