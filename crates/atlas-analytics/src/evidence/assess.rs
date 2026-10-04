use super::*;
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{Activity, Agent};
use std::collections::{BTreeMap, BTreeSet};

fn usable(s: &Statement) -> bool {
    !matches!(s.curation, Curation::Refuted | Curation::Disputed) && !matches!(s.value, Value::Unknown)
}

fn complete(s: &Statement) -> bool {
    !s.citations.is_empty()
        && s.citations.iter().all(|c| {
            !c.entity.url.is_empty()
                && !c.locator.is_empty()
                && c.entity.version.as_ref().is_some_and(|v| !v.is_empty())
                && c.entity.retrieved_at.as_ref().is_some_and(|v| !v.is_empty())
                && c.entity
                    .sha256
                    .as_ref()
                    .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        })
}

fn component(parts: &mut Vec<Component>, reason: &str, points: i32) {
    parts.push(Component {
        reason: reason.into(),
        points,
    });
}

/// Score one grouped edge. `as_of_year` is explicit so reruns do not use wall-clock time.
/// Findings must be those produced by `analyze` for this edge.
pub fn confidence(statements: &[Statement], findings: &[Finding], as_of_year: u16) -> Confidence {
    let mut ordered: Vec<_> = statements.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut limitations = BTreeSet::new();
    let mut best = 0;
    let mut parts = Vec::new();
    for s in &ordered {
        let mut p = Vec::new();
        let base = match s.tier {
            Tier::Expert => 65,
            Tier::Curated => 55,
            Tier::Cohort => 45,
            Tier::AuthorStatement => 30,
            Tier::Computational => 20,
            Tier::Unrated => 10,
        };
        component(&mut p, &format!("Base evidence tier from {}", s.id), base);
        let cap = match s.kind {
            EdgeKind::Observed => 90,
            EdgeKind::Extracted => 55,
            EdgeKind::Inferred => 40,
            EdgeKind::Hypothesis => 15,
        };
        if s.curation == Curation::Reviewed {
            component(&mut p, "Reviewed curation", 5);
        }
        if s.curation == Curation::Unreviewed {
            component(&mut p, "Unreviewed curation", -10);
        }
        if let Value::Frequency { affected, examined } = s.value {
            if examined == 0 || affected > examined {
                limitations.insert(format!("{}: invalid cohort retained; contributes no support", s.id));
                continue;
            }
            component(
                &mut p,
                "Cohort precision proxy: n/(n+50), at most 10 points; not study quality",
                (10.0 * examined as f64 / (examined as f64 + 50.0)).floor() as i32,
            );
        } else {
            limitations.insert("Cohort size is unavailable or not applicable; no precision bonus".into());
        }
        match s.reviewed_year {
            Some(y) if y > as_of_year => {
                limitations.insert("Future review date; no freshness credit".into());
            }
            Some(y) if as_of_year - y <= 2 => component(&mut p, "Reviewed within two calendar years", 5),
            Some(y) if as_of_year - y > 5 => {
                component(&mut p, "More than five years since review; check for updates", -5)
            }
            None => {
                limitations.insert("Review date unknown; download date is not a review date".into());
            }
            _ => {}
        }
        if !complete(s) {
            component(&mut p, "Incomplete source provenance", -10);
            limitations.insert("Some source URL/version/retrieval time/SHA-256/locator is missing".into());
        }
        let raw_statement_score = p.iter().map(|c| c.points).sum::<i32>();
        if raw_statement_score < 0 {
            component(&mut p, "Statement floor at zero", -raw_statement_score);
        }
        let mut score = raw_statement_score.max(0);
        let mut effective_cap = cap;
        if s.value == Value::Umbrella {
            effective_cap = effective_cap.min(25);
        }
        if !usable(s) {
            effective_cap = 0;
        }
        if s.kind == EdgeKind::Extracted && s.quote.as_ref().is_none_or(|q| q.trim().is_empty()) {
            effective_cap = 0;
            limitations.insert("An extracted assertion has no quote; it contributes no support".into());
        }
        if score > effective_cap {
            component(
                &mut p,
                "Cap for assertion kind or unusable/refuted/unknown assertion",
                effective_cap - score,
            );
            score = effective_cap;
        }
        if score > best || parts.is_empty() {
            best = score;
            parts = p;
        }
    }
    // Database agreement is not independent replication. No paper-count or source-count bonus.
    let sources: BTreeSet<_> = ordered
        .iter()
        .filter(|s| usable(s))
        .flat_map(|s| &s.citations)
        .map(|c| c.entity.url.as_str())
        .collect();
    component(
        &mut parts,
        &format!(
            "{} source URLs; agreement is descriptive, independence unverified",
            sources.len()
        ),
        0,
    );
    limitations.insert(
        "Uncalibrated project heuristic; never display as probability, percent certainty, or ClinGen/GRADE class"
            .into(),
    );
    let direct = findings.iter().any(|f| f.kind == FindingKind::DirectContradiction);
    let scope = findings.iter().any(|f| f.kind == FindingKind::ScopeReview);
    let variation = findings.iter().any(|f| f.kind == FindingKind::CohortVariation);
    let penalty = if direct {
        -25
    } else if scope {
        -10
    } else if variation {
        -5
    } else {
        0
    };
    component(
        &mut parts,
        "Source agreement: direct conflict -25, unresolved scope -10, cohort variation -5 (largest only)",
        penalty,
    );
    let raw = best + penalty;
    if raw < 0 {
        component(&mut parts, "Floor at zero", -raw);
    }
    let score = raw.clamp(0, 100) as u8;
    let indirect = statements.iter().any(|s| s.value == Value::Umbrella);
    let label = if direct {
        "Conflicting reports: expert review needed"
    } else if scope {
        "Reports differ: check who and what was studied"
    } else if variation {
        "Estimates differ between reports"
    } else if findings.iter().any(|f| f.kind == FindingKind::ContextDifference) {
        "Reports describe different situations; compare them separately"
    } else if indirect {
        "Broader-condition link: eligibility unverified"
    } else if score >= 65 {
        "More supporting evidence in this atlas"
    } else if score >= 40 {
        "Some supporting evidence in this atlas"
    } else {
        "Limited or unverified support in this atlas"
    };
    Confidence {
        support_score: score,
        label: label.into(),
        components: parts,
        limitations: limitations.into_iter().collect(),
        statement_ids: ordered.iter().map(|s| s.id.clone()).collect(),
    }
}

