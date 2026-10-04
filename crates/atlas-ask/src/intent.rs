//! The query intents: what a model (or a user editing chips) may ask the atlas.
//!
//! Each intent has typed slots, a JSON schema for tool calling ([`IntentSpec::schema`]) and a
//! deterministic executor in [`crate::exec`]. A parsed question is a list of [`Chip`]s: an intent
//! plus slot values as written by the model or the user. Executors resolve slot text to atlas nodes
//! and record the result on the chip, so the UI can show and edit it before or after running.

use std::collections::BTreeMap;

use atlas_core::node::NodeRef;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Resolve,
    ConditionSummaryFacts,
    Connections,
    Related,
    Assets,
    SharedPeople,
    Path,
    Gaps,
    DiseasesWith,
    NodeDetails,
}

impl IntentKind {
    pub const ALL: [IntentKind; 10] = [
        Self::Resolve,
        Self::ConditionSummaryFacts,
        Self::Connections,
        Self::Related,
        Self::Assets,
        Self::SharedPeople,
        Self::Path,
        Self::Gaps,
        Self::DiseasesWith,
        Self::NodeDetails,
    ];

    pub fn as_str(self) -> &'static str {
        self.spec().name
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s.trim())
    }

    pub fn spec(self) -> &'static IntentSpec {
        INTENTS
            .iter()
            .find(|s| s.kind == self)
            .expect("every intent has a spec")
    }
}

/// What a slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "values", rename_all = "snake_case")]
pub enum SlotType {
    /// Free text the user typed (a question fragment, a name).
    Text,
    /// A condition: name, alias, gene symbol or id (`MONDO:…`, `ORPHA:…`, `OMIM:…`).
    Condition,
    /// Any atlas node: condition, gene, symptom, study (`NCT…`), grant, paper, person, organisation.
    Node,
    /// A gene symbol, alias or HGNC id.
    Gene,
    /// A symptom name or HPO id.
    Symptom,
    /// One of a fixed set of values.
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SlotSpec {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub ty: SlotType,
    pub required: bool,
    /// Value used when the slot is empty (choices only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<&'static str>,
    pub help: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct IntentSpec {
    #[serde(skip)]
    pub kind: IntentKind,
    pub name: &'static str,
    pub description: &'static str,
    pub slots: &'static [SlotSpec],
    /// At least one of these slots must be filled (`diseases_with`).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub one_of: &'static [&'static str],
}

const fn slot(name: &'static str, ty: SlotType, required: bool, help: &'static str) -> SlotSpec {
    SlotSpec {
        name,
        ty,
        required,
        default: None,
        help,
    }
}

const fn choice(
    name: &'static str,
    values: &'static [&'static str],
    default: &'static str,
    help: &'static str,
) -> SlotSpec {
    SlotSpec {
        name,
        ty: SlotType::Choice(values),
        required: false,
        default: Some(default),
        help,
    }
}

pub const CONNECTION_KINDS: &[&str] = &[
    "any",
    "patient_group",
    "expert_centre",
    "study",
    "trial",
    "registry",
    "natural_history",
    "observational",
    "researcher",
    "grant",
];
pub const ASSET_KINDS: &[&str] = &[
    "any",
    "study",
    "trial",
    "registry",
    "natural_history",
    "observational",
    "expanded_access",
    "grant",
];
pub const ASSET_STATUS: &[&str] = &["any", "open", "recruiting", "closed"];
pub const RELATED_BY: &[&str] = &["both", "gene", "symptom"];

const CONDITION: SlotSpec = slot(
    "condition",
    SlotType::Condition,
    true,
    "the condition as the user wrote it: name, alias, gene symbol or id",
);

/// Every intent, in the order the prompt lists them.
pub const INTENTS: &[IntentSpec] = &[
    IntentSpec {
        kind: IntentKind::Resolve,
        name: "resolve",
        description: "Find which conditions, genes or symptoms in the atlas match a name, alias, typo or id. Use when the user's words are ambiguous.",
        slots: &[slot("text", SlotType::Text, true, "the words to look up")],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::ConditionSummaryFacts,
        name: "condition_summary_facts",
        description: "Basic facts about one condition: definition, other names, genes, inheritance, onset, frequency, common symptoms, how many studies and groups the atlas lists.",
        slots: &[CONDITION],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::Connections,
        name: "connections",
        description: "Who can help with a condition: patient groups, expert centres, studies, registries, researchers and research projects, each with its official public channel.",
        slots: &[
            CONDITION,
            choice("kind", CONNECTION_KINDS, "any", "which kind of connection"),
        ],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::Related,
        name: "related",
        description: "Other conditions that share a gene or informative symptoms with this condition.",
        slots: &[
            CONDITION,
            choice("by", RELATED_BY, "both", "shared gene, shared symptoms, or both"),
        ],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::Assets,
        name: "assets",
        description: "Existing research assets for a condition: trials, registries, natural-history and observational studies, research projects (grants), filtered by kind and status.",
        slots: &[
            CONDITION,
            choice("kind", ASSET_KINDS, "any", "which kind of asset"),
            choice("status", ASSET_STATUS, "any", "open = still running or recruiting"),
        ],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::SharedPeople,
        name: "shared_people",
        description: "Researchers who work on both of two conditions (or genes), with the papers or grants that show it.",
        slots: &[
            slot("a", SlotType::Condition, true, "first condition or gene"),
            slot("b", SlotType::Condition, true, "second condition or gene"),
        ],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::Path,
        name: "path",
        description: "The shortest chain of sourced links between two things in the atlas (conditions, genes, studies, people, organisations).",
        slots: &[
            slot("from", SlotType::Node, true, "start: any name or id"),
            slot("to", SlotType::Node, true, "end: any name or id"),
        ],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::Gaps,
        name: "gaps",
        description: "What the atlas did not find for a condition (no exact patient group, no open study, ...) and which sources were searched when.",
        slots: &[CONDITION],
        one_of: &[],
    },
    IntentSpec {
        kind: IntentKind::DiseasesWith,
        name: "diseases_with",
        description: "Conditions linked to a gene, a symptom, or a biological process. Fill exactly one of gene, symptom, process.",
        slots: &[
            slot("gene", SlotType::Gene, false, "gene symbol, alias or HGNC id"),
            slot("symptom", SlotType::Symptom, false, "symptom name or HPO id"),
            slot("process", SlotType::Text, false, "biological process or pathway name"),
        ],
        one_of: &["gene", "symptom", "process"],
    },
    IntentSpec {
        kind: IntentKind::NodeDetails,
        name: "node_details",
        description: "Details of one item: a study (NCT id), organisation, researcher, grant, paper, gene or condition, with its direct links.",
        slots: &[slot("id", SlotType::Node, true, "the item's id or name")],
        one_of: &[],
    },
];

/// Every slot name used by any intent, in a fixed order (the flat tool-call schema).
pub fn all_slot_names() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for s in INTENTS.iter().flat_map(|i| i.slots) {
        if !out.contains(&s.name) {
            out.push(s.name);
        }
    }
    out
}

