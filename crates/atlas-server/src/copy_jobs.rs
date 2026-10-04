//! Catalog messages of the persona jobs (D39, `GET /api/{condition,gene}/{id}/jobs`), kept apart
//! from `copy.rs` like `copy_d30.rs`; fold them in later. Each message is a key, its params and the
//! English fallback rendered from the same params (D26). Keys: `jobs.label.<job>`,
//! `jobs.kind.<asset kind>`, `jobs.route.<route>`, `jobs.none.<job>`.

use atlas_core::graph::{AssetKind, Job};
use serde_json::{Value, json};

fn msg(key: String, params: Value, fallback: String) -> Value {
    json!({ "key": key, "params": params, "fallback": fallback })
}

pub fn label(job: Job) -> Value {
    let text = match job {
        Job::ModelsSamples => "Research models, samples and data you can request",
        Job::TherapyProgrammes => "Who is developing a therapy, and at which stage",
        Job::FreePapers => "Research papers you can read for free",
        Job::Funding => "Funding for research on this",
        Job::OutcomeMeasures => "How change in this condition is measured",
    };
    msg(format!("jobs.label.{}", job.as_str()), json!({}), text.into())
}

/// One plain line per item kind (asset kinds plus `grant` and `paper`).
pub fn kind(kind: &str) -> Value {
    let text = match AssetKind::parse(kind) {
        Some(k) => k.explain(),
        None => match kind {
            "grant" => "A research project that a funder is paying for; the lead organisation is who to contact.",
            "paper" => "A research paper with a legal free-to-read version.",
            _ => "A research resource.",
        },
    };
    msg(format!("jobs.kind.{kind}"), json!({}), text.into())
}

/// How to get it, in plain words.
pub fn route(route: &str, holder: Option<&str>) -> Value {
    let who = holder.unwrap_or("the holder");
    let text = match route {
        "repository_order" => format!("Order or request it from {who} through the catalogue page."),
        "request_form" => format!("Request it from {who} with the form on their page."),
        "access_committee" => format!("Apply to the data access committee ({who}); access is reviewed."),
        "apply" => format!("Apply to {who}; check the deadline and eligibility on the official page."),
        "read_free" => "Read the free full text at the link.".to_owned(),
        "contact_holder" => format!("Contact {who} through their official page."),
        "public_download" => "Download it from the official page.".to_owned(),
        _ => format!("See the official page of {who} for how to take part or ask for access."),
    };
    msg(format!("jobs.route.{route}"), json!({ "holder": who }), text)
}

/// Honest "nothing found" line: what was searched.
pub fn none(job: Job, searched: &[String]) -> Value {
    let list = if searched.is_empty() {
        "no loaded source".to_owned()
    } else {
        searched.join(", ")
    };
    let text = match job {
        Job::FreePapers => format!(
            "We found no paper with a free full text linked here. Searched: {list}. Other papers may still be free through your library or Europe PMC."
        ),
        _ => format!("We found nothing linked here yet. Searched: {list}. That does not mean nothing exists."),
    };
    msg(
        format!("jobs.none.{}", job.as_str()),
        json!({ "searched": searched }),
        text,
    )
}
