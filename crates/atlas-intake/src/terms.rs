//! Search terms from redacted text: the connection's model (D48: the default free-tier connection,
//! or the caller's own key) with a strict JSON schema, then every term is
//! located in the text (a term that is not in the document is dropped, never invented). A
//! deterministic finder (HGVS variants, gene-like symbols, word n-grams) serves when no model runs.

use std::sync::LazyLock;

use atlas_llm::{Call, Completion, CompletionRequest, JsonSchema, Llm, Message};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// At most this many terms per document.
pub const MAX_TERMS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermKind {
    Gene,
    Variant,
    Condition,
    Phenotype,
}

impl TermKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "gene" => Self::Gene,
            "variant" => Self::Variant,
            "condition" | "disease" => Self::Condition,
            "phenotype" | "feature" | "symptom" => Self::Phenotype,
            _ => return None,
        })
    }
}

/// A term as the model (or the lexical finder) gives it. Empty strings mean "none".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawTerm {
    pub kind: TermKind,
    /// Verbatim from the document.
    pub text: String,
    /// Standard English name (HPO label, disease name), for documents in other languages.
    #[serde(default)]
    pub english: String,
    #[serde(default)]
    pub hgvs: String,
    #[serde(default)]
    pub gene: String,
    #[serde(default)]
    pub negated: bool,
}

#[derive(Deserialize)]
struct Envelope {
    terms: Vec<RawTerm>,
}

/// A term with its place in the redacted text. Offsets are UTF-16 code units (JavaScript strings).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Located {
    pub term: RawTerm,
    pub start: usize,
    pub end: usize,
    /// The surrounding words (≤ 160 characters), from the redacted text.
    pub quote: String,
}

pub fn schema() -> JsonSchema {
    let s = |d: &str| json!({ "type": "string", "description": d });
    JsonSchema::new(
        "intake_terms",
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["terms"],
            "properties": { "terms": { "type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "text", "english", "hgvs", "gene", "negated"],
                "properties": {
                    "kind": { "type": "string", "enum": ["gene", "variant", "condition", "phenotype"] },
                    "text": s("copied character for character from the document"),
                    "english": s("standard English name: HPO term label for a phenotype, disease name for a condition; empty if the text is already that"),
                    "hgvs": s("HGVS notation for a variant, else empty"),
                    "gene": s("gene symbol of a variant, else empty"),
                    "negated": { "type": "boolean", "description": "true when the document says the feature is absent or excluded" }
                }
            }}}
        }),
    )
}

const SYSTEM: &str = "You extract search terms from a medical document (a doctor's letter, report or paper) for a rare-disease research atlas. \
Identifiers were replaced by placeholders such as [NAME] or [DATE-OF-BIRTH]; ignore them and never guess them. \
Return every gene, genetic variant, diagnosed or suspected condition, and clinical feature (sign, symptom, finding) of the patient the document is about. \
Rules: `text` is copied exactly from the document (same characters, at most 80); one entry per distinct term; \
`english` is the standard English name (Human Phenotype Ontology label for features, disease name for conditions) or empty; \
for variants give `hgvs` (e.g. c.1162C>T or p.Arg388Ter) and `gene`; set `negated` when the document says the feature is absent or was ruled out; \
leave out family members' findings, medications, procedures and normal results. Never add a term that is not in the document. At most 40 terms.";

/// The request for one document (no prompt cache: the document is new each time). No model is
/// named: the connection's default model runs (D48).
pub fn request(redacted: &str) -> CompletionRequest {
    CompletionRequest::new(vec![
        Message::system(SYSTEM),
        Message::user(format!("Document:\n<<<\n{redacted}\n>>>")),
    ])
    .with_schema(schema())
    .with_max_tokens(2500)
}

/// Ask the model; returns the raw terms and the calls (one, or two after a schema retry).
pub async fn from_model(llm: &Llm, call: &Call, redacted: &str) -> atlas_llm::Result<(Vec<RawTerm>, Vec<Completion>)> {
    let done = llm.complete_json::<Envelope>(call, request(redacted)).await?;
    let mut terms = done.value.terms;
    terms.truncate(MAX_TERMS);
    Ok((terms, done.calls))
}

