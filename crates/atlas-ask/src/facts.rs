//! The evidence bundle of one answer: every fact an executor returned, under a short key (`E1`)
//! that the model cites, and the edge or node id behind it (`/api/provenance/{id}`).

use std::collections::HashMap;

use atlas_llm::Fact;
use serde::{Deserialize, Serialize};

/// One citable statement from the atlas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskFact {
    /// Short key the model cites (`E1`, `E2`, ...).
    pub key: String,
    /// Plain statement with the exact names, ids, numbers and links.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<serde_json::Value>,
    /// Edge id (`from|relation|to`) or node id; resolves at `/api/provenance/{id}`.
    pub id: String,
    /// `id` is an edge (else a node or a coverage record).
    pub edge: bool,
    /// `observed` / `inferred` / ... for edges; `attribute` for node facts; `coverage` for searches.
    pub kind: String,
    /// Human name of the source (`ClinicalTrials.gov`, `Orphanet`, `HPO annotation`, ...).
    pub source: String,
    /// Official public page for the fact, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Chip that produced the fact first.
    pub chip: String,
}

/// What an executor hands in; the book assigns the key.
#[derive(Clone, Debug)]
pub struct NewFact {
    pub id: String,
    pub edge: bool,
    pub kind: &'static str,
    pub text: String,
    pub msg: Option<serde_json::Value>,
    pub source: String,
    pub url: Option<String>,
}

impl NewFact {
    pub fn edge(id: impl Into<String>, kind: &'static str, text: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            edge: true,
            kind,
            text: text.into(),
            msg: None,
            source: source.into(),
            url: None,
        }
    }

    pub fn node(id: impl Into<String>, text: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            edge: false,
            kind: "attribute",
            text: text.into(),
            msg: None,
            source: source.into(),
            url: None,
        }
    }

    pub fn coverage(id: impl Into<String>, text: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            edge: false,
            kind: "coverage",
            text: text.into(),
            msg: None,
            source: source.into(),
            url: None,
        }
    }

    pub fn with_url(mut self, url: Option<String>) -> Self {
        self.url = url.filter(|u| !u.trim().is_empty());
        self
    }

    pub fn node_msg(id: impl Into<String>, msg: serde_json::Value, source: impl Into<String>) -> Self {
        Self::node(id, msg["fallback"].as_str().unwrap_or_default(), source).with_message(msg)
    }

    pub fn edge_msg(
        id: impl Into<String>,
        kind: &'static str,
        msg: serde_json::Value,
        source: impl Into<String>,
    ) -> Self {
        Self::edge(id, kind, msg["fallback"].as_str().unwrap_or_default(), source).with_message(msg)
    }

    pub fn coverage_msg(id: impl Into<String>, msg: serde_json::Value, source: impl Into<String>) -> Self {
        Self::coverage(id, msg["fallback"].as_str().unwrap_or_default(), source).with_message(msg)
    }

    fn with_message(mut self, msg: serde_json::Value) -> Self {
        self.msg = Some(msg);
        self
    }
}

/// Facts of one answer, deduplicated by `(id, text)`.
#[derive(Clone, Debug, Default)]
pub struct FactBook {
    facts: Vec<AskFact>,
    seen: HashMap<(String, String), usize>,
}

impl FactBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or find) a fact; returns its key.
    pub fn add(&mut self, chip: &str, f: NewFact) -> String {
        let text = squash(&f.text);
        if let Some(&i) = self.seen.get(&(f.id.clone(), text.clone())) {
            return self.facts[i].key.clone();
        }
        let key = format!("E{}", self.facts.len() + 1);
        self.seen.insert((f.id.clone(), text.clone()), self.facts.len());
        self.facts.push(AskFact {
            key: key.clone(),
            text,
            msg: f.msg,
            id: f.id,
            edge: f.edge,
            kind: f.kind.to_owned(),
            source: f.source,
            url: f.url,
            chip: chip.to_owned(),
        });
        key
    }

    pub fn all(&self) -> &[AskFact] {
        &self.facts
    }

    pub fn get(&self, key: &str) -> Option<&AskFact> {
        self.facts.iter().find(|f| f.key == key)
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// The facts as atlas-llm validator input. The URL is part of the text so a sentence may
    /// repeat it.
    pub fn for_validator(&self) -> Vec<Fact> {
        self.facts.iter().map(|f| Fact::new(&f.key, prompt_text(f))).collect()
    }

    pub fn into_vec(self) -> Vec<AskFact> {
        self.facts
    }
}

/// Text shown to the model for a fact (with its official link).
pub fn prompt_text(f: &AskFact) -> String {
    match &f.url {
        Some(u) if !f.text.contains(u.as_str()) => format!("{} (link: {u})", f.text),
        _ => f.text.clone(),
    }
}

/// `[E1] text` lines.
pub fn block(facts: &[AskFact]) -> String {
    facts
        .iter()
        .map(|f| format!("[{}] {}", f.key, prompt_text(f)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line, single spaces.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_assigned_once() {
        let mut b = FactBook::new();
        let k1 = b.add("c1", NewFact::edge("A|r|B", "observed", "A  links\nB", "src"));
        let k2 = b.add("c2", NewFact::edge("A|r|B", "observed", "A links B", "src"));
        let k3 = b.add(
            "c2",
            NewFact::node("A", "A is a thing", "src").with_url(Some("https://a.org".into())),
        );
        assert_eq!((k1.as_str(), k2.as_str(), k3.as_str()), ("E1", "E1", "E2"));
        assert_eq!(b.get("E1").unwrap().chip, "c1");
        assert!(block(b.all()).contains("[E2] A is a thing (link: https://a.org)"));
        assert_eq!(b.for_validator()[1].text, "A is a thing (link: https://a.org)");
    }
}
