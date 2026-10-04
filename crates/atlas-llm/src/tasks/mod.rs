//! Text tasks for the journeys (J1.1–J1.5, J2.1, J2.5, D16): reconcile, plain summary, why
//! sentence, message draft, snippet translation.
//!
//! Every generated sentence carries the keys of the facts it uses. The flow is always:
//! structured call (schema with a `cites` enum over the fact keys) → validator → on failure one
//! regeneration with the validator's issues → on failure (or any provider error / offline cache
//! miss) a deterministic template built from the facts. The result says which path was taken,
//! lists the validator's findings per attempt and carries the calls with their PROV-O records
//! (U1.1 "Under the hood").

pub mod copy;
pub mod reconcile;
pub mod text;
pub mod validate;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm::{Call, Completion, Llm, check_json};
use crate::request::{CompletionRequest, Message};

pub use reconcile::{Candidate, Confidence, Reconciled, reconcile};
pub use text::{
    Draft, DraftKind, MessageCard, Translation, UserRole, draft_message, plain_summary, translate_snippet, why_sentence,
};
pub use validate::{Cited, Rules, SentenceKind};

/// Connection used when the caller names none and sends no key: `ATLAS_LLM_DEFAULT`; else the
/// hosted free tier (D23, D48: OpenRouter gpt-oss-120b) when `OPENROUTER_API_KEY` is set and
/// `ATLAS_FREE_DISABLED` is not; else `kisski`. With an `Llm` at hand, prefer [`crate::Llm::default_connection`] (it also checks the
/// registry and the runtime kill switch).
pub fn default_connection() -> String {
    if let Ok(name) = std::env::var("ATLAS_LLM_DEFAULT")
        && !name.trim().is_empty()
    {
        return name.trim().to_owned();
    }
    let key = std::env::var("OPENROUTER_API_KEY").is_ok_and(|k| !k.trim().is_empty());
    let off = std::env::var("ATLAS_FREE_DISABLED").is_ok_and(|v| crate::free_tier::flag(&v));
    if key && !off {
        crate::free_tier::HOSTED_FREE.into()

    } else {
        "kisski".into()
    }
}

/// One citable fact, built by the caller from graph edges (key = the edge/evidence key shown in
/// "How we know", e.g. `E3`). `text` is a plain statement carrying the exact names, IDs, numbers
/// and URLs; generated text may only use what is in the facts it cites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub key: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<Value>,
}

impl Fact {
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
            msg: None,
        }
    }

    /// A deterministic record paraphrase with its catalog key and parameters.
    pub fn message(key: impl Into<String>, msg: Value) -> Self {
        Self {
            key: key.into(),
            text: msg["fallback"].as_str().unwrap_or_default().to_owned(),
            msg: Some(msg),
        }
    }
}

/// Which path produced the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// LLM output that passed the validator.
    Llm,
    /// Deterministic template from the facts (validator failed twice, or no LLM available).
    Template,
    /// Deterministic match without an LLM call (reconcile: exact name hit).
    Lexical,
}

/// The validator's findings, per attempt (empty list = passed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validation {
    pub attempts: Vec<Vec<String>>,
    pub passed: bool,
    /// Token/citation checks do not establish semantic entailment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assurance: Option<String>,
    /// Why the template was used (validator issues or the provider error).
    pub fallback_reason: Option<String>,
}

/// A validated, cited text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generated {
    /// Language of `sentences` (the template falls back to the facts' language, `en`).
    pub lang: String,
    pub requested_lang: String,
    pub sentences: Vec<Cited>,
    pub origin: Origin,
    pub validation: Validation,
    pub calls: Vec<Completion>,
}

