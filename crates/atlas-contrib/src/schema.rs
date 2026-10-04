//! Machine-readable form rules, also used by submission validation (D42).

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{ContribError, Result};
use crate::model::{
    ContributionKind, IdentifierSystem, MAX_IDENTIFIER_SYSTEMS, MAX_OTHER_CHARS, MAX_QUOTE_CHARS, MAX_STATEMENT_CHARS,
    Relationship, ResourceKind, SubjectKind, Submission,
};

#[derive(Serialize)]
struct KindRules {
    kind: ContributionKind,
    required: Vec<&'static str>,
    /// Every group requires at least one non-empty member.
    required_any: Vec<Vec<&'static str>>,
    optional: Vec<&'static str>,
}

fn rules(kind: ContributionKind) -> KindRules {
    let mut required = vec!["kind", "contributor.contact"];
    let required_any = match kind {
        ContributionKind::NewLink => vec![vec!["subject.id", "subject.label"], vec!["target.id", "target.label"]],
        ContributionKind::Correction => {
            required.push("statement");
            vec![vec!["edge", "subject.id", "subject.label"]]
        }
        ContributionKind::MissingEvidence => {
            vec![vec!["edge", "subject.id", "subject.label", "target.id", "target.label"]]
        }
        ContributionKind::OutdatedContact => vec![vec!["subject.id", "subject.label"]],
        ContributionKind::DataSource => {
            required.push("data_source");
            vec![vec!["data_source.url", "data_source.description"]]
        }
        ContributionKind::Other => {
            required.extend(["kind_other", "statement"]);
            vec![]
        }
    };
    let optional = fields()
        .into_iter()
        .map(|f| f.path)
        .filter(|p| !required.contains(p))
        .filter(|p| kind == ContributionKind::DataSource || !p.starts_with("data_source"))
        .collect();
    KindRules {
        kind,
        required,
        required_any,
        optional,
    }
}

#[derive(Serialize)]
struct Field {
    path: &'static str,
    field_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
}

fn fields() -> Vec<Field> {
    let mut out = Vec::new();
    for (path, max) in [
        ("kind_other", MAX_OTHER_CHARS),
        ("subject_kind_other", MAX_OTHER_CHARS),
        ("relationship_other", MAX_OTHER_CHARS),
        ("subject.id", 200),
        ("subject.label", 300),
        ("target.id", 200),
        ("target.label", 300),
        ("edge", 600),
        ("statement", MAX_STATEMENT_CHARS),
        ("evidence_url", 2000),
        ("quote", MAX_QUOTE_CHARS),
        ("contact_url", 2000),
        ("found_via.page", 600),
        ("found_via.assistant", 100),
        ("lang", 35),
        ("contributor.contact", 200),
        ("contributor.name", 120),
        ("contributor.organisation", 200),
        ("data_source.url", 2000),
        ("data_source.description", MAX_STATEMENT_CHARS),
        ("data_source.licence", 500),
        ("data_source.spdx_id", 100),
        ("data_source.resource_kind_other", MAX_OTHER_CHARS),
        ("data_source.identifier_systems_other", MAX_OTHER_CHARS),
    ] {
        let format = match path {
            "contributor.contact" => Some("email"),
            "evidence_url" | "contact_url" | "data_source.url" => Some("http-url-no-credentials"),
            "edge" => Some("from|relation|to"),
            _ => None,
        };
        out.push(Field {
            path,
            field_type: "string",
            max_length: Some(max),
            format,
        });
    }
    for (path, field_type) in [
        ("kind", "enum"),
        ("subject_kind", "enum"),
        ("relationship", "enum"),
        ("data_source", "object"),
        ("data_source.resource_kind", "enum"),
        ("data_source.identifier_systems", "enum-array"),
        ("data_source.consent", "boolean"),
    ] {
        out.push(Field {
            path,
            field_type,
            max_length: None,
            format: None,
        });
    }
    out
}

/// The same bound is used in cleaning and advertised to the form.
pub(crate) fn max_length(path: &str) -> Option<usize> {
    fields().into_iter().find(|f| f.path == path).and_then(|f| f.max_length)
}

