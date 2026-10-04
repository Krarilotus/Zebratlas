//! The validator behind every generated text (D4, D10, J1.2, J1.3, J1.5, J2.5).
//!
//! A sentence passes when:
//! - every cited key exists;
//! - a factual sentence cites at least one fact;
//! - every URL, identifier (CURIE, NCT number), gene-like symbol and number in it appears in the
//!   facts it cites (other sentence kinds: in any fact or in the caller's verbatim strings);
//! - claims share content tokens with cited facts and keep their negation/polarity;
//! - it is short enough for a reading age of about 12 (word cap, space-separated scripts only).
//!
//! These are citation and token checks, not a semantic verification of the claim.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::Fact;

static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s)\]>"'<]+"#).unwrap());
/// `ORPHA:558`, `PMID:123`, `HGNC:11444`, `MONDO:0100143`, `NCT01234567`.
static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:[A-Za-z][A-Za-z0-9_]*:[A-Za-z0-9_][A-Za-z0-9_.-]*|NCT\d{8})\b").unwrap());
/// All-caps symbols with a digit: `STXBP1`, `SCN1A`, `KCNQ2`.
static SYMBOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][A-Z0-9-]*[0-9][A-Z0-9-]*\b").unwrap());
// Preserve spelling: without locale information even 30,000 versus 30.000 is ambiguous.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:[+-]\b|\b)\d+(?:[.,]\d+)*(?:\s*(?:%|percent\b|prozent\b|mg\b|µg\b|ug\b|kg\b|g\b|ml\b|mmol\b|mm\b|cm\b))?",
    )
    .unwrap()
});
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]+(?:['’][\p{L}]+)?").unwrap());
static CLAUSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?;](?:\s+|$)").unwrap());

/// What a sentence is for. Only `Fact` needs citations; every kind is token-checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SentenceKind {
    Fact,
    Greeting,
    Ask,
    /// "What must be checked by experts" (J2.5).
    Caveat,
    Closing,
}

/// One generated sentence with the fact keys it relies on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cited {
    pub text: String,
    /// Deterministic template copy; model-written sentences have no catalog message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<serde_json::Value>,
    #[serde(default)]
    pub cites: Vec<String>,
    #[serde(default = "fact_kind")]
    pub kind: SentenceKind,
}

fn fact_kind() -> SentenceKind {
    SentenceKind::Fact
}

/// Verbatim tokens found in a text.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub urls: Vec<String>,
    pub ids: Vec<String>,
    pub symbols: Vec<String>,
    /// Exact digit strings (separators and zeros kept), with recognised units attached.
    pub numbers: Vec<String>,
}