impl Generated {
    /// A composed catalog message for deterministic output; LLM prose keeps its language tag.
    pub fn message(&self) -> Option<Value> {
        (self.origin == Origin::Template).then(|| {
            let sentences = self
                .sentences
                .iter()
                .map(|s| {
                    s.msg.clone().unwrap_or_else(|| {
                        json!({
                            "key": "record.statement", "params": {"statement": s.text}, "fallback": s.text
                        })
                    })
                })
                .collect::<Vec<_>>();
            let fallback = sentences
                .iter()
                .filter_map(|s| s["fallback"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            json!({"key": "text.sentences", "params": {"sentences": sentences}, "fallback": fallback})
        })
    }

    /// Sentences joined with spaces.
    pub fn text(&self) -> String {
        self.sentences
            .iter()
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Result of the guarded loop.
pub(crate) struct Attempted<T> {
    pub value: Option<T>,
    pub validation: Validation,
    pub calls: Vec<Completion>,
}

/// Structured call → schema check → task validator; one regeneration with the issues.
/// Provider errors end the loop at once (no automatic replay); the caller then uses its template.
pub(crate) async fn guarded<T: DeserializeOwned>(
    llm: &Llm,
    call: &Call,
    mut req: CompletionRequest,
    validate: impl Fn(&T) -> Vec<String>,
) -> Attempted<T> {
    let schema = req.schema.as_ref().map(|s| s.schema.clone()).unwrap_or(Value::Null);
    let mut validation = Validation::default();
    let mut calls = Vec::new();
    let started = std::time::Instant::now();
    for attempt in 0..2 {
        if let Some(budget) = call.deadline {
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                validation.fallback_reason = Some("task deadline exceeded".into());
                break;
            }
            req.deadline = req.deadline.min(remaining);
        }
        let done = match llm.complete(call, req.clone()).await {
            Ok(d) => d,
            Err(e) => {
                validation.fallback_reason = Some(format!("provider: {e}"));
                break;
            }
        };
        let text = done.response.text.clone();
        calls.push(done);
        let issues = match check_json(&schema, &text) {
            Err(e) => vec![e],
            Ok(v) => match serde_json::from_value::<T>(v) {
                Err(e) => vec![format!("wrong shape: {e}")],
                Ok(t) => {
                    let issues = validate(&t);
                    if issues.is_empty() {
                        validation.attempts.push(vec![]);
                        validation.passed = true;
                        validation.assurance = Some("citation and token checks passed".into());
                        return Attempted {
                            value: Some(t),
                            validation,
                            calls,
                        };
                    }
                    issues
                }
            },
        };
        validation.attempts.push(issues.clone());
        if attempt == 0 {
            req.messages.push(Message::assistant(text));
            req.messages.push(Message::user(format!(
                "Your answer broke these rules:\n- {}\nWrite it again following all rules. Reply with JSON only.",
                issues.join("\n- ")
            )));
        } else {
            validation.fallback_reason = Some(format!("validator: {}", issues.join("; ")));
        }
    }
    Attempted {
        value: None,
        validation,
        calls,
    }
}

/// Human-readable language name for prompts (the 12 UI languages; others pass the code through).
pub fn language_name(code: &str) -> String {
    let name = match code
        .split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_ascii_lowercase()
        .as_str()
    {
        "en" => "English",
        "de" => "German",
        "es" => "Spanish",
        "fr" => "French",
        "pt" => "Portuguese",
        "it" => "Italian",
        "zh" => "Chinese (Simplified)",
        "ja" => "Japanese",
        "hi" => "Hindi",
        "ar" => "Arabic",
        "ru" => "Russian",
        "tr" => "Turkish",
        "nl" => "Dutch",
        "pl" => "Polish",
        _ => return format!("the language with code '{code}'"),
    };
    format!("{name} ({code})")
}

/// `[E1] text` lines for the prompt.
pub(crate) fn facts_block(facts: &[Fact]) -> String {
    facts
        .iter()
        .map(|f| format!("[{}] {}", f.key, f.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// JSON schema for `{"sentences": [{"text", "cites", "kind"?}]}` with cites restricted to the keys.
pub(crate) fn sentences_schema(
    keys: &[String],
    min: usize,
    max: usize,
    kinds: Option<&[&str]>,
    with_subject: bool,
) -> Value {
    let mut item_props = json!({
        "text": {"type": "string"},
        "cites": {"type": "array", "items": {"type": "string", "enum": keys}},
    });
    let mut required = vec!["text", "cites"];
    if let Some(k) = kinds {
        item_props["kind"] = json!({"type": "string", "enum": k});
        required.push("kind");
    }
    let mut props = json!({
        "sentences": {
            "type": "array", "minItems": min, "maxItems": max,
            "items": {"type": "object", "additionalProperties": false, "required": required, "properties": item_props}
        }
    });
    let mut top_required = vec!["sentences"];
    if with_subject {
        props["subject"] = json!({"type": "string"});
        top_required.insert(0, "subject");
    }
    json!({"type": "object", "additionalProperties": false, "required": top_required, "properties": props})
}

/// Shared, stable preamble of every cited-text task (summary, why sentence, message). It is the
/// first message and an Anthropic prompt-cache breakpoint, so it is byte-identical across tasks,
/// languages and conditions: never format request data into it. Long enough (> 512 tokens) to be
/// cacheable on Claude Sonnet 5.5 / Opus 5.5.
pub const PREAMBLE: &str = "\
You write for Zebratlas, a free service that helps families, patients and patient-group \
leaders find the people, groups and studies that can help with a rare condition. Readers may have \
just heard a diagnosis, may read on a phone late at night, and may not be native speakers. Your text \
is shown next to its sources; every claim must be traceable to one of the numbered facts you are given.

How to write
- Use only the facts in the request. Do not add knowledge of your own, even if you are sure it is true.
- Every factual sentence lists in `cites` the keys of the facts it uses (for example [\"E2\"]). A sentence \
that states no fact (a greeting, a question, a closing) has an empty `cites` list.
- Copy names of people, organisations, studies, genes and conditions, and every ID, number and link, \
exactly as written in the facts. Never translate, shorten or re-spell them, even when you write in \
another language. Numbers stay digits as written in the facts.
- Plain words a 12-year-old understands. Short sentences (at most 25 words unless the task says \
otherwise). If a medical word is needed, explain it in the same sentence in everyday words.
- Be calm and direct. No medical advice, no diagnosis, no promises about treatment or outcomes, no \
speculation about what will happen to a particular child.
- Write like a careful human editor: no rhetorical questions, exclamation marks, dashes for effect, slogans \
or lists of three for rhythm; no reassurance such as \"you are not alone\" or \"don't worry\"; avoid the words \
journey, explore, discover, empower, unlock, simply and just. Say \"patient group\", \"clinical trial\", \
\"specialist centre\" and \"gene change\".
- If the facts do not support something, leave it out rather than guessing.
- Write in the language the request asks for. Keep the JSON keys in English.
- Reply with JSON only, matching the schema; no prose before or after it.

Worked example (format only; the facts in a real request differ)
Facts:
[E1] Dravet syndrome (ORPHA:33069) is a rare, severe epilepsy that starts in the first year of life.
[E2] Most people with Dravet syndrome have a change in the SCN1A gene.
[E3] The Dravet Syndrome Foundation (https://www.dravetfoundation.org) supports families in 12 countries.
Good answer:
{\"sentences\": [
  {\"text\": \"Dravet syndrome is a rare, serious kind of epilepsy (repeated seizures) that starts in a baby's first year.\", \"cites\": [\"E1\"]},
  {\"text\": \"In most people it comes from a change in one gene called SCN1A.\", \"cites\": [\"E2\"]}
]}
Bad answers and why:
- \"Dravet syndrome affects 1 in 15,700 children.\" (the number is not in the facts)
- \"The Dravet-Stiftung helps families.\" (the name was translated; copy it exactly)
- \"Children usually improve with the right medicine.\" (not in the facts, and it is a promise)
- \"Dravet syndrome is caused by SCN1A.\" with cites [] (a fact without a citation)
";

/// Messages for a cited-text task: the shared [`PREAMBLE`] and the task's fixed instructions
/// (both stable, both cache breakpoints), then the request with its language and facts.
pub(crate) fn cited_messages(task: &str, request: String) -> Vec<Message> {
    vec![
        Message::system(PREAMBLE).cached(),
        Message::system(task).cached(),
        Message::user(request),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_restricts_cites() {
        let s = sentences_schema(&["E1".into(), "E2".into()], 2, 2, None, false);
        assert_eq!(
            s["properties"]["sentences"]["items"]["properties"]["cites"]["items"]["enum"],
            json!(["E1", "E2"])
        );
        assert!(jsonschema::validator_for(&s).is_ok());
        let m = sentences_schema(&["E1".into()], 3, 12, Some(&["fact", "ask"]), true);
        assert_eq!(m["required"], json!(["subject", "sentences"]));
    }

    #[test]
    fn languages() {
        assert_eq!(language_name("de"), "German (de)");
        assert_eq!(language_name("zh-Hans"), "Chinese (Simplified) (zh-Hans)");
        assert!(language_name("sw").contains("'sw'"));
    }
}
