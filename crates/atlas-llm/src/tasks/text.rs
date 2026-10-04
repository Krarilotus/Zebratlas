//! J1.2 plain summary, J1.3/J2.1 why sentence, J1.5/J2.5 message drafts, D16 snippet translation.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::validate::{Cited, Rules, SentenceKind, check, check_in_language, check_translation_languages};
use super::{
    Fact, Generated, Origin, Validation, cited_messages, facts_block, guarded, language_name, sentences_schema,
};
use crate::llm::{Call, Completion, Llm};
use crate::request::{CompletionRequest, JsonSchema, Message};

const DEADLINE: Duration = Duration::from_secs(90);
/// gpt-oss and other reasoning models spend tokens on thinking first.
const MAX_TOKENS: u32 = 4000;

#[derive(Deserialize)]
struct Sentences {
    sentences: Vec<Cited>,
}

#[derive(Deserialize)]
struct SubjectAndSentences {
    subject: String,
    sentences: Vec<Cited>,
}

fn keys(facts: &[Fact]) -> Vec<String> {
    facts.iter().map(|f| f.key.clone()).collect()
}

fn request(messages: Vec<Message>, schema: serde_json::Value, name: &str) -> CompletionRequest {
    CompletionRequest::new(messages)
        .with_schema(JsonSchema::new(name, schema))
        .with_temperature(0.0)
        .with_max_tokens(MAX_TOKENS)
        .with_deadline(DEADLINE)
}

/// Template: the first `n` facts verbatim, each citing itself. Facts are English (`en`).
fn template(facts: &[Fact], n: usize) -> Vec<Cited> {
    facts
        .iter()
        .take(n)
        .map(|f| Cited {
            msg: f.msg.clone(),
            text: f.text.clone(),
            cites: vec![f.key.clone()],
            kind: SentenceKind::Fact,
        })
        .collect()
}

fn finish(
    lang: &str,
    value: Option<Vec<Cited>>,
    fallback: Vec<Cited>,
    validation: Validation,
    calls: Vec<Completion>,
) -> Generated {
    match value {
        Some(sentences) => Generated {
            lang: lang.into(),
            requested_lang: lang.into(),
            sentences,
            origin: Origin::Llm,
            validation,
            calls,
        },
        None => Generated {
            lang: "en".into(),
            requested_lang: lang.into(),
            sentences: fallback,
            origin: Origin::Template,
            validation,
            calls,
        },
    }
}

fn no_facts(lang: &str) -> Generated {
    Generated {
        lang: "en".into(),
        requested_lang: lang.into(),
        sentences: vec![],
        origin: Origin::Template,
        validation: Validation {
            fallback_reason: Some("no facts given".into()),
            ..Validation::default()
        },
        calls: vec![],
    }
}

/// J1.2: "what this is" in exactly two plain, cited sentences (reading age ≈ 12).
pub async fn plain_summary(llm: &Llm, call: &Call, condition: &str, facts: &[Fact], lang: &str) -> Generated {
    let routed = llm.for_easy_task(call);
    let call = &routed;
    if facts.is_empty() {
        return no_facts(lang);
    }
    let rules = Rules {
        verbatim: vec![condition.to_owned()],
        ..Rules::facts_only(2, 2, 25)
    };
    let task = "Task: plain summary. You explain a rare condition to a parent who has just heard the diagnosis. \
                Write exactly two sentences: what this condition is, and what it usually means in everyday life. \
                At most 25 words each.";
    let user = format!(
        "Write in {}.\nCondition: {condition}\nFacts:\n{}\n\nReply as JSON.",
        language_name(lang),
        facts_block(facts)
    );
    let req = request(
        cited_messages(task, user),
        sentences_schema(&keys(facts), 2, 2, None, false),
        "plain_summary",
    );
    let a = guarded::<Sentences>(llm, call, req, |s| check_in_language(&s.sentences, facts, &rules, lang)).await;
    finish(
        lang,
        a.value.map(|s| s.sentences),
        template(facts, 2),
        a.validation,
        a.calls,
    )
}

