//! Catalog messages pending web integration in copy-audit deliverable 2.
use serde_json::{Value, json};

fn value(p: &Value, key: &str) -> String {
    match &p[key] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

pub fn msg(key: &str, p: Value) -> Value {
    let fallback = text(key, &p);
    json!({"key": key, "params": p, "fallback": fallback})
}

pub fn text(key: &str, p: &Value) -> String {
    match key {
        "contribute.check.sample_found" => format!(
            "The checked sample contains evidence about {}. A reviewer needs to verify it.",
            value(p, "kind")
        ),
        "contribute.check.sample_not_found" => format!(
            "The checked sample contains no evidence about {}. The full source may contain it.",
            value(p, "kind")
        ),
        "contribute.check.known_source_matched" => {
            "This source URL is already listed. Review it before adding a duplicate.".into()
        }
        "contribute.check.known_source_unmatched" => "No matching source URL was found in the lists checked.".into(),
        "contribute.check.description_only" => {
            "A reviewer needs to identify this source from your description before checking it.".into()
        }
        "contribute.check.snapshots" => format!(
            "{} of {} response snapshots were retained for rechecking. Each response lists its storage status.",
            value(p, "retained"),
            value(p, "total")
        ),
        "contribute.check.duplicate_source_matched" => {
            "Another submission names this source. A reviewer needs to compare them.".into()
        }
        "contribute.check.duplicate_source_unmatched" => {
            "No matching source was found in pending or accepted submissions.".into()
        }
        "contribute.error.invalid" => "Check the required fields and their values, then submit again.".into(),
        "contribute.error.not_found" => "This contribution was not found.".into(),
        "contribute.error.transition" => format!(
            "Cannot {} a contribution with status {}.",
            value(p, "action"),
            value(p, "from")
        ),
        "contribute.error.blocked" => "Resolve the blocking check results before accepting this contribution.".into(),
        "contribute.error.stale" => "This contribution changed. Reload it and try again.".into(),
        "contribute.error.unauthorized" => "Sign in as a reviewer to continue.".into(),
        "contribute.error.forbidden" => "You do not have permission for this action.".into(),
        "contribute.error.rate_limited" => {
            format!("Too many submissions. Try again in {} seconds.", value(p, "seconds"))
        }
        "contribute.error.internal" => "We could not process this contribution. Try again later.".into(),
        "contribute.check.relationship_unmapped" => {
            "A reviewer needs to check the relationship you described before adding a connection.".into()
        }
        "contribute.check.no_url" => "No link was given.".into(),
        "contribute.check.url_ok" => "The page opens.".into(),
        "contribute.check.url_http_error" => format!(
            "The page answered with an error (HTTP {arg0}).",
            arg0 = value(p, "arg0")
        ),
        "contribute.check.url_blocked" => "This address points to a private network and was not opened.".into(),
        "contribute.check.url_invalid" => "This is not a web address we can open.".into(),
        "contribute.check.url_too_large" => "The page is too large to check automatically.".into(),
        "contribute.check.url_unreachable" => "The page could not be reached right now.".into(),
        "contribute.check.no_quote" => "No quote was given.".into(),
        "contribute.check.quote_no_url" => "A quote was given without a link, so it can't be checked.".into(),
        "contribute.check.quote_found" => "The quoted words appear on the page.".into(),
        "contribute.check.quote_not_found" => {
            "The quoted words were not found in the text checked. A reviewer can check the page itself.".into()
        }
        "contribute.check.quote_unchecked" => {
            "The page could not be read as text, so the quote was not checked.".into()
        }
        "contribute.check.identity_resolved" => format!("Recorded entry: {arg0}.", arg0 = value(p, "arg0")),
        "contribute.check.identity_kind_mismatch" => format!(
            "{arg0} is recorded as a {arg1}; this field expects a {arg2}.",
            arg0 = value(p, "arg0"),
            arg1 = value(p, "arg1"),
            arg2 = value(p, "arg2")
        ),
        "contribute.check.identity_unknown_id" => format!("No entry was found for ID {id}.", id = value(p, "id")),
        "contribute.check.identity_ambiguous" => format!(
            "\"{label}\" matches several entries; a reviewer will pick one.",
            label = value(p, "label")
        ),
        "contribute.check.identity_candidates" => format!(
            "\"{label}\" has no exact name match. Similar entries need review.",
            label = value(p, "label")
        ),
        "contribute.check.identity_new" => format!(
            "\"{label}\" has no recorded match. A reviewer will check whether to add it.",
            label = value(p, "label")
        ),
        "contribute.check.identity_not_found" => format!(
            "\"{label}\" was not found in the records checked.",
            label = value(p, "label")
        ),
        "contribute.check.edge_missing" => "The connection could not be read.".into(),
        "contribute.check.edge_found" => "This connection is recorded.".into(),
        "contribute.check.reviewed_connection" => "The connection was added by a reviewed contribution.".into(),
        "contribute.check.connection_not_recorded" => "This connection was not found in the records checked.".into(),
        "contribute.check.duplicate_unchecked" => "Duplicates are checked once the condition or gene is found.".into(),
        "contribute.check.duplicate_curated" => "This connection is already recorded from a curated source.".into(),
        "contribute.check.related_curated" => "These entries already have a different recorded connection.".into(),
        "contribute.check.conflict_contribution" => "Another contribution says this connection is wrong.".into(),
        "contribute.check.no_duplicate" => "No matching report was found in the contribution queue.".into(),
        "contribute.check.duplicate_pending" => "Others have reported the same thing.".into(),
        "contribute.check.conflict_curated" => {
            "This disputes a connection from a curated source; a reviewer compares both sources.".into()
        }
        "contribute.check.disputed_suggestion" => {
            "This disputes a connection that another contribution added or suggested.".into()
        }
        "contribute.check.contact_same" => "This contact is already recorded.".into(),
        "contribute.check.contact_differs" => "The submitted contact differs from the recorded contact.".into(),
        "contribute.check.contact_already_reported" => "Others have reported the same contact problem.".into(),
        "contribute.check.duplicate_contribution" => {
            "This connection was already added by a reviewed contribution.".into()
        }
        "contribute.check.connection_awaiting_review" => {
            "Someone else suggested the same connection; it is waiting for review.".into()
        }
        "contribute.check.new_connection" => "No matching connection was found in the records checked.".into(),
        _ => panic!("unknown copy key: {key}"),
    }
}