fn mismatch(a: &Context, b: &Context) -> bool {
    [
        (&a.inheritance, &b.inheritance),
        (&a.variant, &b.variant),
        (&a.population, &b.population),
        (&a.onset, &b.onset),
        (&a.sex, &b.sex),
    ]
    .iter()
    .any(|(a, b)| a.is_some() && b.is_some() && a != b)
        || (!a.modifiers.is_empty() && !b.modifiers.is_empty() && a.modifiers != b.modifiers)
}

fn opposite(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Present, Value::Absent)
            | (Value::Absent, Value::Present)
            | (Value::LossOfFunction, Value::GainOfFunction)
            | (Value::GainOfFunction, Value::LossOfFunction)
    ) || matches!(
        (a, b),
        (Value::Frequency { affected: 1.., .. }, Value::Absent)
            | (Value::Absent, Value::Frequency { affected: 1.., .. })
    )
}

fn valid_value(v: &Value) -> bool {
    !matches!(v, Value::Frequency { affected, examined } if *examined == 0 || affected > examined)
}

fn compare(a: &Statement, b: &Statement) -> Option<Finding> {
    if !usable(a) || !usable(b) || !valid_value(&a.value) || !valid_value(&b.value) {
        return None;
    }
    if [a, b].iter().any(|s| {
        s.kind == EdgeKind::Hypothesis
            || (s.kind == EdgeKind::Extracted && s.quote.as_ref().is_none_or(|q| q.trim().is_empty()))
    }) {
        return None;
    }
    let differing_frequency = match (&a.value, &b.value) {
        (
            Value::Frequency {
                affected: a,
                examined: n,
            },
            Value::Frequency {
                affected: b,
                examined: m,
            },
        ) => (*a as u128) * (*m as u128) != (*b as u128) * (*n as u128),
        _ => false,
    };
    if !opposite(&a.value, &b.value) && !differing_frequency {
        return None;
    }
    let (kind, explanation, next_question) = if mismatch(&a.context, &b.context) {
        (
            FindingKind::ContextDifference,
            "Reports refer to different known contexts; this is not a direct contradiction.",
            "Which inheritance pattern, variant, population, age and sex apply to each report?",
        )
    } else if differing_frequency {
        (
            FindingKind::CohortVariation,
            "Reported fractions differ. They have not been pooled and may describe different cohorts; this is not proof of contradiction.",
            "Do recruitment, age, diagnostic definitions and denominators match? Are participants duplicated?",
        )
    } else if a.context == b.context
        && !a.context.source_disease.is_empty()
        && a.kind != EdgeKind::Inferred
        && b.kind != EdgeKind::Inferred
        && (a.context.variant.as_ref().is_some_and(|s| !s.trim().is_empty())
            || a.context.population.as_ref().is_some_and(|s| !s.trim().is_empty()))
    {
        (
            FindingKind::DirectContradiction,
            "Opposite assertions have matching recorded scope. Both are retained; this flags a claim conflict, not a verdict on either source.",
            "Can a curator reproduce both claims in the original records and resolve the opposite findings?",
        )
    } else {
        (
            FindingKind::ScopeReview,
            "Opposite assertions share a graph edge, but variant/cohort scope or disease identity is not established as identical.",
            "Review the original variants, inheritance and disease definitions before calling this a contradiction.",
        )
    };
    Some(Finding {
        kind,
        statements: vec![a.id.clone(), b.id.clone()],
        explanation: explanation.into(),
        next_question: next_question.into(),
    })
}

