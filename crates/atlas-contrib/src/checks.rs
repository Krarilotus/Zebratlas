//! Auto-checks run on every submission (and on demand by a reviewer):
//!
//! - `evidence_url` / `contact_url`: the page resolves (2xx), recorded with retrieval time and SHA-256;
//! - `quote`: the quoted words appear on the evidence page;
//! - `subject` / `target`: the names resolve to graph nodes (atlas-core search), or are new;
//! - `edge`: the edge a correction or missing-evidence report is about exists;
//! - `duplicate`: the same link is already curated, accepted, or waiting in the queue;
//! - `conflict`: the contribution disputes a curated edge, or another contribution disputes it.
//!
//! Checks inform the reviewer; only a few block accepting (`blocking`): a curated or accepted
//! duplicate, an unresolved target of a new link, a correction of an edge that does not exist, and
//! URLs pointing at private addresses.

use serde_json::{Value, json};

use crate::fetch::{Fetched, Fetcher};
use crate::graph::{GraphLookup, edge_id, parse_edge_id};
use crate::model::{
    Check, CheckReport, CheckStatus, Contribution, ContributionKind, NodeHit, NodeInput, State, Submission,
};
use crate::text::{contains_quote, normalize};
use crate::util::now_rfc3339;

fn check(name: &str, status: CheckStatus, code: &str, message: Value) -> Check {
    Check {
        name: name.into(),
        status,
        code: code.into(),
        message: message["fallback"].as_str().unwrap_or_default().to_owned(),
        message_msg: message,
        blocking: false,
        detail: Value::Null,
    }
}

trait CheckExt {
    fn blocking(self) -> Self;
    fn detail(self, detail: Value) -> Self;
}

impl CheckExt for Check {
    fn blocking(mut self) -> Self {
        self.blocking = true;
        self
    }
    fn detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }
}

/// Runs every check for contribution `id`. `others` are the other contributions (any state); the
/// caller may pre-filter them, the checks only look at open and accepted ones.
pub async fn run(
    id: &str,
    sub: &Submission,
    lookup: &dyn GraphLookup,
    fetcher: &dyn Fetcher,
    others: &[Contribution],
) -> CheckReport {
    if sub.kind == ContributionKind::DataSource {
        return crate::datasource::run(id, sub, fetcher, others).await;
    }
    let mut checks = Vec::new();
    let mut fetched = Vec::new();

    // URLs: fetch both concurrently.
    let (evidence, contact) = tokio::join!(
        async {
            match &sub.evidence_url {
                Some(u) => Some(fetcher.fetch(u).await),
                None => None,
            }
        },
        async {
            match &sub.contact_url {
                Some(u) => Some(fetcher.fetch(u).await),
                None => None,
            }
        }
    );
    checks.push(url_check("evidence_url", evidence.as_ref()));
    if sub.contact_url.is_some() {
        checks.push(url_check("contact_url", contact.as_ref()));
    }
    checks.push(quote_check(sub.quote.as_deref(), evidence.as_ref()));
    fetched.extend(evidence.map(|f| f.record));
    fetched.extend(contact.map(|f| f.record));

    // Identities.
    let subject_kinds: Vec<&str> = match (sub.kind, sub.subject_kind) {
        (ContributionKind::NewLink, Some(k)) => vec![k.node_kind()],
        (ContributionKind::OutdatedContact, _) => vec!["organisation", "study", "person"],
        _ => Vec::new(),
    };
    let new_allowed = sub.kind == ContributionKind::NewLink;
    let subject = if sub.subject.is_empty() {
        None
    } else {
        let (c, hit) = resolve("subject", &sub.subject, &subject_kinds, new_allowed, lookup);
        checks.push(c);
        hit
    };
    let target = match &sub.target {
        Some(t) => {
            let (mut c, hit) = resolve("target", t, &["disease", "gene"], false, lookup);
            if sub.kind == ContributionKind::NewLink && hit.is_none() {
                c.status = CheckStatus::Fail;
                c = c.blocking();
            }
            checks.push(c);
            hit
        }
        None => None,
    };
    let relation = sub
        .relation(target.as_ref().map(|t| t.kind.as_str()))
        .map(str::to_string);
    if sub.kind == ContributionKind::NewLink && relation.is_none() {
        checks.push(
            check(
                "relationship",
                CheckStatus::Warn,
                "relationship_unmapped",
                crate::copy::msg("contribute.check.relationship_unmapped", serde_json::json!({})),
            )
            .detail(
                json!({"subject_kind_other": sub.subject_kind_other, "relationship_other": sub.relationship_other}),
            ),
        );
    }

    // The edge a correction / missing-evidence report is about.
    let live: Vec<&Contribution> = others
        .iter()
        .filter(|o| o.id != id && o.state != State::Rejected)
        .collect();
    if let Some(edge) = &sub.edge {
        checks.push(edge_check(edge, sub.kind, lookup, &live));
    }

    let ctx = Ctx {
        id,
        sub,
        subject: subject.as_ref(),
        target: target.as_ref(),
        relation: relation.as_deref(),
        lookup,
        live: &live,
    };
    checks.extend(duplicate_and_conflict(&ctx));

    CheckReport {
        checked_at: now_rfc3339(),
        agent: crate::AGENT.to_string(),
        checks,
        subject,
        target,
        relation,
        fetched,
    }
}