impl IntentSpec {
    /// JSON schema of the intent's arguments (OpenAI-style tool `parameters`).
    pub fn schema(&self) -> Value {
        let mut props = serde_json::Map::new();
        for s in self.slots {
            let mut p = json!({"type": "string", "description": s.help});
            if let SlotType::Choice(values) = s.ty {
                p["enum"] = json!(values);
            }
            if let Some(d) = s.default {
                p["default"] = json!(d);
            }
            props.insert(s.name.into(), p);
        }
        let required: Vec<&str> = self.slots.iter().filter(|s| s.required).map(|s| s.name).collect();
        let mut schema = json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        });
        if !self.one_of.is_empty() {
            schema["minProperties"] = json!(1);
        }
        schema
    }

    /// Tool definition: `{name, description, parameters}`.
    pub fn tool(&self) -> Value {
        json!({"name": self.name, "description": self.description, "parameters": self.schema()})
    }

    /// One prompt line: `connections(condition, kind=any|patient_group|…): description`.
    pub fn prompt_line(&self) -> String {
        let args: Vec<String> = self
            .slots
            .iter()
            .map(|s| match s.ty {
                SlotType::Choice(v) => format!("{}={}", s.name, v.join("|")),
                _ if s.required => s.name.to_owned(),
                _ => format!("{}?", s.name),
            })
            .collect();
        format!("- {}({}): {}", self.name, args.join(", "), self.description)
    }
}

/// Who wrote a chip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChipOrigin {
    /// Parsed from the question by the user's model.
    #[default]
    Model,
    /// Written or edited by the user.
    User,
    /// Parsed by the deterministic fallback (no model, or the model's plan failed).
    Rules,
}

/// Outcome of running a chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChipStatus {
    /// Ran and returned facts.
    Found,
    /// Ran; the atlas has nothing for it (the notes say what was searched).
    Empty,
    /// Could not run (missing slot, unknown name, ...); see `notes`.
    Invalid,
}

/// A slot value resolved to an atlas node, with the other candidates the user may switch to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub node: NodeRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<NodeRef>,
}

/// One editable step of a parsed question: an intent with its slot values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    /// `c1`, `c2`, ... (stable within one answer).
    #[serde(default)]
    pub id: String,
    pub intent: IntentKind,
    /// Slot values as written (by the model or the user).
    #[serde(default)]
    pub slots: BTreeMap<String, String>,
    #[serde(default)]
    pub origin: ChipOrigin,
    /// Filled when the chip ran.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resolved: BTreeMap<String, Resolved>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ChipStatus>,
    /// Fact keys this chip produced (`E1`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<String>,
    /// Plain notes (not citable): what was searched, why nothing came back, why it could not run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes_msg: Vec<Value>,
}

