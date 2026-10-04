use super::*;
use atlas_core::node::EdgeKind;
use atlas_core::provenance::SourceEntity;

fn statement(id: &str, value: Value) -> Statement {
    Statement {
        id: id.into(),
        subject: "TEST:disease".into(),
        relation: Relation::Mechanism,
        object: "TEST:gene".into(),
        value,
        raw_value: "synthetic test fixture".into(),
        kind: EdgeKind::Observed,
        tier: Tier::Curated,
        curation: Curation::Reviewed,
        source_classification: None,
        context: Context {
            source_disease: "TEST:disease".into(),
            ..Context::default()
        },
        citations: vec![Citation {
            entity: SourceEntity {
                url: "https://example.invalid/test".into(),
                version: Some("fixture".into()),
                retrieved_at: Some("2026-01-01T00:00:00Z".into()),
                sha256: Some("a".repeat(64)),
                ..SourceEntity::default()
            },
            locator: id.into(),
            references: vec![],
        }],
        reviewed_year: Some(2026),
        quote: None,
    }
}

#[test]
fn unknown_is_not_opposite_and_dn_is_not_gof() {
    let r = analyze(
        &[
            statement("a", Value::Unknown),
            statement("b", Value::LossOfFunction),
            statement("c", Value::DominantNegative),
        ],
        2026,
    )
    .unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].kind, FindingKind::UnknownMechanism);
}

#[test]
fn opposite_effects_require_matching_scope() {
    let mut a = statement("a", Value::LossOfFunction);
    let mut b = statement("b", Value::GainOfFunction);
    assert_eq!(
        analyze(&[a.clone(), b.clone()], 2026).unwrap().findings[0].kind,
        FindingKind::ScopeReview
    );
    a.context.inheritance = Some("biallelic".into());
    b.context.inheritance = Some("monoallelic".into());
    assert_eq!(
        analyze(&[a.clone(), b.clone()], 2026).unwrap().findings[0].kind,
        FindingKind::ContextDifference
    );
    b.context.inheritance = a.context.inheritance.clone();
    a.context.variant = Some("TEST:same-variant".into());
    b.context.variant = a.context.variant.clone();
    assert_eq!(
        analyze(&[a, b], 2026).unwrap().findings[0].kind,
        FindingKind::DirectContradiction
    );
}

#[test]
fn not_and_missing_are_distinct() {
    let mut a = statement("a", Value::Present);
    a.relation = Relation::Phenotype;
    let mut b = a.clone();
    b.id = "b".into();
    b.value = Value::Absent;
    assert!(analyze(&[a.clone()], 2026).unwrap().findings.is_empty());
    assert_eq!(
        analyze(&[a, b], 2026).unwrap().findings[0].kind,
        FindingKind::ScopeReview
    );
}

#[test]
fn cohorts_are_not_pooled_or_called_contradictions() {
    let a = statement(
        "a",
        Value::Frequency {
            affected: 58,
            examined: 100,
        },
    );
    let b = statement(
        "b",
        Value::Frequency {
            affected: 87,
            examined: 100,
        },
    );
    assert_eq!(
        analyze(&[a, b], 2026).unwrap().findings[0].kind,
        FindingKind::CohortVariation
    );
    let a = statement(
        "a",
        Value::Frequency {
            affected: 5,
            examined: 10,
        },
    );
    let b = statement(
        "b",
        Value::Frequency {
            affected: 10,
            examined: 20,
        },
    );
    assert!(analyze(&[a, b], 2026).unwrap().findings.is_empty());
}

#[test]
fn invalid_cohort_retained_without_support() {
    for (affected, examined) in [(1, 0), (9, 2)] {
        let r = analyze(&[statement("a", Value::Frequency { affected, examined })], 2026).unwrap();
        assert_eq!(r.statements.len(), 1);
        assert_eq!(r.edges[0].confidence.support_score, 0);
    }
}