pub(crate) fn url_check(name: &str, fetched: Option<&Fetched>) -> Check {
    let Some(f) = fetched else {
        return check(
            name,
            CheckStatus::Skipped,
            "no_url",
            crate::copy::msg("contribute.check.no_url", serde_json::json!({})),
        );
    };
    let r = &f.record;
    let detail = json!({ "url": r.url, "status": r.status, "sha256": r.sha256, "retrieved_at": r.retrieved_at });
    match r.outcome.as_str() {
        "ok" => check(
            name,
            CheckStatus::Pass,
            "url_ok",
            crate::copy::msg("contribute.check.url_ok", serde_json::json!({})),
        )
        .detail(detail),
        "http_status" => check(
            name,
            CheckStatus::Fail,
            "url_http_error",
            crate::copy::msg(
                "contribute.check.url_http_error",
                serde_json::json!({"arg0": r.status.unwrap_or(0)}),
            ),
        )
        .detail(detail),
        "blocked" => check(
            name,
            CheckStatus::Fail,
            "url_blocked",
            crate::copy::msg("contribute.check.url_blocked", serde_json::json!({})),
        )
        .blocking()
        .detail(detail),
        "invalid_url" => check(
            name,
            CheckStatus::Fail,
            "url_invalid",
            crate::copy::msg("contribute.check.url_invalid", serde_json::json!({})),
        )
        .blocking()
        .detail(detail),
        "too_large" => check(
            name,
            CheckStatus::Warn,
            "url_too_large",
            crate::copy::msg("contribute.check.url_too_large", serde_json::json!({})),
        )
        .detail(detail),
        _ => check(
            name,
            CheckStatus::Warn,
            "url_unreachable",
            crate::copy::msg("contribute.check.url_unreachable", serde_json::json!({})),
        )
        .detail(detail),
    }
}

fn quote_check(quote: Option<&str>, evidence: Option<&Fetched>) -> Check {
    let Some(q) = quote else {
        return check(
            "quote",
            CheckStatus::Skipped,
            "no_quote",
            crate::copy::msg("contribute.check.no_quote", serde_json::json!({})),
        );
    };
    let Some(page) = evidence else {
        return check(
            "quote",
            CheckStatus::Skipped,
            "quote_no_url",
            crate::copy::msg("contribute.check.quote_no_url", serde_json::json!({})),
        );
    };
    match &page.text {
        Some(text) if contains_quote(text, q) => check(
            "quote",
            CheckStatus::Pass,
            "quote_found",
            crate::copy::msg("contribute.check.quote_found", serde_json::json!({})),
        ),
        Some(_) => check(
            "quote",
            CheckStatus::Fail,
            "quote_not_found",
            crate::copy::msg("contribute.check.quote_not_found", serde_json::json!({})),
        ),
        None => check(
            "quote",
            CheckStatus::Skipped,
            "quote_unchecked",
            crate::copy::msg("contribute.check.quote_unchecked", serde_json::json!({})),
        ),
    }
}