// ---------------------------------------------------------------------------------------------
// Locating terms in the text
// ---------------------------------------------------------------------------------------------

fn fold(c: char) -> char {
    if c.is_whitespace() {
        ' '
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Char index of `needle` in `hay` (case-insensitive, any whitespace matches any whitespace).
fn find_chars(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| needle.iter().enumerate().all(|(k, &c)| hay[i + k] == c))
}

/// Keep the terms that occur in `text`, with their spans; duplicates (same kind and text) once.
pub fn locate(text: &str, terms: Vec<RawTerm>) -> (Vec<Located>, usize) {
    let chars: Vec<char> = text.chars().collect();
    let folded: Vec<char> = chars.iter().map(|&c| fold(c)).collect();
    // UTF-16 offset of every char index
    let mut u16_at = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0;
    for c in &chars {
        u16_at.push(acc);
        acc += c.len_utf16();
    }
    u16_at.push(acc);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut dropped = 0;
    for t in terms {
        let needle: Vec<char> = t.text.trim().chars().map(fold).collect();
        if needle.is_empty() || !seen.insert((t.kind, needle.clone())) {
            continue;
        }
        let Some(i) = find_chars(&folded, &needle) else {
            dropped += 1;
            continue;
        };
        let j = i + needle.len();
        let (a, b) = (i.saturating_sub(60), (j + 60).min(chars.len()));
        let quote: String = chars[a..b]
            .iter()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let quote: String = quote.chars().take(160).collect();
        out.push(Located {
            term: t,
            start: u16_at[i],
            end: u16_at[j],
            quote,
        });
    }
    (out, dropped)
}

// ---------------------------------------------------------------------------------------------
// Deterministic finder
// ---------------------------------------------------------------------------------------------

static HGVS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:(?:N[MCGRP]_\d+(?:\.\d+)?)(?:\(([A-Z][A-Z0-9-]{1,14})\))?:)?(?:c\.[-*]?\d+(?:[+-]\d+)?(?:_[-*]?\d+(?:[+-]\d+)?)?(?:[ACGT]>[ACGT]|delins[ACGT]+|del[ACGT]*|dup[ACGT]*|ins[ACGT]+)|p\.\(?(?:[A-Z][a-z]{2}\d+(?:[A-Z][a-z]{2}|Ter|\*|=|fs(?:Ter|\*)?\d*)|[A-Z]\d+[A-Z*])\)?)",
    )
    .expect("hgvs")
});

static SYMBOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][A-Z0-9]{1,9}(?:-AS1)?\b").expect("symbol"));

/// Upper-case words in letters that are not gene symbols worth a lookup.
const NOT_GENES: &[&str] = &[
    "EEG",
    "MRI",
    "MRT",
    "DNA",
    "RNA",
    "ICD",
    "EKG",
    "ECG",
    "PCR",
    "NGS",
    "WES",
    "WGS",
    "OMIM",
    "HPO",
    "ACMG",
    "VUS",
    "CT",
    "IQ",
    "ID",
    "NAME",
    "DATE",
    "BIRTH",
    "ADDRESS",
    "PHONE",
    "EMAIL",
    "RECORD",
    "INSURANCE",
    "IBAN",
    "NO",
    "DOB",
    "MD",
    "PHD",
    "UK",
    "USA",
    "EU",
    "DE",
    "FR",
    "GB",
    "OF",
    "AND",
    "THE",
    "UND",
    "ET",
    "CNV",
    "SNV",
    "HGVS",
    "CSF",
    "ENT",
    "HNO",
    "KG",
    "CM",
    "MG",
    "ML",
    "AM",
    "PM",
    "II",
    "III",
    "IV",
    "VI",
    "DSM",
    "ADHD",
    "ASD",
];