fn choices<T: Serialize>(path: &str, values: &[T], other_field: &str) -> Value {
    json!({"path": path, "values": values, "other_field": other_field,
        "other_requires_text": true, "other_auto_mapped": false})
}

fn enumerations() -> Vec<Value> {
    vec![
        choices("kind", &ContributionKind::ALL, "kind_other"),
        choices(
            "subject_kind",
            &[
                SubjectKind::PatientGroup,
                SubjectKind::Organisation,
                SubjectKind::Registry,
                SubjectKind::Study,
                SubjectKind::Person,
                SubjectKind::Other,
            ],
            "subject_kind_other",
        ),
        choices(
            "relationship",
            &[
                Relationship::ServesCondition,
                Relationship::ServesGene,
                Relationship::StudiesCondition,
                Relationship::NamesGene,
                Relationship::ResearchesCondition,
                Relationship::ResearchesGene,
                Relationship::Other,
            ],
            "relationship_other",
        ),
        choices(
            "data_source.resource_kind",
            &[
                ResourceKind::Registry,
                ResourceKind::Dataset,
                ResourceKind::KnowledgeGraph,
                ResourceKind::Api,
                ResourceKind::SparqlEndpoint,
                ResourceKind::SssomMapping,
                ResourceKind::Other,
            ],
            "data_source.resource_kind_other",
        ),
        choices(
            "data_source.identifier_systems",
            &[
                IdentifierSystem::Mondo,
                IdentifierSystem::Omim,
                IdentifierSystem::Orpha,
                IdentifierSystem::Hgnc,
                IdentifierSystem::Hpo,
                IdentifierSystem::Nct,
                IdentifierSystem::Other,
            ],
            "data_source.identifier_systems_other",
        ),
    ]
}

/// Public form contract. `required_any` and enum companion conditions refine the optional list.
pub fn document() -> Value {
    json!({
        "schema": "atlas.contribute.schema", "version": 1,
        "kinds": ContributionKind::ALL.map(rules), "fields": fields(), "enumerations": enumerations(),
        "contact": {"path": "contributor.contact", "required": true,
            "supplied_by": "signed_in_account_email", "private": true},
        "normalization": {"trim_strings": true, "empty_strings_are_missing": true,
            "reject_control_characters_except": ["\n", "\t"]},
        "limits": {"data_source.identifier_systems": MAX_IDENTIFIER_SYSTEMS},
        "legacy_optional": ["data_source.consent"]
    })
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, key| v.get(key))
}

fn present(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}

pub(crate) fn validate(s: &Submission) -> Result<()> {
    let v = serde_json::to_value(s).map_err(|e| ContribError::Internal(e.to_string()))?;
    let rules = rules(s.kind);
    for field in rules.required {
        if !present(at(&v, field)) {
            return Err(ContribError::invalid(format!("{field} is required")));
        }
    }
    for group in rules.required_any {
        if !group.iter().any(|field| present(at(&v, field))) {
            return Err(ContribError::invalid(format!(
                "provide at least one of: {}",
                group.join(", ")
            )));
        }
    }
    let email = s.contributor.contact.as_deref().expect("required above");
    if email.contains(char::is_whitespace)
        || !email.split_once('@').is_some_and(|(a, b)| {
            !a.is_empty() && b.contains('.') && !b.contains('@') && b.split('.').all(|p| !p.is_empty())
        })
    {
        return Err(ContribError::invalid("contributor.contact must be an email address"));
    }
    for choice in enumerations() {
        let path = choice["path"].as_str().expect("choice path");
        let companion = choice["other_field"].as_str().expect("choice companion");
        let is_other = |v: &Value| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("other"));
        let other = at(&v, path).is_some_and(|v| match v {
            Value::Array(a) => a.iter().any(is_other),
            v => is_other(v),
        });
        if other != present(at(&v, companion)) {
            return Err(ContribError::invalid(format!(
                "{companion} is required exactly when {path} contains other"
            )));
        }
    }
    Ok(())
}