/// Resolves an input to one node, or explains why not.
fn resolve(
    name: &str,
    input: &NodeInput,
    kinds: &[&str],
    new_allowed: bool,
    lookup: &dyn GraphLookup,
) -> (Check, Option<NodeHit>) {
    if let Some(id) = &input.id {
        return match lookup.node(id) {
            Some(hit) if kinds.is_empty() || kinds.contains(&hit.kind.as_str()) => (
                check(
                    name,
                    CheckStatus::Pass,
                    "identity_resolved",
                    crate::copy::msg(
                        "contribute.check.identity_resolved",
                        serde_json::json!({"arg0": hit.label}),
                    ),
                )
                .detail(json!({ "id": hit.id, "kind": hit.kind })),
                Some(hit),
            ),
            Some(hit) => (
                check(
                    name,
                    CheckStatus::Warn,
                    "identity_kind_mismatch",
                    crate::copy::msg(
                        "contribute.check.identity_kind_mismatch",
                        serde_json::json!({"arg0": hit.label, "arg1": hit.kind, "arg2": kinds.join(" or ")}),
                    ),
                )
                .detail(json!({ "id": hit.id, "kind": hit.kind, "expected": kinds })),
                Some(hit),
            ),
            None => (
                check(
                    name,
                    CheckStatus::Warn,
                    "identity_unknown_id",
                    crate::copy::msg("contribute.check.identity_unknown_id", serde_json::json!({"id": id})),
                )
                .detail(json!({ "id": id })),
                None,
            ),
        };
    }
    let label = input.label.as_deref().unwrap_or_default();
    let hits = lookup.search(label, kinds, 5);
    let exact: Vec<&NodeHit> = hits.iter().filter(|h| h.exact).collect();
    let candidates = |hs: &[NodeHit]| -> Value {
        hs.iter()
            .map(|h| json!({ "id": h.id, "kind": h.kind, "label": h.label, "exact": h.exact }))
            .collect()
    };
    match exact.as_slice() {
        [one] => (
            check(
                name,
                CheckStatus::Pass,
                "identity_resolved",
                crate::copy::msg(
                    "contribute.check.identity_resolved",
                    serde_json::json!({"arg0": one.label}),
                ),
            )
            .detail(json!({ "id": one.id, "kind": one.kind })),
            Some((*one).clone()),
        ),
        [_, _, ..] => (
            check(
                name,
                CheckStatus::Warn,
                "identity_ambiguous",
                crate::copy::msg(
                    "contribute.check.identity_ambiguous",
                    serde_json::json!({"label": label}),
                ),
            )
            .detail(json!({ "candidates": candidates(&hits) })),
            None,
        ),
        [] if !hits.is_empty() => (
            check(
                name,
                CheckStatus::Warn,
                "identity_candidates",
                crate::copy::msg(
                    "contribute.check.identity_candidates",
                    serde_json::json!({"label": label}),
                ),
            )
            .detail(json!({ "candidates": candidates(&hits) })),
            None,
        ),
        [] if new_allowed => (
            check(
                name,
                CheckStatus::Pass,
                "identity_new",
                crate::copy::msg("contribute.check.identity_new", serde_json::json!({"label": label})),
            )
            .detail(json!({ "label": label })),
            None,
        ),
        [] => (
            check(
                name,
                CheckStatus::Warn,
                "identity_not_found",
                crate::copy::msg(
                    "contribute.check.identity_not_found",
                    serde_json::json!({"label": label}),
                ),
            ),
            None,
        ),
    }
}