/// Variants (with the gene named next to them, when any) and gene-like symbols; the caller validates.
pub fn lexical(text: &str) -> Vec<RawTerm> {
    let mut out = Vec::new();
    for c in HGVS.captures_iter(text) {
        let m = c.get(0).expect("match");
        let gene = c.get(1).map(|g| g.as_str().to_owned()).or_else(|| {
            // "STXBP1 c.1162C>T", "STXBP1: c.…": the symbol right before
            let before = &text[..m.start()];
            let tail: String = before
                .chars()
                .rev()
                .take(24)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            SYMBOL.find_iter(&tail).last().map(|s| s.as_str().to_owned())
        });
        let hgvs = m.as_str().rsplit(':').next().unwrap_or(m.as_str()).to_owned();
        out.push(RawTerm {
            kind: TermKind::Variant,
            text: m.as_str().to_owned(),
            english: String::new(),
            hgvs,
            gene: gene.unwrap_or_default(),
            negated: false,
        });
    }
    for m in SYMBOL.find_iter(text) {
        let s = m.as_str();
        if s.len() >= 3 && !NOT_GENES.contains(&s) && s.chars().any(|c| c.is_ascii_digit() || s.len() >= 4) {
            out.push(RawTerm {
                kind: TermKind::Gene,
                text: s.to_owned(),
                english: String::new(),
                hgvs: String::new(),
                gene: String::new(),
                negated: false,
            });
        }
    }
    out
}

static WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}][\p{L}\p{N}'’-]*").expect("words"));

/// Word n-grams (1..=max_n words, within a line, ≥ 5 characters) for exact name lookups, longest first.
pub fn ngrams(text: &str, max_n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = WORDS.find_iter(line).map(|m| m.as_str()).collect();
        for n in (1..=max_n).rev() {
            for w in words.windows(n) {
                let g = w.join(" ");
                if g.chars().count() >= 5 && !g.contains("NAME") {
                    out.push(g);
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|g| seen.insert(g.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(kind: TermKind, text: &str) -> RawTerm {
        RawTerm {
            kind,
            text: text.into(),
            english: String::new(),
            hgvs: String::new(),
            gene: String::new(),
            negated: false,
        }
    }

    #[test]
    fn locate_drops_invented_terms() {
        let text = "Krampfanfälle seit dem 3. Monat.\nSTXBP1 c.1162C>T";
        let (found, dropped) = locate(
            text,
            vec![
                term(TermKind::Phenotype, "krampfanfälle"),
                term(TermKind::Gene, "SCN1A"),
                term(TermKind::Gene, "STXBP1"),
            ],
        );
        assert_eq!(dropped, 1);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].start, found[0].end), (0, 13));
        assert!(found[1].quote.contains("STXBP1"));
    }

    #[test]
    fn lexical_variants_and_genes() {
        let t = lexical("Befund: STXBP1 c.1162C>T, p.(Arg388*); NM_000834.5(GRIN2B):c.1912G>A. EEG unauffällig.");
        let variants: Vec<_> = t.iter().filter(|x| x.kind == TermKind::Variant).collect();
        assert!(
            variants.iter().any(|v| v.gene == "STXBP1" && v.hgvs == "c.1162C>T"),
            "{variants:?}"
        );
        assert!(
            variants.iter().any(|v| v.gene == "GRIN2B" && v.hgvs == "c.1912G>A"),
            "{variants:?}"
        );
        let genes: Vec<_> = t
            .iter()
            .filter(|x| x.kind == TermKind::Gene)
            .map(|x| x.text.as_str())
            .collect();
        assert!(genes.contains(&"STXBP1") && !genes.contains(&"EEG"), "{genes:?}");
    }

    #[test]
    fn schema_is_valid_json_schema() {
        let s = schema();
        assert!(jsonschema_ok(&s.schema));
    }

    fn jsonschema_ok(v: &serde_json::Value) -> bool {
        v["properties"]["terms"]["items"]["required"]
            .as_array()
            .is_some_and(|r| r.len() == 6)
    }
}