impl Chip {
    pub fn new(intent: IntentKind, slots: &[(&str, &str)]) -> Self {
        Self {
            id: String::new(),
            intent,
            slots: slots.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            origin: ChipOrigin::Model,
            resolved: BTreeMap::new(),
            status: None,
            facts: Vec::new(),
            notes: Vec::new(),
            notes_msg: Vec::new(),
        }
    }

    pub fn with_origin(mut self, origin: ChipOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Slot value, trimmed; empty counts as absent. Choices fall back to their default.
    pub fn slot(&self, name: &str) -> Option<&str> {
        let given = self.slots.get(name).map(|s| s.trim()).filter(|s| !s.is_empty());
        given.or_else(|| {
            self.intent
                .spec()
                .slots
                .iter()
                .find(|s| s.name == name)
                .and_then(|s| s.default)
        })
    }

    /// Problems with the slots (missing required, unknown slot, invalid choice); empty = valid.
    pub fn check(&self) -> Vec<String> {
        let spec = self.intent.spec();
        let mut issues = Vec::new();
        for (k, v) in &self.slots {
            if v.trim().is_empty() {
                continue;
            }
            match spec.slots.iter().find(|s| s.name == k) {
                None => issues.push(format!("{} has no slot '{k}'", spec.name)),
                Some(s) => {
                    if let SlotType::Choice(values) = s.ty
                        && !values.contains(&v.trim())
                    {
                        issues.push(format!("{}: '{k}' must be one of {}", spec.name, values.join(", ")));
                    }
                }
            }
        }
        for s in spec.slots.iter().filter(|s| s.required) {
            if self.slot(s.name).is_none() {
                issues.push(format!("{}: slot '{}' is empty", spec.name, s.name));
            }
        }
        if !spec.one_of.is_empty() && !spec.one_of.iter().any(|n| self.slot(n).is_some()) {
            issues.push(format!("{}: fill one of {}", spec.name, spec.one_of.join(", ")));
        }
        issues
    }

    /// Same intent and the same non-empty slot values (used to skip repeated calls).
    pub fn same_query(&self, other: &Chip) -> bool {
        let norm = |c: &Chip| -> Vec<(String, String)> {
            c.intent
                .spec()
                .slots
                .iter()
                .filter_map(|s| c.slot(s.name).map(|v| (s.name.to_owned(), v.to_lowercase())))
                .collect()
        };
        self.intent == other.intent && norm(self) == norm(other)
    }

    /// Clear what a previous run recorded (before re-running an edited chip).
    pub fn reset(&mut self) {
        self.notes_msg.clear();
        self.resolved.clear();
        self.status = None;
        self.facts.clear();
        self.notes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_intent_has_a_valid_schema() {
        assert_eq!(INTENTS.len(), IntentKind::ALL.len());
        for k in IntentKind::ALL {
            let spec = k.spec();
            assert_eq!(IntentKind::parse(spec.name), Some(k));
            let schema = spec.schema();
            assert_eq!(schema["type"], "object");
            assert!(serde_json::to_string(&spec.tool()).unwrap().contains(spec.name));
        }
        assert!(all_slot_names().contains(&"condition"));
    }

    #[test]
    fn chip_checks_slots() {
        let ok = Chip::new(
            IntentKind::Connections,
            &[("condition", "STXBP1"), ("kind", "patient_group")],
        );
        assert!(ok.check().is_empty());
        assert_eq!(ok.slot("kind"), Some("patient_group"));
        let default = Chip::new(IntentKind::Connections, &[("condition", "STXBP1")]);
        assert_eq!(default.slot("kind"), Some("any"));
        let bad = Chip::new(IntentKind::Connections, &[("kind", "pizza"), ("colour", "red")]);
        let issues = bad.check().join("\n");
        assert!(
            issues.contains("pizza") || issues.contains("must be one of"),
            "{issues}"
        );
        assert!(
            issues.contains("colour") && issues.contains("'condition' is empty"),
            "{issues}"
        );
        let none = Chip::new(IntentKind::DiseasesWith, &[]);
        assert!(none.check().iter().any(|i| i.contains("fill one of")));
        assert!(ok.same_query(&Chip::new(
            IntentKind::Connections,
            &[("condition", "stxbp1"), ("kind", "patient_group")]
        )));
    }

    #[test]
    fn chip_json_roundtrip() {
        let c: Chip =
            serde_json::from_value(json!({"intent": "related", "slots": {"condition": "Dravet", "by": "gene"}}))
                .unwrap();
        assert_eq!(c.intent, IntentKind::Related);
        assert_eq!(c.origin, ChipOrigin::Model);
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["intent"], "related");
    }
}