#[test]
fn kind_caps_quotes_and_refuted_assertions() {
    for (kind, cap) in [
        (EdgeKind::Observed, 90),
        (EdgeKind::Extracted, 55),
        (EdgeKind::Inferred, 40),
        (EdgeKind::Hypothesis, 15),
    ] {
        let mut s = statement("a", Value::Present);
        s.kind = kind;
        s.quote = Some("synthetic quote".into());
        s.tier = Tier::Expert;
        assert!(confidence(&[s.clone()], &[], 2026).support_score <= cap);
        if kind == EdgeKind::Extracted {
            s.quote = None;
            assert_eq!(confidence(&[s.clone()], &[], 2026).support_score, 0);
        }
        s.curation = Curation::Refuted;
        assert_eq!(confidence(&[s], &[], 2026).support_score, 0);
    }
}

#[test]
fn deterministic_duplicates_do_not_inflate_support() {
    let a = statement("a", Value::Present);
    let b = statement("b", Value::Present);
    let r = analyze(&[b.clone(), a.clone()], 2026).unwrap();
    let q = analyze(&[a.clone(), b.clone()], 2026).unwrap();
    assert_eq!(serde_json::to_string(&r).unwrap(), serde_json::to_string(&q).unwrap());
    assert_eq!(
        r.edges[0].confidence.support_score,
        confidence(std::slice::from_ref(&a), &[], 2026).support_score
    );
    assert_eq!(analyze(&[a.clone(), a.clone()], 2026).unwrap().statements.len(), 1);
    let mut conflicting = a.clone();
    conflicting.value = Value::Absent;
    assert!(analyze(&[a, conflicting], 2026).is_err());
}

#[test]
fn score_arithmetic_and_no_date_invention() {
    let mut a = statement("a", Value::Present);
    a.reviewed_year = None;
    a.citations.clear();
    let c = confidence(&[a], &[], 2026);
    assert_eq!(
        c.support_score as i32,
        c.components.iter().map(|x| x.points).sum::<i32>()
    );
    assert!(c.limitations.iter().any(|x| x.contains("Review date unknown")));
    let mut low = statement("low", Value::Present);
    low.tier = Tier::Unrated;
    low.curation = Curation::Unreviewed;
    low.citations.clear();
    low.reviewed_year = Some(2000);
    let c = confidence(&[low], &[], 2026);
    assert_eq!(c.support_score, 0);
    assert_eq!(
        c.support_score as i32,
        c.components.iter().map(|x| x.points).sum::<i32>()
    );
}

#[test]
fn proposal_requires_shared_sourced_pathway() {
    let a = statement("a", Value::LossOfFunction);
    let mut b = a.clone();
    b.id = "b".into();
    b.subject = "TEST:other".into();
    let r = analyze(&[a.clone(), b.clone()], 2026).unwrap();
    let p = propose_shared_mechanism(&r, &a.subject, &b.subject).unwrap();
    assert!(p.statement_ids.is_empty());
    let mut a = a;
    a.relation = Relation::Pathway;
    b.relation = Relation::Pathway;
    a.value = Value::Present;
    b.value = Value::Present;
    let r = analyze(&[a.clone(), b.clone()], 2026).unwrap();
    let p = propose_shared_mechanism(&r, &a.subject, &b.subject).unwrap();
    assert_eq!(p.kind, EdgeKind::Hypothesis);
    assert_eq!(p.statement_ids.len(), 2);
    assert_eq!(p.citations.len(), 2);
    assert!(!p.would_challenge.is_empty());
}

#[test]
fn unsupported_extraction_cannot_create_conflict_and_umbrella_is_not_eligibility() {
    let a = statement("a", Value::Present);
    let mut b = statement("b", Value::Absent);
    b.kind = EdgeKind::Extracted;
    assert!(analyze(&[a, b], 2026).unwrap().findings.is_empty());
    let mut a = statement("study", Value::Umbrella);
    a.relation = Relation::StudyLink;
    let review = analyze(&[a], 2026).unwrap();
    assert!(review.edges[0].confidence.support_score <= 25);
    assert!(review.edges[0].confidence.label.contains("eligibility"));
}