fn norm_number(s: &str) -> String {
    s.to_lowercase()
        .replace("percent", "%")
        .replace("prozent", "%")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Extract URLs, then ids, then symbols, then numbers, each from what the previous left.
pub fn tokens(text: &str) -> Tokens {
    let mut rest = text.to_owned();
    let mut take = |re: &Regex| {
        let found: Vec<String> = re
            .find_iter(&rest)
            .map(|m| m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']).to_owned())
            .collect();
        rest = re.replace_all(&rest, " ").into_owned();
        found
    };
    let urls = take(&URL);
    let ids = take(&ID);
    let symbols = take(&SYMBOL);
    let numbers = NUMBER.find_iter(&rest).map(|m| norm_number(m.as_str())).collect();
    Tokens {
        urls,
        ids,
        symbols,
        numbers,
    }
}

/// Numeric values and units from facts; identifiers do not license unrelated counts.
fn fact_numbers(text: &str) -> BTreeSet<String> {
    tokens(text).numbers.into_iter().collect()
}

fn words(text: &str) -> BTreeSet<String> {
    WORD.find_iter(text).map(|m| m.as_str().to_lowercase()).collect()
}

/// English and German negation/absence cues, including contractions and inflections.
fn negative(text: &str) -> bool {
    words(text).iter().any(|w| {
        matches!(
            w.as_str(),
            "not"
                | "no"
                | "never"
                | "neither"
                | "nor"
                | "without"
                | "absent"
                | "negative"
                | "cannot"
                | "n't"
                | "nicht"
                | "nie"
                | "niemals"
                | "ohne"
                | "weder"
                | "kein"
                | "keine"
                | "keinen"
                | "keinem"
                | "keiner"
                | "keines"
                | "fehlt"
                | "abwesend"
                | "negativ"
        ) || w.ends_with("n't")
            || w.ends_with("n’t")
    })
}

fn content_words(text: &str) -> BTreeSet<String> {
    words(text)
        .into_iter()
        .filter(|w| {
            !matches!(
                w.as_str(),
                "a" | "an"
                    | "the"
                    | "is"
                    | "are"
                    | "was"
                    | "were"
                    | "be"
                    | "been"
                    | "it"
                    | "its"
                    | "in"
                    | "on"
                    | "at"
                    | "of"
                    | "for"
                    | "to"
                    | "and"
                    | "or"
                    | "by"
                    | "with"
                    | "this"
                    | "that"
                    | "these"
                    | "those"
                    | "has"
                    | "have"
                    | "had"
                    | "not"
                    | "no"
                    | "without"
                    | "der"
                    | "die"
                    | "das"
                    | "ein"
                    | "eine"
                    | "ist"
                    | "sind"
                    | "mit"
                    | "und"
                    | "nicht"
                    | "kein"
                    | "keine"
                    | "sie"
                    | "es"
                    | "im"
                    | "von"
                    | "zu"
            )
        })
        .collect()
}

// This is a lexical support check, not semantic entailment. Match each clause against the
// most relevant cited clause so a negative trial fact cannot license a positive trial claim.
fn support_issues(text: &str, scope: &str) -> Vec<String> {
    let mut issues = vec![];
    for claim in CLAUSE.split(text).filter(|s| !s.trim().is_empty()) {
        let terms = content_words(claim);
        let clauses: Vec<_> = CLAUSE
            .split(scope)
            .map(|f| (f, terms.intersection(&content_words(f)).count()))
            .collect();
        let best = clauses.iter().map(|(_, score)| *score).max().unwrap_or(0);
        if best == 0 {
            issues.push("claim has no cited support (no shared content tokens)".into());
        } else if clauses
            .iter()
            .filter(|(_, score)| *score == best)
            .any(|(fact, _)| negative(fact) != negative(claim))
        {
            issues.push("preserve the cited fact's negation/polarity".into());
        }
    }
    issues
}

pub fn check_in_language(sentences: &[Cited], facts: &[Fact], rules: &Rules, lang: &str) -> Vec<String> {
    let mut issues = check(sentences, facts, rules);
    if !matches!(lang.split(['-', '_']).next().unwrap_or(lang), "en" | "de")
        && sentences.iter().any(|s| {
            s.kind == SentenceKind::Fact && facts.iter().any(|f| s.cites.contains(&f.key) && negative(&f.text))
        })
    {
        issues.push("negation cannot be checked in this language; use the source template".into());
    }
    issues
}

/// Limits for one task.
#[derive(Clone, Debug)]
pub struct Rules {
    pub min_sentences: usize,
    pub max_sentences: usize,
    /// Word cap per sentence (reading age ≈ 12); `None` = no cap.
    pub max_words: Option<usize>,
    /// Minimum number of `Fact` sentences.
    pub min_fact_sentences: usize,
    /// Kinds allowed in this task.
    pub kinds: &'static [SentenceKind],
    /// Kinds that must occur at least once.
    pub required_kinds: &'static [SentenceKind],
    /// Strings that may appear without a fact (recipient and sender names, the condition label).
    pub verbatim: Vec<String>,
}

impl Rules {
    pub fn facts_only(min: usize, max: usize, max_words: usize) -> Self {
        Self {
            min_sentences: min,
            max_sentences: max,
            max_words: Some(max_words),
            min_fact_sentences: min,
            kinds: &[SentenceKind::Fact],
            required_kinds: &[],
            verbatim: vec![],
        }
    }
}

/// All problems with a set of sentences; empty = valid.
pub fn check(sentences: &[Cited], facts: &[Fact], rules: &Rules) -> Vec<String> {
    let mut issues = Vec::new();
    let n = sentences.len();
    if n < rules.min_sentences || n > rules.max_sentences {
        issues.push(format!(
            "write {}–{} sentences, not {n}",
            rules.min_sentences, rules.max_sentences
        ));
    }
    let all_text: String = facts
        .iter()
        .map(|f| f.text.as_str())
        .chain(rules.verbatim.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let mut fact_sentences = 0;
    for (i, s) in sentences.iter().enumerate() {
        let no = i + 1;
        if !rules.kinds.contains(&s.kind) {
            issues.push(format!("sentence {no}: kind {:?} is not allowed here", s.kind));
        }
        let unknown: Vec<&str> = s
            .cites
            .iter()
            .filter(|k| !facts.iter().any(|f| &f.key == *k))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            issues.push(format!("sentence {no} cites unknown keys {unknown:?}"));
        }
        if s.kind == SentenceKind::Fact {
            fact_sentences += 1;
            if s.cites.is_empty() {
                issues.push(format!("sentence {no} states a fact without citing a fact key"));
            }
            let cited = facts
                .iter()
                .filter(|f| s.cites.contains(&f.key))
                .map(|f| f.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            issues.extend(
                support_issues(&s.text, &cited)
                    .into_iter()
                    .map(|issue| format!("sentence {no}: {issue}")),
            );
        }
        // Facts that license the tokens of this sentence.
        let scope: String = if s.kind == SentenceKind::Fact {
            facts
                .iter()
                .filter(|f| s.cites.contains(&f.key))
                .map(|f| f.text.as_str())
                .chain(rules.verbatim.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            all_text.clone()
        };
        let t = tokens(&s.text);
        for tok in t.urls.iter().chain(&t.ids).chain(&t.symbols) {
            if !scope.contains(tok.as_str()) {
                issues.push(format!(
                    "sentence {no}: '{tok}' is not in the cited facts (copy names, IDs and links exactly)"
                ));
            }
        }
        let allowed = fact_numbers(&scope);
        for num in &t.numbers {
            if !allowed.contains(num) {
                issues.push(format!("sentence {no}: the number {num} is not in the cited facts"));
            }
        }
        if let Some(max) = rules.max_words {
            let words = s.text.split_whitespace().count();
            // Scripts without spaces (zh, ja) are not counted this way.
            if words > 1 && words > max {
                issues.push(format!(
                    "sentence {no} has {words} words; use at most {max} simple words"
                ));
            }
        }
        if s.text.trim().is_empty() {
            issues.push(format!("sentence {no} is empty"));
        }
    }
    if fact_sentences < rules.min_fact_sentences {
        issues.push(format!(
            "give at least {} cited factual sentences, not {fact_sentences}",
            rules.min_fact_sentences
        ));
    }
    for kind in rules.required_kinds {
        if !sentences.iter().any(|s| s.kind == *kind) {
            issues.push(format!("add a sentence of kind {kind:?}"));
        }
    }
    issues
}

/// Tokens of `original` that a translation dropped or changed.
pub fn check_translation_languages(original: &str, translation: &str, source: &str, target: &str) -> Vec<String> {
    let mut issues = check_translation(original, translation);
    let supported = |lang: &str| matches!(lang.split(['-', '_']).next().unwrap_or(lang), "en" | "de");
    // For an unsupported source language we cannot know whether it negates a claim. Fail closed.
    if !supported(source) || (!supported(target) && negative(original)) {
        issues.push("negation cannot be checked in this language; keep the original".into());
    }
    issues
}

pub fn check_translation(original: &str, translation: &str) -> Vec<String> {
    let a = tokens(original);
    let b = tokens(translation);
    let mut issues = Vec::new();
    if negative(original) != negative(translation) {
        issues.push("preserve the original negation/polarity".into());
    }
    for tok in a.urls.iter().chain(&a.ids).chain(&a.symbols) {
        if !translation.contains(tok.as_str()) {
            issues.push(format!("keep '{tok}' exactly as in the original"));
        }
    }
    let have: BTreeSet<&String> = b.numbers.iter().collect();
    for n in &a.numbers {
        if !have.contains(n) {
            issues.push(format!("the number {n} from the original is missing"));
        }
    }
    for n in &b.numbers {
        if !a.numbers.contains(n) {
            issues.push(format!("the number {n} is not in the original"));
        }
    }
    if translation.trim().is_empty() {
        issues.push("the translation is empty".into());
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Vec<Fact> {
        vec![
            Fact::new(
                "F1",
                "STXBP1 encephalopathy (ORPHA:178469) is caused by variants in STXBP1.",
            ),
            Fact::new(
                "F2",
                "About 1 in 30,000 births; see https://www.orpha.net/en/disease/detail/178469.",
            ),
            Fact::new("F3", "Trial NCT01234567 is recruiting in 3 countries."),
        ]
    }

    fn s(text: &str, cites: &[&str]) -> Cited {
        Cited {
            msg: None,
            text: text.into(),
            cites: cites.iter().map(|c| (*c).into()).collect(),
            kind: SentenceKind::Fact,
        }
    }

    #[test]
    fn tokenizer() {
        let t = tokens("STXBP1 (ORPHA:178469) affects 1 in 30.000; see https://x.org/a. Trial NCT01234567.");
        assert_eq!(t.urls, vec!["https://x.org/a"]);
        assert_eq!(t.ids, vec!["ORPHA:178469", "NCT01234567"]);
        assert_eq!(t.symbols, vec!["STXBP1"]);
        assert_eq!(t.numbers, vec!["1", "30.000"]);
    }

    #[test]
    fn valid_text_passes() {
        let r = Rules::facts_only(2, 2, 25);
        let ok = [
            s(
                "STXBP1 encephalopathy is a rare brain condition caused by changes in the STXBP1 gene.",
                &["F1"],
            ),
            s("It affects about 1 in 30,000 babies.", &["F2"]),
        ];
        assert!(check(&ok, &facts(), &r).is_empty(), "{:?}", check(&ok, &facts(), &r));
    }

    #[test]
    fn catches_invented_numbers_ids_uncited_and_wrong_scope() {
        let r = Rules::facts_only(1, 3, 25);
        let issues = check(
            &[
                s("It affects 1 in 20,000 babies.", &["F2"]),   // invented number
                s("A trial NCT09999999 is open.", &["F3"]),     // invented id
                s("SCN1A is also involved.", &[]),              // uncited + unknown symbol
                s("Trial NCT01234567 is recruiting.", &["F1"]), // id from a fact it does not cite
                s("x", &["F9"]),                                // unknown key
            ],
            &facts(),
            &r,
        );
        let all = issues.join("\n");
        for needle in [
            "20,000",
            "NCT09999999",
            "without citing",
            "SCN1A",
            "NCT01234567",
            "F9",
            "write 1–3",
        ] {
            assert!(all.contains(needle), "missing {needle} in:\n{all}");
        }
    }

    #[test]
    fn long_sentences_are_flagged() {
        let long = "word ".repeat(30);
        let issues = check(&[s(&long, &["F1"])], &facts(), &Rules::facts_only(1, 1, 20));
        assert!(issues.iter().any(|i| i.contains("30 words")));
    }

    #[test]
    fn translation_keeps_tokens() {
        let orig = "Die Studie NCT01234567 (https://x.org) rekrutiert 12 Kinder mit STXBP1.";
        assert!(
            check_translation(
                orig,
                "The study NCT01234567 (https://x.org) recruits 12 children with STXBP1."
            )
            .is_empty()
        );
        let bad = check_translation(orig, "The study recruits twelve children with STXBP-1.");
        assert_eq!(bad.len(), 4, "{bad:?}");
    }

    #[test]
    fn adversarial_claims_are_rejected() {
        let facts = [Fact::new(
            "F",
            "Trial NCT01234567 is not recruiting. The observed rate is 0.5 percent.",
        )];
        for claim in [
            "Trial NCT01234567 is recruiting.",
            "The observed rate is 5 percent.",
            "A cure is available.",
        ] {
            assert!(
                !check(&[s(claim, &["F"])], &facts, &Rules::facts_only(1, 1, 25)).is_empty(),
                "accepted {claim}"
            );
        }
    }

    #[test]
    fn translation_preserves_polarity_and_quantities() {
        assert!(
            check_translation(
                "Trial NCT01234567 is not recruiting.",
                "Studie NCT01234567 rekrutiert nicht."
            )
            .is_empty()
        );
        assert!(check_translation("The rate is 0.5 percent.", "Die Rate ist 0.5 Prozent.").is_empty());
        for (original, translated) in [
            ("Trial NCT01234567 is not recruiting.", "Studie NCT01234567 rekrutiert."),
            ("Die Studie rekrutiert nicht.", "The study is recruiting."),
            ("The rate is 0.5%.", "Die Rate ist 5%."),
            ("The dose is 0.5 mg.", "Die Dosis ist 0.5 g."),
            ("The change is -0.5%.", "Die Änderung ist 0.5%."),
            ("The value is 005.", "Der Wert ist 5."),
        ] {
            assert!(
                !check_translation(original, translated).is_empty(),
                "accepted {translated}"
            );
        }
    }
}