/// J1.3 / J2.1: one plain sentence why this connection is relevant for the user, cited.
pub async fn why_sentence(llm: &Llm, call: &Call, who: &str, facts: &[Fact], lang: &str) -> Generated {
    let routed = llm.for_easy_task(call);
    let call = &routed;
    if facts.is_empty() {
        return no_facts(lang);
    }
    let rules = Rules {
        verbatim: vec![who.to_owned()],
        ..Rules::facts_only(1, 1, 30)
    };
    let task = "Task: why sentence. You tell a family in one plain sentence (at most 30 words) why a group, \
                study or person is relevant for them, using the name exactly as given.";
    let user = format!(
        "Write in {}.\nWho: {who}\nFacts:\n{}\n\nWrite one sentence saying why {who} is relevant for this family. Reply as JSON.",
        language_name(lang),
        facts_block(facts)
    );
    let req = request(
        cited_messages(task, user),
        sentences_schema(&keys(facts), 1, 1, None, false),
        "why_sentence",
    );
    let a = guarded::<Sentences>(llm, call, req, |s| check_in_language(&s.sentences, facts, &rules, lang)).await;
    finish(
        lang,
        a.value.map(|s| s.sentences),
        template(facts, 1),
        a.validation,
        a.calls,
    )
}

/// Who is writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Parent,
    Patient,
    Carer,
    GroupLeader,
    Researcher,
    Clinician,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    /// J1.5: a family contacts a group / study / expert.
    Outreach,
    /// J2.5: a group proposes a partnership (why us, why you, what to share, what experts must check, meeting ask).
    PartnerProposal,
}

/// The connection card a message is drafted from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCard {
    /// Recipient name, copied verbatim (e.g. "STXBP1 Foundation").
    pub recipient: String,
    /// `patient group`, `study team`, `registry`, `expert centre`, `researcher`.
    pub recipient_kind: String,
    /// Official public channel (website / contact form / CT.gov contact page); shown by the UI.
    pub channel: Option<String>,
    /// The condition label, verbatim.
    pub condition: String,
    pub facts: Vec<Fact>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub kind: DraftKind,
    pub subject: String,
    pub body: Generated,
    pub channel: Option<String>,
}

