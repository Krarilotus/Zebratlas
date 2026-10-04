//! J1.1: free text in any language (a gene, an alias, a typo, a sentence from a doctor's letter)
//! → one of the search candidates. The model picks from an enum of candidate ids (it never
//! creates an id, D4) and must quote the span of the query it matched.

use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Origin, Validation, guarded, language_name};
use crate::llm::{Call, Completion, Llm};
use crate::request::{CompletionRequest, JsonSchema, Message};

/// A search hit offered to the model (from atlas-core search).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
    /// `disease`, `gene`, ...
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reconciled {
    pub query: String,
    /// Chosen candidate id, if any.
    pub choice: Option<String>,
    /// Up to three plain choices to show when ambiguous (J1.1: "≤3 plain choices").
    pub alternatives: Vec<String>,
    pub confidence: Confidence,
    /// The part of the query that names the condition (verbatim).
    pub mention: Option<String>,
    pub origin: Origin,
    pub validation: Validation,
    pub calls: Vec<Completion>,
}

#[derive(Debug, Deserialize)]
struct Pick {
    #[serde(rename = "match")]
    choice: String,
    alternatives: Vec<String>,
    confidence: Confidence,
    mention: String,
}

const NONE: &str = "none";

/// Stable system prompt (prompt-cache breakpoint): instructions and worked examples only, no
/// request data.
const RECONCILE_SYSTEM: &str = "\
You map what a worried family member typed to one entry of a fixed list of rare conditions and genes. \
The text may be in any language, contain typos, abbreviations or a protein name instead of a gene \
name, or be a whole sentence copied from a doctor's letter. Choose only from the listed candidate ids; \
answer \"none\" if nothing fits. Never invent an id and never pick an id that is not in the list.

Fields
- match: the best candidate id, or \"none\".
- alternatives: up to 2 other candidate ids that are also plausible; empty when the match is clear.
- confidence: high (the text names the condition or gene unambiguously), medium (a typo, a protein \
name, or a gene that maps to the condition), low (a guess between similar entries).
- mention: the exact words from the text that name the condition or gene, copied character for \
character, not translated and not corrected.

