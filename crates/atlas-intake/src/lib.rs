//! Document intake (D41): a pasted or uploaded document (PDF, DOCX, TXT) becomes search terms.
//!
//! In memory only: [`extract`] reads the text, [`redact`] removes identifiers (names, dates of birth,
//! addresses, phone, e-mail, record / insurance numbers, IBAN) **before any model sees it**, and
//! [`terms`] asks the connection's model (through `atlas-llm`, no disk cache; D48: the default
//! free-tier connection or the user's own key) for genes, variants,
//! conditions and clinical features, each located in the redacted text. Terms the model returns
//! that are not in the document are dropped; validation against the graph is the caller's job
//! (atlas-server owns the resolver). Nothing here writes files or logs document content.

pub mod extract;
pub mod redact;
pub mod terms;

use std::time::Instant;

use sha2::{Digest, Sha256};

pub use extract::{Extracted, Format};
pub use redact::{Kind as RedactionKind, Redacted};
pub use terms::{Located, RawTerm, TermKind};

/// Upload limits (D41).
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_pages: usize,
    /// Characters of redacted text sent to the model (the rest is cut, `truncated: true`).
    pub max_model_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 5 * 1024 * 1024,
            max_pages: 30,
            max_model_chars: 40_000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    #[error("the document is larger than 5 MB")]
    TooLarge,
    #[error("the document has {0} pages; up to 30 are read")]
    TooManyPages(usize),
    #[error("this file type is not supported (PDF, DOCX or text; no images)")]
    Unsupported,
    #[error("the file could not be read")]
    Unreadable,
    #[error("no text found (a scanned PDF without a text layer?)")]
    NoText,
    #[error("the document is empty")]
    Empty,
}

impl IntakeError {
    /// Message key for the web catalogs (D26).
    pub fn key(&self) -> &'static str {
        match self {
            Self::TooLarge => "intake.error.too_large",
            Self::TooManyPages(_) => "intake.error.too_many_pages",
            Self::Unsupported => "intake.error.unsupported",
            Self::Unreadable => "intake.error.unreadable",
            Self::NoText => "intake.error.no_text",
            Self::Empty => "intake.error.empty",
        }
    }

    /// HTTP status the server answers with.
    pub fn status(&self) -> u16 {
        match self {
            Self::TooLarge => 413,
            Self::Unsupported => 415,
            _ => 422,
        }
    }
}

/// The source the user gave.
pub enum Input<'a> {
    File(&'a [u8]),
    Paste(&'a str),
}

/// A document after extraction and redaction: everything the model step and the response need.
pub struct Prepared {
    pub sha256: String,
    pub bytes: usize,
    pub format: Format,
    pub pages: Option<usize>,
    /// Characters of the extracted text.
    pub chars: usize,
    pub redacted: Redacted,
    /// The redacted text, cut to the model limit at a line break.
    pub sent: String,
    pub truncated: bool,
    pub extract_ms: u64,
    pub redact_ms: u64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn ms(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Extract and redact. CPU-bound (PDF parsing): run it on a blocking thread.
pub fn prepare(input: Input<'_>, limits: &Limits) -> Result<Prepared, IntakeError> {
    let t = Instant::now();
    let (raw, e) = match input {
        Input::File(b) => (b, extract::extract(b, limits)?),
        Input::Paste(s) => (s.as_bytes(), extract::pasted(s, limits)?),
    };
    let extract_ms = ms(t);
    let t = Instant::now();
    let redacted = redact::redact(&e.text);
    let redact_ms = ms(t);
    let (sent, truncated) = cut(&redacted.text, limits.max_model_chars);
    Ok(Prepared {
        sha256: sha256_hex(raw),
        bytes: raw.len(),
        format: e.format,
        pages: e.pages,
        chars: e.text.chars().count(),
        sent,
        truncated,
        redacted,
        extract_ms,
        redact_ms,
    })
}

/// At most `max` characters, cut at the last line break before the limit.
fn cut(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        None => (text.to_owned(), false),
        Some((at, _)) => {
            let head = &text[..at];
            let end = head.rfind('\n').filter(|&i| i > at / 2).unwrap_or(at);
            (text[..end].to_owned(), true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_paste_cuts_at_line() {
        let l = Limits {
            max_model_chars: 12,
            ..Limits::default()
        };
        let p = prepare(Input::Paste("line one\nline two\nline three"), &l).unwrap();
        assert!(p.truncated);
        assert_eq!(p.sent, "line one");
        assert_eq!(p.format, Format::Paste);
        assert_eq!(p.sha256.len(), 64);
    }
}
