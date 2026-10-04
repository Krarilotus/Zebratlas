//! Deterministic fallbacks: question → chips without a model, and facts → template answer.
//!
//! Used when the user's provider is unavailable, fails, or its plan or answer does not validate.
//! The atlas then still answers, from the same executors, in plain template sentences.

use atlas_core::node::NodeKind;
use atlas_core::search::{SearchOptions, Tier};
use atlas_llm::tasks::{Cited, SentenceKind};

use crate::exec::Ctx;
use crate::facts::FactBook;
use crate::intent::{Chip, ChipOrigin, ChipStatus, IntentKind};

/// Keyword groups (English and German; other languages fall through to the default chips).
const GROUP: &[&str] = &[
    "group",
    "foundation",
    "association",
    "community",
    "verein",
    "stiftung",
    "selbsthilfe",
    "gemeinschaft",
    "families",
    "familien",
];
const STUDY: &[&str] = &[
    "trial", "study", "studies", "registry", "studie", "register", "recruit", "rekrut",
];
const PEOPLE: &[&str] = &[
    "researcher",
    "scientist",
    "expert",
    "doctor",
    "forscher",
    "wissenschaftler",
    "experte",
    "arzt",
    "ärzt",
    "spezialist",
];
const RELATED: &[&str] = &[
    "similar",
    "related",
    "same gene",
    "like ours",
    "ähnlich",
    "verwandt",
    "gleiche gen",
];
const GAPS: &[&str] = &[
    "nothing", "missing", "gap", "no group", "no study", "fehlt", "lücke", "keine",
];

fn has(q: &str, words: &[&str]) -> bool {
    words.iter().any(|w| q.contains(w))
}

/// The condition the question is about: an id, a gene symbol, or an exact name (longest n-gram).
pub fn subject(ctx: &Ctx<'_>, question: &str) -> Option<String> {
    let words: Vec<String> = question
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-')
                .to_owned()
        })
        .filter(|w| !w.is_empty())
        .collect();
    for w in &words {
        if w.contains(':') && ctx.atlas.disease_idx(w).is_some() {
            return Some(w.clone());
        }
    }
    for w in &words {
        // `STXBP1`, also inside compounds such as `STXBP1-Mutation` or `SCN1A-related`.
        for part in [w.as_str(), w.split('-').next().unwrap_or(w)] {
            let symbol_like = part.chars().any(|c| c.is_ascii_digit()) && part.chars().any(|c| c.is_ascii_uppercase());
            if symbol_like && ctx.resolve_gene(part).is_some() {
                return Some(part.to_owned());
            }
        }
    }
    let opts = SearchOptions {
        limit: 3,
        include_retired: false,
    };
    for n in (1..=4.min(words.len())).rev() {
        for win in words.windows(n) {
            let text = win.join(" ");
            if text.len() < 4 {
                continue;
            }
            let exact = ctx
                .atlas
                .search()
                .search(&text, opts)
                .into_iter()
                .any(|h| h.tier == Tier::Exact && matches!(h.node.kind, NodeKind::Disease | NodeKind::Gene));
            if exact {
                return Some(text);
            }
        }
    }
    None
}

/// Chips for a question without a model.
pub fn parse(ctx: &Ctx<'_>, question: &str) -> Vec<Chip> {
    let q = question.to_lowercase();
    let Some(s) = subject(ctx, question) else {
        return vec![Chip::new(IntentKind::Resolve, &[("text", question.trim())]).with_origin(ChipOrigin::Rules)];
    };
    let c = |intent, extra: &[(&str, &str)]| {
        let mut slots = vec![("condition", s.as_str())];
        slots.extend_from_slice(extra);
        Chip::new(intent, &slots).with_origin(ChipOrigin::Rules)
    };
    let mut chips = Vec::new();
    if has(&q, GROUP) {
        chips.push(c(IntentKind::Connections, &[("kind", "patient_group")]));
    }
    if has(&q, STUDY) {
        chips.push(c(IntentKind::Assets, &[("kind", "any"), ("status", "open")]));
    }
    if has(&q, PEOPLE) {
        chips.push(c(IntentKind::Connections, &[("kind", "researcher")]));
    }
    if has(&q, RELATED) {
        chips.push(c(IntentKind::Related, &[("by", "both")]));
    }
    if has(&q, GAPS) {
        chips.push(c(IntentKind::Gaps, &[]));
    }
    if chips.is_empty() {
        chips.push(c(IntentKind::ConditionSummaryFacts, &[]));
        chips.push(c(IntentKind::Connections, &[("kind", "any")]));
    }
    chips
}

/// Template answer straight from the facts (English, the facts' language).
pub fn template(chips: &[Chip], book: &FactBook) -> Vec<Cited> {
    let mut out = Vec::new();
    for chip in chips {
        match chip.status {
            Some(ChipStatus::Found) => {
                for key in chip.facts.iter().take(4) {
                    if let Some(f) = book.get(key) {
                        out.push(Cited {
                            msg: f.msg.clone(),
                            text: sentence(&f.text),
                            cites: vec![f.key.clone()],
                            kind: SentenceKind::Fact,
                        });
                    }
                }
            }
            Some(ChipStatus::Empty) | Some(ChipStatus::Invalid) => {
                let msg = crate::copy::msg(
                    "ask.template.no_result",
                    serde_json::json!({"intent": chip.intent.as_str()}),
                );
                out.push(Cited {
                    text: msg["fallback"].as_str().unwrap_or_default().to_owned(),
                    msg: Some(msg),
                    cites: vec![],
                    kind: SentenceKind::Caveat,
                });
            }
            None => {}
        }
        if out.len() >= 12 {
            break;
        }
    }
    if out.is_empty() {
        out.push(Cited {
            text: crate::copy::msg("ask.template.no_facts", serde_json::json!({}))["fallback"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            msg: Some(crate::copy::msg("ask.template.no_facts", serde_json::json!({}))),
            cites: vec![],
            kind: SentenceKind::Caveat,
        });
    }
    out.truncate(12);
    out
}

fn sentence(s: &str) -> String {
    let t = s.trim();
    if t.ends_with(['.', '!', '?']) {
        t.to_owned()
    } else {
        format!("{t}.")
    }
}