fn edge_check(edge: &str, kind: ContributionKind, lookup: &dyn GraphLookup, live: &[&Contribution]) -> Check {
    let Some((from, _, _)) = parse_edge_id(edge) else {
        return check(
            "edge",
            CheckStatus::Fail,
            "edge_missing",
            crate::copy::msg("contribute.check.edge_missing", serde_json::json!({})),
        )
        .blocking();
    };
    if let Some(e) = lookup.edges_of(from).into_iter().find(|e| e.id() == edge) {
        return check(
            "edge",
            CheckStatus::Pass,
            "edge_found",
            crate::copy::msg("contribute.check.edge_found", serde_json::json!({})),
        )
        .detail(json!({ "edge": edge, "kind": e.kind, "origin": "curated" }));
    }
    if let Some(c) = live
        .iter()
        .find(|c| c.state == State::Accepted && asserted_edge(c).as_deref() == Some(edge))
    {
        return check(
            "edge",
            CheckStatus::Pass,
            "edge_found",
            crate::copy::msg("contribute.check.reviewed_connection", serde_json::json!({})),
        )
        .detail(json!({ "edge": edge, "origin": "user_asserted", "contribution": c.id }));
    }
    let c = check(
        "edge",
        CheckStatus::Fail,
        "edge_missing",
        crate::copy::msg("contribute.check.connection_not_recorded", serde_json::json!({})),
    )
    .detail(json!({ "edge": edge }));
    // A missing-evidence report may name a connection that should exist; a correction can't.
    if kind == ContributionKind::Correction {
        c.blocking()
    } else {
        c
    }
}

struct Ctx<'a> {
    id: &'a str,
    sub: &'a Submission,
    subject: Option<&'a NodeHit>,
    target: Option<&'a NodeHit>,
    relation: Option<&'a str>,
    lookup: &'a dyn GraphLookup,
    live: &'a [&'a Contribution],
}

/// Subject key of a contribution: the resolved id, else the normalised label.
fn subject_key(c: &Contribution) -> Option<String> {
    c.checks
        .as_ref()
        .and_then(|r| r.subject.as_ref())
        .map(|h| h.id.clone())
        .or_else(|| c.submission.subject.key())
}

fn target_key(c: &Contribution) -> Option<String> {
    c.checks
        .as_ref()
        .and_then(|r| r.target.as_ref())
        .map(|h| h.id.clone())
        .or_else(|| c.submission.target.as_ref().and_then(NodeInput::key))
}

/// The edge a new link asserts (`from|relation|to`), once its target is resolved. A subject that is
/// new to the atlas becomes the node `contrib:<id>`.
pub fn asserted_edge(c: &Contribution) -> Option<String> {
    let report = c.checks.as_ref()?;
    let relation = report.relation.as_deref()?;
    let to = &report.target.as_ref()?.id;
    let from = report
        .subject
        .as_ref()
        .map_or_else(|| new_node_id(&c.id), |s| s.id.clone());
    Some(edge_id(&from, relation, to))
}

pub fn new_node_id(contribution: &str) -> String {
    format!("contrib:{contribution}")
}

fn ids(cs: &[&&Contribution]) -> Value {
    cs.iter().map(|c| json!({ "id": c.id, "state": c.state })).collect()
}