impl Draft {
    /// Greeting line, body paragraph, closing line.
    pub fn text(&self) -> String {
        let part = |pred: &dyn Fn(SentenceKind) -> bool| {
            self.body
                .sentences
                .iter()
                .filter(|s| pred(s.kind))
                .map(|s| s.text.trim())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let greeting = part(&|k| k == SentenceKind::Greeting);
        let middle = part(&|k| !matches!(k, SentenceKind::Greeting | SentenceKind::Closing));
        let closing = part(&|k| k == SentenceKind::Closing);
        [greeting, middle, closing]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Deterministic messages always carry English fallbacks; the UI renders their catalog keys.
fn draft_template(
    card: &MessageCard,
    _role: UserRole,
    kind: DraftKind,
    lang: &str,
    _sender: Option<&str>,
) -> (String, String, Vec<Cited>) {
    let cited = |msg: serde_json::Value, kind: SentenceKind| Cited {
        text: super::copy::legacy_text(&msg, lang),
        msg: Some(msg),
        cites: vec![],
        kind,
    };
    let message = super::copy::msg;
    // Keep complete source claims; a deterministic fallback must not invent a
    // shorter paraphrase or cut an important qualification mid-sentence.
    let concise: Vec<Fact> = card
        .facts
        .iter()
        .filter(|f| f.text.split_whitespace().count() <= 25)
        .cloned()
        .collect();
    let mut out = template(&concise, if kind == DraftKind::PartnerProposal { 3 } else { 2 });
    if kind == DraftKind::PartnerProposal {
        out.push(cited(
            message("write.template.check_links", json!({})),
            SentenceKind::Caveat,
        ));
        out.push(cited(message("write.template.meeting", json!({})), SentenceKind::Ask));
    } else {
        out.push(cited(message("write.template.ask", json!({})), SentenceKind::Ask));
    }
    let subject = message("write.template.subject", json!({"condition": card.condition}));
    (
        if lang.to_ascii_lowercase().starts_with("de") {
            "de"
        } else {
            "en"
        }
        .into(),
        super::copy::legacy_text(&subject, lang),
        out,
    )
}

/// J1.5 / J2.5: an editable message to the card's recipient, in `lang`. Names, IDs and links are
/// copied verbatim (validator); every factual sentence cites a fact.
pub async fn draft_message(
    llm: &Llm,
    call: &Call,
    card: &MessageCard,
    role: UserRole,
    kind: DraftKind,
    lang: &str,
    sender: Option<&str>,
) -> Draft {
    let proposal = kind == DraftKind::PartnerProposal;
    let routed = if proposal { call.clone() } else { llm.for_easy_task(call) };
    let call = &routed;
    let mut verbatim = vec![card.recipient.clone(), card.condition.clone()];
    verbatim.extend(sender.map(str::to_owned));
    verbatim.extend(card.channel.clone());
    let rules = Rules {
        min_sentences: if card.facts.is_empty() { 1 } else { 2 },
        max_sentences: if proposal { 5 } else { 4 },
        max_words: Some(25),
        min_fact_sentences: if proposal {
            card.facts.len().min(2)
        } else {
            card.facts.len().min(1)
        },
        kinds: if proposal {
            &[SentenceKind::Fact, SentenceKind::Ask, SentenceKind::Caveat]
        } else {
            &[SentenceKind::Fact, SentenceKind::Ask]
        },
        required_kinds: &[SentenceKind::Ask],
        verbatim,
    };
    let kinds: &[&str] = if proposal {
        &["fact", "ask", "caveat"]
    } else {
        &["fact", "ask"]
    };
    let schema = sentences_schema(
        &keys(&card.facts),
        rules.min_sentences,
        rules.max_sentences,
        Some(kinds),
        true,
    );
    let task = if proposal {
        "a concise partnership request: one or two relevant sourced premises, a caveat only when necessary, \
         and exactly one specific request (kind ask)"
    } else {
        "one or two relevant sourced premises followed by exactly one specific question (kind ask)"
    };
    let system = "Task: write a concise, factual message that a person can edit and send.\n\
         - Write the recipient's name exactly as given. Do not invent contact details, dates or promises.\n\
         - Every factual premise must cite supplied facts. Do not infer a personal diagnosis, family, role, affiliation or shared resources.\n\
         - No greeting, signature, pleasantries, filler, personal introduction or unsupported claims.\n\
         - Include exactly one direct question or request. Keep each sentence at most 25 words.\n\
         - Sentence kinds: fact (needs cites), ask, caveat only when needed.\n\
         - Also give a short subject line (at most 15 words).";
    let user = format!(
        "Write in {}.\nRecipient: {} ({})\nCondition: {}\nFacts:\n{}\n\nWrite {task}. \
         Reply as JSON.",
        language_name(lang),
        card.recipient,
        card.recipient_kind,
        card.condition,
        facts_block(&card.facts)
    );
    let req = request(
        cited_messages(system, user),
        schema,
        if proposal {
            "partner_proposal"
        } else {
            "outreach_message"
        },
    );
    let recipient = card.recipient.clone();
    let a = guarded::<SubjectAndSentences>(llm, call, req, |m| {
        let mut issues = check_in_language(&m.sentences, &card.facts, &rules, lang);
        if m.sentences.iter().filter(|s| s.kind == SentenceKind::Ask).count() != 1 {
            issues.push("include exactly one specific request".into());
        }
        if !m.sentences.iter().any(|s| s.text.contains(&recipient)) {
            issues.push(format!("address the recipient by the exact name '{recipient}'"));
        }
        let subject = [Cited {
            msg: None,
            text: m.subject.clone(),
            cites: vec![],
            kind: SentenceKind::Greeting,
        }];
        let subject_rules = Rules {
            min_sentences: 1,
            max_sentences: 1,
            max_words: Some(15),
            min_fact_sentences: 0,
            kinds: &[SentenceKind::Greeting],
            required_kinds: &[],
            verbatim: rules.verbatim.clone(),
        };
        issues.extend(
            check(&subject, &card.facts, &subject_rules)
                .into_iter()
                .map(|i| i.replace("sentence 1", "subject")),
        );
        issues
    })
    .await;
    match a.value {
        Some(m) => Draft {
            kind,
            subject: m.subject,
            body: Generated {
                lang: lang.into(),
                requested_lang: lang.into(),
                sentences: m.sentences,
                origin: Origin::Llm,
                validation: a.validation,
                calls: a.calls,
            },
            channel: card.channel.clone(),
        },
        None => {
            let (tlang, subject, sentences) = draft_template(card, role, kind, lang, sender);
            Draft {
                kind,
                subject,
                body: Generated {
                    lang: tlang,
                    requested_lang: lang.into(),
                    sentences,
                    origin: Origin::Template,
                    validation: a.validation,
                    calls: a.calls,
                },
                channel: card.channel.clone(),
            }
        }
    }
}

/// D16: a foreign-language quote with a translation next to it (the original is always kept).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Translation {
    pub original: String,
    /// Language of the original as the model saw it (BCP-47), if translated.
    pub source_lang: Option<String>,
    pub lang: String,
    /// `None` when no valid translation could be made: show the original only.
    pub text: Option<String>,
    pub origin: Origin,
    pub validation: Validation,
    pub calls: Vec<Completion>,
}

#[derive(Deserialize)]
struct Translated {
    source_lang: String,
    translation: String,
}

pub async fn translate_snippet(llm: &Llm, call: &Call, text: &str, lang: &str) -> Translation {
    let routed = llm.for_easy_task(call);
    let call = &routed;
    let mut out = Translation {
        original: text.to_owned(),
        source_lang: None,
        lang: lang.to_owned(),
        text: None,
        origin: Origin::Template,
        validation: Validation::default(),
        calls: vec![],
    };
    if text.trim().is_empty() {
        return out;
    }
    let schema = json!({
        "type": "object", "additionalProperties": false, "required": ["source_lang", "translation"],
        "properties": {"source_lang": {"type": "string"}, "translation": {"type": "string"}}
    });
    let system = "You translate short quotes from medical and patient-organisation sources faithfully. \
                  Keep names, gene symbols, IDs, numbers and links exactly as written. Do not add, explain or shorten. \
                  Reply with JSON only.";
    let user = format!(
        "Translate into {}. source_lang = BCP-47 code of the original.\n\"\"\"{text}\"\"\"\nReply as JSON.",
        language_name(lang)
    );
    let req = request(
        vec![Message::system(system).cached(), Message::user(user)],
        schema,
        "translate_snippet",
    );
    let a = guarded::<Translated>(llm, call, req, |t| {
        check_translation_languages(text, &t.translation, &t.source_lang, lang)
    })
    .await;
    out.validation = a.validation;
    out.calls = a.calls;
    if let Some(t) = a.value {
        out.source_lang = Some(t.source_lang);
        out.text = Some(t.translation);
        out.origin = Origin::Llm;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_valid_against_their_own_rules() {
        let card = MessageCard {
            recipient: "STXBP1 Foundation".into(),
            recipient_kind: "patient group".into(),
            channel: Some("https://www.stxbp1disorders.org/contact".into()),
            condition: "STXBP1 encephalopathy".into(),
            facts: vec![
                Fact::new(
                    "E1",
                    "The STXBP1 Foundation is a patient group for STXBP1 encephalopathy.",
                ),
                Fact::new("E2", "It runs a registry with 600 families."),
            ],
        };
        for lang in ["en", "de"] {
            let (l, subject, sentences) =
                draft_template(&card, UserRole::Parent, DraftKind::Outreach, lang, Some("Devon"));
            assert_eq!(l, lang);
            assert!(subject.contains("STXBP1 encephalopathy"));
            let rules = Rules {
                min_sentences: 2,
                max_sentences: 4,
                max_words: Some(25),
                min_fact_sentences: 1,
                kinds: &[SentenceKind::Fact, SentenceKind::Ask],
                required_kinds: &[SentenceKind::Ask],
                verbatim: vec![card.recipient.clone(), card.condition.clone(), "Devon".into()],
            };
            assert!(
                check(&sentences, &card.facts, &rules).is_empty(),
                "{lang}: {:?}",
                check(&sentences, &card.facts, &rules)
            );
            assert_eq!(sentences.iter().filter(|s| s.kind == SentenceKind::Ask).count(), 1);
            assert!(
                !sentences
                    .iter()
                    .any(|s| matches!(s.kind, SentenceKind::Greeting | SentenceKind::Closing))
            );
        }
        let d = Draft {
            kind: DraftKind::Outreach,
            subject: "s".into(),
            body: finish("en", None, template(&card.facts, 1), Validation::default(), vec![]),
            channel: None,
        };
        assert_eq!(d.text(), card.facts[0].text);
    }
}