pub(crate) fn activity(as_of_year: u16) -> Activity {
    Activity {
        id: "activity:evidence-review-v1".into(),
        label: "Deterministic evidence review".into(),
        agent: Agent {
            name: "atlas-analytics/evidence".into(),
            version: POLICY_VERSION.into(),
            commit: None,
        },
        parameters: BTreeMap::from([
            ("policy".into(), POLICY_VERSION.into()),
            ("as_of_year".into(), as_of_year.to_string()),
            (
                "provenance".into(),
                "Input entities and locators carried in statements; no wall clock read".into(),
            ),
        ]),
        ..Activity::default()
    }
}

/// Review normalized graph assertions. Duplicate identical IDs are retained once and counted;
/// conflicting duplicate IDs are an error, so evidence cannot silently disappear.
pub fn analyze(statements: &[Statement], as_of_year: u16) -> Result<Review, String> {
    if as_of_year == 0 {
        return Err("as_of_year must be nonzero".into());
    }
    let input_count = statements.len();
    let mut unique = BTreeMap::new();
    for s in statements {
        if s.id.is_empty() || s.subject.is_empty() || s.object.is_empty() {
            return Err("Empty statement or endpoint ID".into());
        }
        if let Some(old) = unique.insert(s.id.clone(), s.clone())
            && old != *s
        {
            return Err(format!("Conflicting duplicate statement ID: {}", s.id));
        }
    }
    let statements: Vec<_> = unique.into_values().collect();
    let mut groups = BTreeMap::<_, Vec<&Statement>>::new();
    for s in &statements {
        groups
            .entry((s.subject.clone(), s.relation.clone(), s.object.clone()))
            .or_default()
            .push(s);
    }
    let mut findings = Vec::new();
    let mut edges = Vec::new();
    for ((subject, relation, object), items) in groups {
        let start = findings.len();
        for (i, a) in items.iter().enumerate() {
            if matches!(a.value, Value::Unknown | Value::Umbrella) {
                findings.push(Finding {
                    kind: if a.value == Value::Unknown {
                        FindingKind::UnknownMechanism
                    } else {
                        FindingKind::IndirectLink
                    },
                    statements: vec![a.id.clone()],
                    explanation: if a.value == Value::Unknown {
                        "The source does not specify an effect; this is not evidence for the opposite effect."
                    } else {
                        "A broader-condition study link does not establish eligibility for this diagnosis."
                    }
                    .into(),
                    next_question: "Check the primary mechanism evidence or the study's exact eligibility criteria."
                        .into(),
                });
            }
            for b in &items[i + 1..] {
                if let Some(f) = compare(a, b) {
                    findings.push(f);
                }
            }
        }
        let owned: Vec<_> = items.into_iter().cloned().collect();
        edges.push(EdgeAssessment {
            subject,
            relation,
            object,
            confidence: confidence(&owned, &findings[start..], as_of_year),
            finding_indexes: (start..findings.len()).collect(),
        });
    }
    let mut activity = activity(as_of_year);
    activity.count("input_assertions", input_count as u64);
    activity.count("retained_assertions", statements.len() as u64);
    activity.count(
        "deduplicated:identical-id-and-content",
        (input_count - statements.len()) as u64,
    );
    activity.count("findings", findings.len() as u64);
    // All original records remain in the caller; duplicates have identical provenance and content.
    Ok(Review {
        policy_version: POLICY_VERSION.into(),
        activity,
        statements,
        edges,
        findings,
        limitations: vec![
            "Detection covers supplied assertions only. No finding does not mean agreement or absence of uncertainty."
                .into(),
            "Disease identity is inherited from Atlas; cross-source identity/granularity differences require review."
                .into(),
        ],
    })
}