fn duplicate_and_conflict(x: &Ctx<'_>) -> Vec<Check> {
    let sub = x.sub;
    let me_subject = x.subject.map(|h| h.id.clone()).or_else(|| sub.subject.key());
    let me_target = x
        .target
        .map(|h| h.id.clone())
        .or_else(|| sub.target.as_ref().and_then(NodeInput::key));
    let same_kind: Vec<&&Contribution> = x.live.iter().filter(|o| o.kind == sub.kind).collect();
    let mut out = Vec::new();

    match sub.kind {
        ContributionKind::DataSource | ContributionKind::Other => {}
        ContributionKind::NewLink => {
            let (Some(relation), Some(target)) = (x.relation, x.target) else {
                out.push(check(
                    "duplicate",
                    CheckStatus::Skipped,
                    "duplicate_unchecked",
                    crate::copy::msg("contribute.check.duplicate_unchecked", serde_json::json!({})),
                ));
                return out;
            };
            if let Some(subject) = x.subject {
                let curated = x.lookup.edges_of(&subject.id);
                if let Some(e) = curated
                    .iter()
                    .find(|e| e.relation == relation && e.other(&subject.id) == target.id)
                {
                    out.push(
                        check(
                            "duplicate",
                            CheckStatus::Fail,
                            "duplicate_curated",
                            crate::copy::msg("contribute.check.duplicate_curated", serde_json::json!({})),
                        )
                        .blocking()
                        .detail(json!({ "edge": e.id(), "kind": e.kind })),
                    );
                    return out;
                }
                if let Some(e) = curated.iter().find(|e| e.other(&subject.id) == target.id) {
                    out.push(
                        check(
                            "duplicate",
                            CheckStatus::Warn,
                            "related_curated",
                            crate::copy::msg("contribute.check.related_curated", serde_json::json!({})),
                        )
                        .detail(json!({ "edge": e.id(), "kind": e.kind })),
                    );
                }
            }
            let dupes: Vec<&&Contribution> = same_kind
                .iter()
                .filter(|o| {
                    o.submission.relation(Some(&target.kind)) == Some(relation)
                        && subject_key(o) == me_subject
                        && target_key(o) == me_target
                })
                .copied()
                .collect();
            push_duplicates(&mut out, &dupes);
            // Conflicts: someone says this exact edge is wrong.
            let mine = edge_id(
                &x.subject.map_or_else(|| new_node_id(x.id), |s| s.id.clone()),
                relation,
                &target.id,
            );
            let disputes: Vec<&&Contribution> = x
                .live
                .iter()
                .filter(|o| {
                    o.kind == ContributionKind::Correction && o.submission.edge.as_deref() == Some(mine.as_str())
                })
                .collect();
            if !disputes.is_empty() {
                out.push(
                    check(
                        "conflict",
                        CheckStatus::Warn,
                        "conflict_contribution",
                        crate::copy::msg("contribute.check.conflict_contribution", serde_json::json!({})),
                    )
                    .detail(json!({ "contributions": ids(&disputes) })),
                );
            }
        }
        ContributionKind::Correction | ContributionKind::MissingEvidence => {
            let dupes: Vec<&&Contribution> = same_kind
                .iter()
                .filter(|o| match &sub.edge {
                    Some(e) => o.submission.edge.as_ref() == Some(e),
                    None => subject_key(o) == me_subject && target_key(o) == me_target,
                })
                .copied()
                .collect();
            // Several people reporting the same problem is a signal, not a blocker.
            if dupes.is_empty() {
                out.push(check(
                    "duplicate",
                    CheckStatus::Pass,
                    "no_duplicate",
                    crate::copy::msg("contribute.check.no_duplicate", serde_json::json!({})),
                ));
            } else {
                out.push(
                    check(
                        "duplicate",
                        CheckStatus::Warn,
                        "duplicate_pending",
                        crate::copy::msg("contribute.check.duplicate_pending", serde_json::json!({})),
                    )
                    .detail(json!({ "contributions": ids(&dupes) })),
                );
            }
            if sub.kind == ContributionKind::Correction
                && let Some(edge) = &sub.edge
            {
                let curated = parse_edge_id(edge)
                    .and_then(|(from, _, _)| x.lookup.edges_of(from).into_iter().find(|e| &e.id() == edge));
                if let Some(e) = curated {
                    out.push(
                        check(
                            "conflict",
                            CheckStatus::Warn,
                            "conflict_curated",
                            crate::copy::msg("contribute.check.conflict_curated", serde_json::json!({})),
                        )
                        .detail(json!({ "edge": edge, "kind": e.kind })),
                    );
                }
                let asserting: Vec<&&Contribution> = x
                    .live
                    .iter()
                    .filter(|o| {
                        o.kind == ContributionKind::NewLink && asserted_edge(o).as_deref() == Some(edge.as_str())
                    })
                    .collect();
                if !asserting.is_empty() {
                    out.push(
                        check(
                            "conflict",
                            CheckStatus::Warn,
                            "conflict_contribution",
                            crate::copy::msg("contribute.check.disputed_suggestion", serde_json::json!({})),
                        )
                        .detail(json!({ "contributions": ids(&asserting) })),
                    );
                }
            }
        }
        ContributionKind::OutdatedContact => {
            if let (Some(subject), Some(new)) = (x.subject, &sub.contact_url) {
                let current: Vec<&String> = subject.urls.iter().collect();
                if current.iter().any(|u| same_url(u, new)) {
                    out.push(
                        check(
                            "conflict",
                            CheckStatus::Warn,
                            "contact_same",
                            crate::copy::msg("contribute.check.contact_same", serde_json::json!({})),
                        )
                        .detail(json!({ "current": current })),
                    );
                } else {
                    out.push(
                        check(
                            "conflict",
                            CheckStatus::Pass,
                            "contact_differs",
                            crate::copy::msg("contribute.check.contact_differs", serde_json::json!({})),
                        )
                        .detail(json!({ "current": current })),
                    );
                }
            }
            let dupes: Vec<&&Contribution> = same_kind
                .iter()
                .filter(|o| subject_key(o) == me_subject)
                .copied()
                .collect();
            if dupes.is_empty() {
                out.push(check(
                    "duplicate",
                    CheckStatus::Pass,
                    "no_duplicate",
                    crate::copy::msg("contribute.check.no_duplicate", serde_json::json!({})),
                ));
            } else {
                out.push(
                    check(
                        "duplicate",
                        CheckStatus::Warn,
                        "duplicate_pending",
                        crate::copy::msg("contribute.check.contact_already_reported", serde_json::json!({})),
                    )
                    .detail(json!({ "contributions": ids(&dupes) })),
                );
            }
        }
    }
    out
}

