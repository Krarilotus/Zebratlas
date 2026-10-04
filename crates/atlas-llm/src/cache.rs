//! Content-addressed response cache: `<dir>/<sha256>.json`.
//!
//! Key = sha256 over a canonical JSON of provider kind, base URL, model, settings, messages and
//! schema (never the key, the connection's display name or the deadline). A hit replays the
//! stored output byte for byte, so demos and re-runs are deterministic and work offline.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{LlmError, Result};
use crate::provider::ProviderKind;
use crate::request::{CompletionRequest, ProviderOutput};

pub const DEFAULT_DIR: &str = "data/cache/llm";
const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    /// Replay hits, store misses (default).
    ReadWrite,
    /// Replay hits; a miss is an error (offline demo).
    ReplayOnly,
    /// Always call; store nothing.
    Off,
}

impl CacheMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read-write" | "rw" | "on" => Some(Self::ReadWrite),
            "replay-only" | "replay" | "offline" => Some(Self::ReplayOnly),
            "off" | "none" => Some(Self::Off),
            _ => None,
        }
    }
}

/// One stored call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub format: u32,
    pub key: String,
    pub provider: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
    pub request: CompletionRequest,
    pub output: ProviderOutput,
    pub latency_ms: u64,
    pub started_at: String,
    pub ended_at: String,
}

#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
    mode: CacheMode,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>, mode: CacheMode) -> Self {
        Self { dir: dir.into(), mode }
    }

    pub fn mode(&self) -> CacheMode {
        self.mode
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Result<Option<CacheEntry>> {
        if self.mode == CacheMode::Off {
            return Ok(None);
        }
        match std::fs::read(self.path(key)) {
            Ok(bytes) => {
                let e: CacheEntry = serde_json::from_slice(&bytes)?;
                Ok((e.format == FORMAT_VERSION && e.key == key).then_some(e))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic write (temp file + rename), so concurrent readers never see half a file.
    pub fn put(&self, entry: &CacheEntry) -> Result<()> {
        if self.mode != CacheMode::ReadWrite {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{}.{}.tmp", entry.key, std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(entry)?)?;
        std::fs::rename(&tmp, self.path(&entry.key)).map_err(LlmError::from)
    }
}

/// Canonical serialisation: sorted keys, compact, built by hand so the hash does not depend on
/// map insertion order (serde_json may be compiled with `preserve_order`).
fn to_canonical_string(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<_> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), to_canonical_string(&m[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(to_canonical_string).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The content key of a call.
pub fn cache_key(kind: ProviderKind, base_url: Option<&str>, model: &str, req: &CompletionRequest) -> String {
    let v = json!({
        "v": FORMAT_VERSION,
        "provider": kind.as_str(),
        "base_url": base_url,
        "model": model,
        "settings": req.settings,
        "messages": req.messages,
        "schema": req.schema,
    });
    sha256_hex(to_canonical_string(&v).as_bytes())
}

/// Hash of the prompt alone (messages + schema): `prov:used` entity id.
pub fn prompt_hash(req: &CompletionRequest) -> String {
    let v = json!({"messages": req.messages, "schema": req.schema});
    sha256_hex(to_canonical_string(&v).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Message;

    #[test]
    fn key_is_stable_and_sensitive() {
        let r = CompletionRequest::new(vec![Message::user("hi")]);
        let a = cache_key(ProviderKind::OpenAi, None, "m", &r);
        assert_eq!(
            a,
            cache_key(
                ProviderKind::OpenAi,
                None,
                "m",
                &r.clone().with_deadline(std::time::Duration::from_secs(1))
            )
        );
        assert_ne!(a, cache_key(ProviderKind::OpenAi, None, "m2", &r));
        assert_ne!(
            a,
            cache_key(ProviderKind::OpenAi, None, "m", &r.clone().with_temperature(0.0))
        );
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn canonical_ignores_key_order() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":2,"x":3}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":3,"y":2},"b":1}"#).unwrap();
        assert_eq!(to_canonical_string(&a), to_canonical_string(&b));
    }
}