Prefer a disease entry over a gene entry when the text describes a person who has the condition \
(\"my son has ...\", \"diagnosed with ...\"); prefer the gene entry when the text only names a gene or variant \
without saying what the person has. Reply with JSON only.

Examples (ids are illustrative)
Text: \"scn1a dravet\" with candidates ORPHA:33069 Dravet syndrome, HGNC:10585 SCN1A
-> {\"match\": \"ORPHA:33069\", \"alternatives\": [\"HGNC:10585\"], \"confidence\": \"high\", \"mention\": \"dravet\"}
Text: \"Nachweis einer pathogenen Variante im SCN1A-Gen, Fieberkrämpfe seit dem 6. Monat\"
-> {\"match\": \"HGNC:10585\", \"alternatives\": [\"ORPHA:33069\"], \"confidence\": \"medium\", \"mention\": \"SCN1A-Gen\"}
Text: \"my daughter has angelman\" with no Angelman entry in the list
-> {\"match\": \"none\", \"alternatives\": [], \"confidence\": \"low\", \"mention\": \"angelman\"}
";

fn norm(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deterministic score of a candidate for the query (exact name = 1.0).
fn lexical_score(query: &str, c: &Candidate) -> f64 {
    let q = norm(query);
    let q_tokens: BTreeSet<&str> = q.split(' ').filter(|t| !t.is_empty()).collect();
    std::iter::once(&c.label)
        .chain(&c.synonyms)
        .map(|name| {
            let n = norm(name);
            if n.is_empty() {
                return 0.0;
            }
            if n == q {
                return 1.0;
            }
            let n_tokens: BTreeSet<&str> = n.split(' ').collect();
            // A whole name inside the query ("my daughter has an STXBP1 mutation").
            if q_tokens.is_superset(&n_tokens) {
                return 0.8;
            }
            // A prefix / typo-ish fragment ("stxbp").
            if q.len() >= 4 && n.split(' ').any(|t| t.starts_with(&q)) {
                return 0.6;
            }
            let inter = q_tokens.intersection(&n_tokens).count() as f64;
            let union = q_tokens.union(&n_tokens).count() as f64;
            0.5 * inter / union.max(1.0)
        })
        .fold(0.0, f64::max)
}

/// Lexical ranking: (unique exact hit?, ranked ids with score > 0).
fn lexical(query: &str, candidates: &[Candidate]) -> (Option<String>, Vec<(String, f64)>) {
    let mut ranked: Vec<(String, f64)> = candidates
        .iter()
        .map(|c| (c.id.clone(), lexical_score(query, c)))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let exact: Vec<&(String, f64)> = ranked.iter().filter(|(_, s)| *s >= 1.0).collect();
    let unique = (exact.len() == 1).then(|| exact[0].0.clone());
    (unique, ranked)
}

/// Pick the candidate the query means. An exact name hit needs no LLM; otherwise the model
/// chooses from the candidate ids; if it fails twice (or no LLM is reachable), the lexical
/// ranking decides.
pub async fn reconcile(llm: &Llm, call: &Call, query: &str, lang: &str, candidates: &[Candidate]) -> Reconciled {
    let mut out = Reconciled {
        query: query.to_owned(),
        choice: None,
        alternatives: vec![],
        confidence: Confidence::None,
        mention: None,
        origin: Origin::Lexical,
        validation: Validation {
            passed: true,
            ..Validation::default()
        },
        calls: vec![],
    };
    if candidates.is_empty() || query.trim().is_empty() {
        return out;
    }
    let (exact, ranked) = lexical(query, candidates);
    if let Some(id) = exact {
        out.choice = Some(id);
        out.confidence = Confidence::High;
        out.mention = Some(query.trim().to_owned());
        return out;
    }

    let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
    let mut choices = ids.clone();
    choices.push(NONE);
    let schema = json!({
        "type": "object", "additionalProperties": false,
        "required": ["match", "alternatives", "confidence", "mention"],
        "properties": {
            "match": {"type": "string", "enum": choices},
            "alternatives": {"type": "array", "maxItems": 2, "items": {"type": "string", "enum": ids}},
            "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
            "mention": {"type": "string"}
        }
    });
    let list = candidates
        .iter()
        .map(|c| {
            let syn = if c.synonyms.is_empty() {
                String::new()
            } else {
                format!("; also called: {}", c.synonyms.join(", "))
            };
            let kind = c.kind.as_deref().map(|k| format!(" ({k})")).unwrap_or_default();
            format!("- {}: {}{kind}{syn}", c.id, c.label)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let system = RECONCILE_SYSTEM;
    let user = format!(
        "Text (probably {}):\n\"\"\"{query}\"\"\"\n\nCandidates:\n{list}\n\n\
         Return JSON: match = best id or \"none\"; alternatives = up to 2 other plausible ids (empty if clear); \
         confidence; mention = the exact words from the text that name the condition or gene (copied, not translated).",
        language_name(lang)
    );
    let req = CompletionRequest::new(vec![Message::system(system).cached(), Message::user(user)])
        .with_schema(JsonSchema::new("reconcile", schema))
        .with_temperature(0.0)
        .with_max_tokens(2000)
        .with_deadline(Duration::from_secs(60));
    let q_norm = norm(query);
    let attempted = guarded::<Pick>(llm, call, req, |p| {
        let mut issues = vec![];
        if p.choice != NONE && !p.mention.trim().is_empty() && !q_norm.contains(&norm(&p.mention)) {
            issues.push(format!(
                "mention '{}' is not words from the text; copy them exactly",
                p.mention
            ));
        }
        if p.alternatives.contains(&p.choice) {
            issues.push("alternatives must not repeat the match".into());
        }
        issues
    })
    .await;
    out.calls = attempted.calls;
    out.validation = attempted.validation;
    if let Some(p) = attempted.value {
        out.origin = Origin::Llm;
        if p.choice != NONE {
            out.choice = Some(p.choice);
            out.mention = Some(p.mention).filter(|m| !m.trim().is_empty());
            out.confidence = p.confidence;
        }
        let mut alts = p.alternatives;
        alts.dedup();
        out.alternatives = alts.into_iter().take(3).collect();
        return out;
    }
    // Template path: lexical ranking.
    out.origin = Origin::Template;
    match ranked.as_slice() {
        [(top, s), rest @ ..] if *s >= 0.6 && rest.first().is_none_or(|(_, s2)| s - s2 >= 0.2) => {
            out.choice = Some(top.clone());
            out.confidence = Confidence::Medium;
        }
        _ => {
            out.alternatives = ranked.iter().take(3).map(|(id, _)| id.clone()).collect();
            out.confidence = if out.alternatives.is_empty() {
                Confidence::None
            } else {
                Confidence::Low
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands() -> Vec<Candidate> {
        vec![
            Candidate {
                id: "ORPHA:178469".into(),
                label: "STXBP1 encephalopathy with epilepsy".into(),
                synonyms: vec!["STXBP1 encephalopathy".into(), "Munc18-1 encephalopathy".into()],
                kind: Some("disease".into()),
            },
            Candidate {
                id: "HGNC:11444".into(),
                label: "STXBP1".into(),
                synonyms: vec!["Munc18-1".into()],
                kind: Some("gene".into()),
            },
            Candidate {
                id: "ORPHA:33069".into(),
                label: "Dravet syndrome".into(),
                synonyms: vec![],
                kind: None,
            },
        ]
    }

    #[test]
    fn exact_names_skip_the_llm() {
        let (exact, _) = lexical("stxbp1 encephalopathy", &cands());
        assert_eq!(exact.as_deref(), Some("ORPHA:178469"));
        let (exact, ranked) = lexical("Meine Tochter hat eine STXBP1-Mutation", &cands());
        assert_eq!(exact, None);
        assert_eq!(ranked[0].0, "HGNC:11444");
        let (_, ranked) = lexical("stxbp", &cands());
        assert!(ranked.iter().take(2).any(|(id, _)| id == "HGNC:11444"));
    }
}