fn push_duplicates(out: &mut Vec<Check>, dupes: &[&&Contribution]) {
    let accepted: Vec<&&Contribution> = dupes.iter().filter(|o| o.state == State::Accepted).copied().collect();
    if !accepted.is_empty() {
        out.push(
            check(
                "duplicate",
                CheckStatus::Fail,
                "duplicate_contribution",
                crate::copy::msg("contribute.check.duplicate_contribution", serde_json::json!({})),
            )
            .blocking()
            .detail(json!({ "contributions": ids(&accepted) })),
        );
    } else if !dupes.is_empty() {
        out.push(
            check(
                "duplicate",
                CheckStatus::Warn,
                "duplicate_pending",
                crate::copy::msg("contribute.check.connection_awaiting_review", serde_json::json!({})),
            )
            .detail(json!({ "contributions": ids(dupes) })),
        );
    } else if !out.iter().any(|c| c.name == "duplicate") {
        out.push(check(
            "duplicate",
            CheckStatus::Pass,
            "no_duplicate",
            crate::copy::msg("contribute.check.new_connection", serde_json::json!({})),
        ));
    }
}

/// Same page, ignoring scheme, `www.`, case of the host and a trailing slash.
pub fn same_url(a: &str, b: &str) -> bool {
    fn key(u: &str) -> String {
        match url::Url::parse(u) {
            Ok(p) => {
                let host = p
                    .host_str()
                    .unwrap_or_default()
                    .trim_start_matches("www.")
                    .to_ascii_lowercase();
                let path = p.path().trim_end_matches('/');
                let query = p.query().map(|q| format!("?{q}")).unwrap_or_default();
                format!("{host}{path}{query}")
            }
            Err(_) => normalize(u),
        }
    }
    key(a) == key(b)
}
