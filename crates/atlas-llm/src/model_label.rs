//! Which model produced an output, for the UI (D48: "every LLM output shows which model produced
//! it"). A catalog message (D26): `key` + `params` + English `fallback`, e.g.
//! `llm.model.open_weight {model: "gpt-oss-120b", vendor: "OpenAI"}` = "gpt-oss-120b (OpenAI open-weight)".
//!
//! Keys (the web catalogs need all three, same params):
//! - `llm.model.open_weight` = "{model} ({vendor} open-weight)"
//! - `llm.model.vendor` = "{model} ({vendor})"
//! - `llm.model.plain` = "{model}"

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Catalog message naming a model. `params.model` is the plain model name (provider prefixes such
/// as `openai/` or KISSKI's `openai-` removed); `params.id` keeps the exact id that was requested.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLabel {
    pub key: String,
    pub params: BTreeMap<String, String>,
    pub fallback: String,
}

/// Open-weight model families and their vendor.
const OPEN_WEIGHT: &[(&str, &str)] = &[("gpt-oss", "OpenAI")];

/// Closed model families and their vendor (by name prefix).
const VENDORS: &[(&str, &str)] = &[
    ("gpt-", "OpenAI"),
    ("o1", "OpenAI"),
    ("o3", "OpenAI"),
    ("o4", "OpenAI"),
    ("codex", "OpenAI"),
    ("claude", "Anthropic"),
    ("sonnet", "Anthropic"),
    ("opus", "Anthropic"),
    ("haiku", "Anthropic"),
    ("gemini", "Google"),
];

/// Plain model name: `openai/gpt-oss-120b` → `gpt-oss-120b`, `openai-gpt-oss-120b` (KISSKI) →
/// `gpt-oss-120b`, `gpt-oss:20b` (Ollama tag) → `gpt-oss-20b`.
pub fn plain_model_name(id: &str) -> String {
    let last = id.trim().rsplit('/').next().unwrap_or(id);
    let name = if last.to_ascii_lowercase().starts_with("openai-") {
        last[7..].to_owned()
    } else {
        last.to_owned()
    };
    match name.split_once(':') {
        // Ollama size tags belong to the name; other tags (`:free`, `:latest`) do not.
        Some((base, tag)) if tag.ends_with('b') && tag[..tag.len() - 1].parse::<f64>().is_ok() => {
            format!("{base}-{tag}")
        }
        Some((base, _)) => base.to_owned(),
        None => name,
    }
}

/// Distinct labels of the models behind some calls, in call order (for "made with ..." in the UI).
pub fn model_labels(calls: &[crate::llm::Completion]) -> Vec<ModelLabel> {
    let mut out: Vec<ModelLabel> = Vec::new();
    for c in calls {
        if !out.contains(&c.response.model_label) {
            out.push(c.response.model_label.clone());
        }
    }
    out
}

/// The label for a model id as requested from a connection.
pub fn model_label(id: &str) -> ModelLabel {
    let model = plain_model_name(id);
    let mut params = BTreeMap::from([("model".to_owned(), model.clone()), ("id".to_owned(), id.to_owned())]);
    let lower = model.to_ascii_lowercase();
    let open = OPEN_WEIGHT.iter().find(|(p, _)| lower.starts_with(p));
    let vendor = VENDORS.iter().find(|(p, _)| lower.starts_with(p));
    let (key, fallback) = match (open, vendor) {
        (Some((_, v)), _) => {
            params.insert("vendor".into(), (*v).into());
            ("llm.model.open_weight", format!("{model} ({v} open-weight)"))
        }
        (None, Some((_, v))) => {
            params.insert("vendor".into(), (*v).into());
            ("llm.model.vendor", format!("{model} ({v})"))
        }
        _ => ("llm.model.plain", model),
    };
    ModelLabel {
        key: key.into(),
        params,
        fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_oss_everywhere_is_openai_open_weight() {
        for id in ["openai/gpt-oss-120b", "openai-gpt-oss-120b", "gpt-oss-120b"] {
            let l = model_label(id);
            assert_eq!(l.key, "llm.model.open_weight");
            assert_eq!(l.fallback, "gpt-oss-120b (OpenAI open-weight)");
            assert_eq!(l.params["vendor"], "OpenAI");
            assert_eq!(l.params["id"], id);
        }
        assert_eq!(model_label("gpt-oss:20b").fallback, "gpt-oss-20b (OpenAI open-weight)");
        assert_eq!(model_label("openai/gpt-oss-120b:free").params["model"], "gpt-oss-120b");
    }

    #[test]
    fn other_vendors_and_unknown_models() {
        assert_eq!(model_label("gpt-5-mini").fallback, "gpt-5-mini (OpenAI)");
        assert_eq!(
            model_label("anthropic/claude-sonnet-4-6").fallback,
            "claude-sonnet-4-6 (Anthropic)"
        );
        assert_eq!(model_label("gemini-2.5-flash").params["vendor"], "Google");
        let l = model_label("Qwen/Qwen3-32B");
        assert_eq!((l.key.as_str(), l.fallback.as_str()), ("llm.model.plain", "Qwen3-32B"));
    }
}
