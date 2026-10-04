//! Shared research questions (D27): explicitly labelled *hypotheses* that link two or more
//! communities through a sourced shared pathway, with the evidence for and against, the deciding
//! experiment (`atlas_analytics::evidence::propose_shared_mechanism`), who could run it (bridging
//! researchers, shared studies/registries) and provenance.
//!
//! Candidates are the condition's `related()` neighbours whose mechanism is strong (shared
//! compatible gene or process). Each candidate pair gets a proposal from the evidence review of
//! the selection; pairs that land on the same pathway are merged into one question, so a question
//! names every community it would compound across. Pairs without a positive shared pathway are
//! not questions (the proposal itself calls that a coverage gap, never "no shared mechanism").
//!
//! Rank: communities × evidence strength, where strength is the mean heuristic support score of
//! the review edges behind the question (0..1, *not* a probability).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use atlas_analytics::evidence::{self, FindingKind, Proposal, Relation, Review, Statement};
use atlas_analytics::{CounterexampleKind, RelatedReport, SimilarityIndex, Verdict};
use atlas_core::node::{NodeKey, NodeRef};
use atlas_core::{Atlas, Graph};
use serde_json::{Value, json};

use crate::connections::{self, Connections};

/// Neighbours examined per condition.
const NEIGHBOURS: usize = 20;
/// Review year for the recency component of the evidence policy.
const AS_OF_YEAR: u16 = 2026;
pub const ACTIVITY: &str = "activity:shared-research-questions-v1";

use crate::copy::sentence;

fn cite_id(s: &Statement) -> String {
    format!("stmt:{}", s.id)
}

/// Source records behind statements (D15 shape: url, locator, retrieval, sha256).
fn source_records(stmts: &[&Statement]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for s in stmts {
        for c in &s.citations {
            let id = format!("{}#{}", c.entity.id, c.locator);
            if !seen.insert(id.clone()) {
                continue;
            }
            out.push(json!({
                "id": cite_id(s), "record": id, "source": c.entity.file, "url": c.entity.url,
                "locator": c.locator, "retrieved_on": c.entity.retrieved_at.as_deref().map(crate::nodes::day),
                "sha256": c.entity.sha256, "version": c.entity.version, "licence": c.entity.licence,
                "references": c.references, "tier": tier(s), "statement": s.raw_value, "edge_id": s.id,
                "kind": s.kind,
            }));
        }
    }
    out
}

fn tier(s: &Statement) -> &'static str {
    use evidence::Tier::*;
    match s.tier {
        Expert | Curated => "high",
        Cohort | AuthorStatement => "moderate",
        Computational | Unrated => "low",
    }
}

/// One candidate pair's proposal and the pathway it lands on.
struct Pair {
    partner: String,
    proposal: Proposal,
    pathway: String,
    pathway_label: String,
    related_score: f64,
}

fn pathway_of<'a>(review: &'a Review, p: &Proposal) -> Option<&'a Statement> {
    p.statement_ids.iter().find_map(|id| {
        review
            .statements
            .iter()
            .find(|s| &s.id == id && s.relation == Relation::Pathway)
    })
}

/// "Name via GENE" → "Name".
fn pathway_name(raw: &str) -> String {
    raw.rsplit_once(" via ").map_or(raw, |(n, _)| n).to_owned()
}

fn strength(review: &Review, ids: &BTreeSet<&str>) -> f64 {
    let scores: Vec<f64> = review
        .edges
        .iter()
        .filter(|e| e.confidence.statement_ids.iter().any(|i| ids.contains(i.as_str())))
        .map(|e| f64::from(e.confidence.support_score) / 100.0)
        .collect();
    if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    }
}

/// Exact studies of a kind that hold data or participants (assets a question can reuse).
const ASSET_KINDS: [&str; 5] = [
    "registry",
    "natural_history",
    "observational",
    "trial",
    "expanded_access",
];

fn community(atlas: &Atlas, conn: &Connections) -> Value {
    let groups = conn
        .found
        .iter()
        .filter(|f| f.exact && f.kind == "patient_group")
        .count();
    json!({
        "node": atlas.disease_ref(conn.condition),
        "genes": conn.genes.iter().map(|(_, sym, _)| sym).collect::<Vec<_>>(),
        "patient_groups": groups,
        // Family counts are not in the graph; never estimated (D21).
        "families": Value::Null,
    })
}

/// Researchers and assets found (exactly) for at least one community; bridges (≥2) first.
fn who_could_run(atlas: &Atlas, graph: &Graph, conns: &[&Connections]) -> (Vec<Value>, Vec<Value>) {
    let mut by: HashMap<NodeKey, (&'static str, Vec<NodeRef>, f64)> = HashMap::new();
    for c in conns {
        for f in c.found.iter().filter(|f| f.exact) {
            if f.kind != "researcher" && !ASSET_KINDS.contains(&f.kind) {
                continue;
            }
            let e = by.entry(f.who).or_insert((f.kind, Vec::new(), 0.0));
            e.1.push(atlas.disease_ref(c.condition));
            e.2 = e.2.max(f.score);
        }
    }
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by(|a, b| {
        (b.1.1.len(), b.1.2)
            .partial_cmp(&(a.1.1.len(), a.1.2))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| graph.node_ref(a.0).id.cmp(&graph.node_ref(b.0).id))
    });
    let (mut people, mut assets) = (Vec::new(), Vec::new());
    for (key, (kind, communities, _)) in rows {
        let node = graph.node_ref(key);
        let bridging = communities.len() >= 2;
        if kind == "researcher" {
            if people.len() < 6 {
                people.push(json!({ "person": node, "communities": communities, "works": [], "bridging": bridging }));
            }
        } else if assets.len() < 8 {
            let s = graph.study(key.idx);
            assets.push(json!({
                "id": s.id, "title": s.title, "type": kind, "status": s.status, "sponsor": s.sponsor,
                "covers": communities, "bridging": bridging,
                "url": format!("https://clinicaltrials.gov/study/{}", s.id), "sources": [],
            }));
        }
    }
    (people, assets)
}

/// Statements of the involved conditions with a contradiction/context finding, as "against".
fn against(review: &Review, ids: &BTreeSet<String>, report: &RelatedReport, partners: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for f in &review.findings {
        if !matches!(
            f.kind,
            FindingKind::DirectContradiction | FindingKind::ContextDifference | FindingKind::ScopeReview
        ) {
            continue;
        }
        let involved: Vec<&Statement> = review
            .statements
            .iter()
            .filter(|s| f.statements.contains(&s.id) && ids.contains(&s.subject))
            .collect();
        if involved.is_empty() {
            continue;
        }
        let kind = serde_json::to_value(&f.kind).unwrap_or(Value::Null);
        out.push(sentence(
            "questions.against.finding",
            json!({ "kind": kind, "explanation": f.explanation }),
            involved.iter().map(|s| cite_id(s)).collect(),
        ));
    }
    for c in &report.counterexamples {
        let hit = c.neighbour.as_ref().is_some_and(|n| partners.contains(&n.neighbour.id))
            || c.kind == CounterexampleKind::MechanismSplitWithinCondition;
        if hit {
            let key = match c.kind {
                CounterexampleKind::SameGeneDifferentMechanism => "questions.against.same_gene_different_mechanism",
                CounterexampleKind::MechanismSplitWithinCondition => "questions.against.mechanism_split",
                CounterexampleKind::LookalikeDifferentMechanism => "questions.against.lookalike",
            };
            out.push(sentence(
                key,
                json!({ "gene": c.gene, "why": crate::copy_related::counter_message(c) }),
                Vec::new(),
            ));
        }
    }
    out.push(sentence("questions.against.variant_level", json!({}), Vec::new()));
    out
}

#[allow(clippy::too_many_arguments)] // Immutable shared collector avoids rebuilding each community.
fn question(
    atlas: &Atlas,
    graph: &Graph,
    cid: &str,
    review: &Review,
    report: &RelatedReport,
    group: &[&Pair],
    extras: &Extras,
    connections: &HashMap<String, Connections>,
) -> Value {
    let first = group[0];
    let partners: Vec<String> = group.iter().map(|p| p.partner.clone()).collect();
    let mut members = vec![cid.to_owned()];
    members.extend(partners.iter().cloned());
    let member_set: BTreeSet<String> = members.iter().cloned().collect();
    let stmt_ids: BTreeSet<&str> = group
        .iter()
        .flat_map(|p| p.proposal.statement_ids.iter().map(String::as_str))
        .collect();
    let stmts: Vec<&Statement> = review
        .statements
        .iter()
        .filter(|s| stmt_ids.contains(s.id.as_str()))
        .collect();
    let conns: Vec<&Connections> = members.iter().filter_map(|m| connections.get(m)).collect();
    let labels: Vec<String> = conns.iter().map(|c| c.label.clone()).collect();
    let strength = strength(review, &stmt_ids);
    let (researchers, assets) = who_could_run(atlas, graph, &conns);
    let comms: Vec<(NodeRef, Vec<String>)> = conns
        .iter()
        .map(|c| {
            (
                atlas.disease_ref(c.condition),
                c.genes.iter().map(|g| g.1.clone()).collect(),
            )
        })
        .collect();
    let people = extras
        .people
        .map_or_else(Vec::new, |pf| crate::bridges::for_question(pf, &comms, 6));
    let drugs = extras
        .drugs
        .map_or_else(Vec::new, |df| crate::drugs::for_question(df, &comms, cid, 8));
    let genes: Vec<String> = comms.iter().flat_map(|c| c.1.clone()).collect();
    let variants: BTreeMap<String, Value> = extras.data.map_or_else(BTreeMap::new, |d| {
        genes
            .iter()
            .filter_map(|g| crate::clinvar::gene_summary(d, g).map(|v| (g.clone(), v)))
            .collect()
    });
    let communities: Vec<Value> = conns
        .iter()
        .map(|c| {
            let mut v = community(atlas, c);
            v["variants"] = json!(c.genes.iter().filter_map(|g| variants.get(&g.1)).collect::<Vec<_>>());
            v
        })
        .collect();
    let mut unknown: Vec<Value> = Vec::new();
    unknown.extend(variants.values().map(crate::clinvar::coverage_sentence));
    let mut sources = source_records(&stmts);
    sources.extend(variants.iter().map(|(g, v)| {
        json!({ "id": format!("clinvar:{g}"), "source": v["source"]["file"], "url": v["source"]["url"],
                "retrieved_on": v["source"]["retrieved_at"], "sha256": v["source"]["sha256"], "tier": "high",
                "statement": crate::copy_extra::msg("questions.source.clinvar", json!({"gene": g, "variants": v["variants"]}))["fallback"],
                "statement_msg": crate::copy_extra::msg("questions.source.clinvar", json!({"gene": g, "variants": v["variants"]})) })
    }));
    let (model_counts, models) = extras.data.map_or((Value::Null, Vec::new()), |d| {
        crate::models::for_genes(d, graph, &genes, 2)
    });
    let for_: Vec<Value> = stmts
        .iter()
        .map(|s| {
            let label = atlas
                .disease_idx(&s.subject)
                .map_or(s.subject.clone(), |d| atlas.disease_at(d).name.clone());
            let rel = serde_json::to_value(&s.relation).unwrap_or(Value::Null);
            sentence(
                "questions.for.statement",
                json!({ "condition": label, "relation": rel, "statement": s.raw_value, "kind": s.kind }),
                vec![cite_id(s)],
            )
        })
        .collect();
    let all_cites: Vec<String> = stmts.iter().map(|s| cite_id(s)).collect();
    let p = &first.proposal;
    // Short labels: each community's gene symbols (first 3 shown, "+N more"); full names in params.
    let mut symbols: Vec<String> = Vec::new();
    for c in &conns {
        for (_, sym, _) in c.genes.iter().take(2) {
            if !symbols.contains(sym) {
                symbols.push(sym.clone());
            }
        }
    }
    let shown: Vec<String> = symbols.iter().take(3).cloned().collect();
    let more = symbols.len().saturating_sub(shown.len());
    // Plain words first (D13): the process as an everyday phrase, the scientific term one tap down.
    let process = crate::copy::process(&first.pathway, &first.pathway_label);
    let hypothesis = sentence(
        "questions.hypothesis.shared_process",
        json!({ "genes": shown, "more": more, "process": process, "pathway": first.pathway_label, "conditions": labels }),
        all_cites.clone(),
    );
    let why = sentence(
        "questions.why.shared_process",
        json!({ "communities": conns.len(), "more": more, "process": process }),
        all_cites.clone(),
    );
    let pair_labels: Vec<String> = conns
        .iter()
        .take(1)
        .map(|c| c.label.clone())
        .chain(
            conns
                .iter()
                .filter(|c| atlas.disease_at(c.condition).id == first.partner)
                .map(|c| c.label.clone()),
        )
        .collect();
    let actionable = u8::from(assets.iter().any(|a| a["bridging"] == true))
        + u8::from(people.iter().any(|x| x["identity"] == "exact"));
    // This endpoint only emits positive shared-pathway proposals. Compose its reviewed
    // procedure from structured inputs; do not pass English proposal prose as parameters.
    let mut must: Vec<Value> = ["annotation", "models", "plan", "effects"]
        .iter()
        .map(|part| crate::copy_extra::msg(&format!("questions.validate.{part}"), json!({})))
        .collect();
    if p.must_validate.len() > 4 {
        must.push(crate::copy_extra::msg("questions.validate.findings", json!({})));
    }
    let design: Vec<Value> = ["assay", "controls", "rescue", "strata"]
        .iter()
        .map(|part| {
            crate::copy_extra::msg(
                &format!("questions.design.{part}"),
                json!({"pathway": first.pathway_label}),
            )
        })
        .collect();
    json!({
        "id": format!("{cid}~{}", first.pathway),
        "kind": "shared_mechanism",
        "edge_kind": "hypothesis",
        "origin": "api",
        "status": "proposed",
        "hypothesis": hypothesis,
        "why_it_matters": why,
        "details": { "conditions": labels, "genes": symbols },
        "shared": sentence("questions.shared.process", json!({ "process": process }), all_cites.clone()),
        "term": sentence("questions.term.pathway", json!({ "pathway": first.pathway_label, "id": first.pathway }), all_cites.clone()),
        "communities": communities,
        "for": for_,
        "against": against(review, &member_set, report, &partners),
        "unknown": must.iter().map(|m| sentence("questions.unknown.validate", json!({ "text": m }), Vec::new())).chain(unknown).collect::<Vec<_>>(),
        "experiment": sentence("questions.experiment", json!({ "question": crate::copy_extra::msg("questions.proposal.question", json!({"conditions": pair_labels, "pathway": first.pathway_label})), "conditions": pair_labels, "pathway": first.pathway_label }),
            all_cites.clone()),
        "experiment_design": design.iter().map(|m| &m["fallback"]).collect::<Vec<_>>(), "experiment_design_msg": design,
        "experiment_note": sentence("questions.experiment_note", json!({ "support": crate::copy_extra::msg("questions.proposal.support", json!({})), "challenge": crate::copy_extra::msg("questions.proposal.challenge", json!({})) }), Vec::new()),
        "assets": assets,
        "people": people,
        "researchers": researchers,
        "drugs": drugs,
        "models": models,
        "model_counts": model_counts,
        "sources": sources,
        "evidence": { "for_edge_ids": stmt_ids, "related_scores": group.iter().map(|p| json!({ "partner": p.partner, "score": p.related_score })).collect::<Vec<_>>() },
        "rank": { "communities": conns.len(), "strength": strength, "actionable": actionable,
                  "score": conns.len() as f64 * strength * (1.0 + 0.5 * actionable as f64),
                  "note": crate::copy_extra::msg("questions.rank_note", json!({}))["fallback"], "note_msg": crate::copy_extra::msg("questions.rank_note", json!({})) },
        "provenance": {
            "activity": ACTIVITY, "policy_version": p.policy_version,
            "proposals": group.iter().map(|p| &p.proposal.activity).collect::<Vec<_>>(),
            "review": { "id": review.activity.id, "counts": review.activity.counts },
        },
        "checked_on": crate::nodes::day(review.activity.ended_at.as_deref().unwrap_or("")),
    })
}

/// Generic anchors that read as misleading for these communities (coordinator review).
const GENERIC: [&str; 4] = [
    "R-HSA-422356", // Regulation of insulin secretion
    "R-HSA-181429", // Serotonin Neurotransmitter Release Cycle
    "R-HSA-112310", // Neurotransmitter release cycle
    "GO:0072659",   // protein localization to plasma membrane
];
/// A process annotated to more genes than this is too broad to anchor a question.
const MAX_PROCESS_GENES: usize = 60;

#[derive(Default)]
struct Specificity(HashMap<String, Option<(usize, f64, bool)>>);

/// Reactome names that read as misleading anchors (toxin pathways, transmitter-specific cycles).
const GENERIC_NAMES: [&str; 3] = ["Toxicity of", "Release Cycle", "insulin secretion"];

impl Specificity {
    /// (genes under, IC) of a process, `None` when unknown.
    /// (genes under, IC, generic name) of a process.
    fn of(&mut self, index: &SimilarityIndex, id: &str) -> Option<(usize, f64, bool)> {
        *self.0.entry(id.to_owned()).or_insert_with(|| {
            let kind = if id.starts_with("GO:") {
                atlas_core::mechanism::ProcessKind::GoBp
            } else {
                atlas_core::mechanism::ProcessKind::Reactome
            };
            let po = index.mechanism_data().processes(kind);
            po.get(id).map(|p| {
                let generic = GENERIC_NAMES.iter().any(|n| po.name(p).contains(n));
                (po.genes_under(p).len(), po.ic(p), generic)
            })
        })
    }
}

/// Same positive-pathway rule as the evidence proposal.
fn positive_pathway(s: &Statement) -> bool {
    use atlas_core::node::EdgeKind;
    s.relation == Relation::Pathway
        && s.value == evidence::Value::Present
        && !matches!(s.curation, evidence::Curation::Refuted | evidence::Curation::Disputed)
        && s.kind != EdgeKind::Hypothesis
        && (s.kind != EdgeKind::Extracted || s.quote.as_ref().is_some_and(|q| !q.trim().is_empty()))
        && !s.citations.is_empty()
}

/// The most specific shared, non-generic process of a pair (GO-BP preferred at similar IC).
fn best_shared_pathway(
    review: &Review,
    index: &SimilarityIndex,
    spec: &mut Specificity,
    a: &str,
    b: &str,
) -> Option<String> {
    let objs = |d: &str| -> BTreeSet<String> {
        review
            .statements
            .iter()
            .filter(|s| s.subject == d && positive_pathway(s))
            .map(|s| s.object.clone())
            .collect()
    };
    let (oa, ob) = (objs(a), objs(b));
    let mut best: Option<((bool, f64), String)> = None;
    for id in oa.intersection(&ob) {
        if GENERIC.contains(&id.as_str()) {
            continue;
        }
        let Some((genes, ic, generic)) = spec.of(index, id) else {
            continue;
        };
        if generic || genes == 0 || genes > MAX_PROCESS_GENES {
            continue;
        }
        // GO-BP first (specific biological processes), then the highest IC.
        let key = (id.starts_with("GO:"), ic);
        if best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, id.clone()));
        }
    }
    best.map(|b| b.1)
}

/// The pair's proposal inputs, anchored on `keep`. The proposal reads statements and
/// their findings, not edge assessments or unrelated communities. The full review is
/// retained separately for question evidence, confidence, and counterexamples.
fn restrict(review: &Review, a: &str, b: &str, keep: &str) -> Review {
    let statements: Vec<_> = review
        .statements
        .iter()
        .filter(|s| (s.subject == a || s.subject == b) && (s.relation != Relation::Pathway || s.object == keep))
        .cloned()
        .collect();
    let ids: BTreeSet<_> = statements.iter().map(|s| s.id.as_str()).collect();
    let findings = review
        .findings
        .iter()
        .filter(|f| f.statements.iter().any(|id| ids.contains(id.as_str())))
        .cloned()
        .collect();
    Review {
        policy_version: review.policy_version.clone(),
        activity: review.activity.clone(),
        statements,
        edges: Vec::new(),
        findings,
        limitations: review.limitations.clone(),
    }
}

/// Studies linked to more conditions/genes than this are umbrellas, not a specific shared asset.
const MAX_STUDY_LINKS: usize = 300;
/// Shared-asset partners examined (most similar first).
const MAX_ASSET_PARTNERS: usize = 25;

/// Conditions that share an exact study/registry with `d` (a reusable asset is a reason to ask
/// whether a mechanism is shared): study → condition, or study → gene → G2P-curated condition.
fn asset_partners(atlas: &Atlas, graph: &Graph, index: &SimilarityIndex, conn: &Connections) -> Vec<String> {
    use atlas_core::graph::Relation as G;
    let d = conn.condition;
    let own = atlas.disease_at(d).id.clone();
    let mut shared: BTreeMap<String, usize> = BTreeMap::new();
    for f in conn.found.iter().filter(|f| f.exact && ASSET_KINDS.contains(&f.kind)) {
        let sid = graph.node_ref(f.who).id;
        let links: Vec<_> = graph
            .incident(&sid)
            .filter(|i| i.outgoing && matches!(i.edge.relation, G::StudiesCondition | G::NamesGene))
            .collect();
        if links.len() > MAX_STUDY_LINKS {
            continue;
        }
        let mut here = BTreeSet::new();
        for i in links {
            match i.edge.relation {
                G::StudiesCondition => {
                    if let Some(x) = atlas.disease_idx(i.other) {
                        here.insert(atlas.disease_at(x).id.clone());
                    }
                }
                _ => {
                    let sym = crate::nodes::any_ref(atlas, graph, i.other).map_or(i.other.to_owned(), |r| r.label);
                    for c in index.gene_mechanisms(&sym).curations {
                        here.extend(c.conditions.into_iter().map(|n| n.id));
                    }
                }
            }
        }
        here.remove(&own);
        for p in here {
            *shared.entry(p).or_default() += 1;
        }
    }
    // Most similar condition first (a mechanism question needs more than a shared study).
    let sim = |id: &str| atlas.disease_idx(id).map_or(0.0, |x| index.explain(d, x).score);
    let mut v: Vec<_> = shared.into_iter().map(|(p, n)| (sim(&p), p, n)).collect();
    v.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.1.cmp(&b.1))
    });
    v.into_iter().take(MAX_ASSET_PARTNERS).map(|x| x.1).collect()
}

/// Optional caches read at request time (each `None` when its file is absent).
pub struct Extras<'a> {
    pub people: Option<&'static crate::bridges::People>,
    pub drugs: Option<&'static crate::drugs::Drugs>,
    pub data: Option<&'a std::path::Path>,
}

impl<'a> Extras<'a> {
    pub fn load(data: &'a std::path::Path) -> Self {
        Self {
            people: crate::bridges::load(data),
            drugs: crate::drugs::load(data),
            data: Some(data),
        }
    }
}

/// All questions for a condition, ranked.
pub fn for_condition(
    atlas: &Atlas,
    graph: &Graph,
    index: &SimilarityIndex,
    extras: &Extras,
    cid: &str,
) -> Result<Vec<Value>, String> {
    let report = index.related(cid, NEIGHBOURS).map_err(|e| e.to_string())?;
    // One traversal per community, shared by asset discovery and every pathway question.
    let mut conns = HashMap::new();
    let mut candidates: Vec<(String, f64)> = report
        .items
        .iter()
        .filter(|r| matches!(r.verdict, Verdict::SameMechanism | Verdict::MechanismOnly))
        .map(|r| (r.neighbour.id.clone(), r.score))
        .collect();
    if let Some(d) = atlas.disease_idx(cid) {
        let own = connections::collect(atlas, graph, d);
        for p in asset_partners(atlas, graph, index, &own) {
            if !candidates.iter().any(|c| c.0 == p) {
                let score = atlas.disease_idx(&p).map_or(0.0, |pd| index.explain(d, pd).score);
                candidates.push((p, score));
            }
        }
        conns.insert(cid.to_owned(), own);
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut selection = vec![cid.to_owned()];
    selection.extend(candidates.iter().map(|c| c.0.clone()));
    let review = atlas_analytics::mechanism_statements::review(index, Some(&selection), AS_OF_YEAR)?;
    let mut pairs = Vec::new();
    let mut spec = Specificity::default();
    for (partner, score) in candidates {
        // Anchor on the most specific shared process (never a generic one), then let the
        // evidence layer derive the experiment from exactly that pathway.
        let Some(anchor) = best_shared_pathway(&review, index, &mut spec, cid, &partner) else {
            continue;
        };
        let reduced = restrict(&review, cid, &partner, &anchor);
        let Ok(proposal) = evidence::propose_shared_mechanism(&reduced, cid, &partner) else {
            continue;
        };
        let Some(pw) = pathway_of(&reduced, &proposal) else {
            continue;
        };
        pairs.push(Pair {
            partner,
            pathway: pw.object.clone(),
            pathway_label: pathway_name(&pw.raw_value),
            proposal,
            related_score: score,
        });
    }
    let mut groups: BTreeMap<&str, Vec<&Pair>> = BTreeMap::new();
    for p in &pairs {
        groups.entry(p.pathway.as_str()).or_default().push(p);
        if !conns.contains_key(&p.partner)
            && let Some(d) = atlas.disease_idx(&p.partner)
        {
            conns.insert(p.partner.clone(), connections::collect(atlas, graph, d));
        }
    }
    let mut out: Vec<Value> = groups
        .values()
        .map(|g| question(atlas, graph, cid, &review, &report, g, extras, &conns))
        // ≥2 communities and at least one cited supporting edge.
        .filter(|q| {
            q["communities"].as_array().is_some_and(|c| c.len() >= 2)
                && q["sources"]
                    .as_array()
                    .is_some_and(|s| s.iter().any(|x| x["edge_id"].is_string()))
        })
        .collect();
    out.sort_by(|a, b| {
        let s = |v: &Value| v["rank"]["score"].as_f64().unwrap_or(0.0);
        s(b).partial_cmp(&s(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

// --- HTTP -----------------------------------------------------------------------------------

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::routes::{ApiError, AppState, internal, not_found};

type QuestionResult = Result<std::sync::Arc<Vec<Value>>, String>;
type QuestionReceiver = tokio::sync::watch::Receiver<Option<QuestionResult>>;

struct CachedQuestions {
    result: QuestionReceiver,
    touched: u64,
}

#[derive(Default)]
struct QuestionEntries {
    by_condition: HashMap<String, CachedQuestions>,
    clock: u64,
}

/// Results belong to one immutable AppState, never to a process-global condition id.
/// At most 64 condition sets are retained; at most two expensive builds run together.
/// Pending entries cannot be evicted, so concurrent misses share one build, including
/// when the visitor who initiated it navigates away. Runtime privacy filtering still
/// runs over every response; no suppression state is cached here.
pub struct QuestionCache {
    entries: std::sync::Mutex<QuestionEntries>,
    capacity: usize,
    workers: std::sync::Arc<tokio::sync::Semaphore>,
}

#[derive(Debug)]
enum CacheError {
    Busy,
    Failed(String),
}

impl Default for QuestionCache {
    fn default() -> Self {
        Self::new(64, 2)
    }
}

impl QuestionCache {
    fn new(capacity: usize, workers: usize) -> Self {
        Self {
            entries: Default::default(),
            capacity: capacity.max(1),
            workers: std::sync::Arc::new(tokio::sync::Semaphore::new(workers.max(1))),
        }
    }

    async fn get_or_compute<F>(
        self: &std::sync::Arc<Self>,
        cid: String,
        build: F,
    ) -> Result<std::sync::Arc<Vec<Value>>, CacheError>
    where
        F: FnOnce() -> Result<Vec<Value>, String> + Send + 'static,
    {
        let (mut receiver, start) = {
            let mut entries = self.entries.lock().map_err(|e| CacheError::Failed(e.to_string()))?;
            entries.clock = entries.clock.saturating_add(1);
            let touched = entries.clock;
            if let Some(hit) = entries.by_condition.get_mut(&cid) {
                hit.touched = touched;
                (hit.result.clone(), None)
            } else {
                if entries.by_condition.len() >= self.capacity {
                    let oldest_ready = entries
                        .by_condition
                        .iter()
                        .filter(|(_, entry)| entry.result.borrow().is_some())
                        .min_by_key(|(_, entry)| entry.touched)
                        .map(|(id, _)| id.clone());
                    let Some(oldest) = oldest_ready else {
                        return Err(CacheError::Busy);
                    };
                    entries.by_condition.remove(&oldest);
                }
                let (sender, receiver) = tokio::sync::watch::channel(None);
                entries.by_condition.insert(
                    cid.clone(),
                    CachedQuestions {
                        result: receiver.clone(),
                        touched,
                    },
                );
                (receiver, Some(sender))
            }
        };
        if let Some(sender) = start {
            let cache = self.clone();
            // The job owns its lifecycle independently of the initiating HTTP request.
            tokio::spawn(async move {
                let result = match cache.workers.clone().acquire_owned().await {
                    Ok(_permit) => tokio::task::spawn_blocking(build)
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                        .map(std::sync::Arc::new),
                    Err(e) => Err(e.to_string()),
                };
                if result.is_err() {
                    // All existing waiters receive the error; the next request can retry.
                    if let Ok(mut entries) = cache.entries.lock() {
                        entries.by_condition.remove(&cid);
                    }
                }
                sender.send_replace(Some(result));
            });
        }
        loop {
            let result = receiver.borrow().clone();
            if let Some(result) = result {
                return result.map_err(CacheError::Failed);
            }
            receiver
                .changed()
                .await
                .map_err(|e| CacheError::Failed(e.to_string()))?;
        }
    }
}

async fn compute(s: &AppState, id: &str) -> Result<Result<(String, std::sync::Arc<Vec<Value>>), Response>, ApiError> {
    let d = crate::nodes::condition(s.atlas(), id).map_err(not_found)?;
    let cid = s.atlas().disease_at(d).id.clone();
    let unavailable = |status: &str| {
        let msg = crate::copy_extra::msg(&format!("api.questions.{status}"), json!({}));
        Ok(Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "detail": msg["fallback"], "detail_msg": msg, "status": status })),
        )
            .into_response()))
    };
    match s.related.get() {
        None => return unavailable("warming_up"),
        Some(Err(_)) => return unavailable("unavailable"),
        Some(Ok(_)) => {}
    }
    let s2 = s.clone();
    let c2 = cid.clone();
    let qs = match s
        .questions
        .get_or_compute(cid.clone(), move || {
            let Some(Ok(index)) = s2.related.get() else {
                return Err("similarity index unavailable".to_string());
            };
            for_condition(s2.atlas(), &s2.graph, index, &Extras::load(&s2.data), &c2)
        })
        .await
    {
        Ok(qs) => qs,
        Err(CacheError::Busy) => {
            return unavailable("busy");
        }
        Err(CacheError::Failed(e)) => return Err(internal(e)),
    };
    Ok(Ok((cid, qs)))
}

/// `GET /api/condition/{id}/questions`
#[derive(serde::Deserialize)]
pub struct QuestionParams {
    limit: Option<usize>,
}

pub async fn condition_questions(
    State(s): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(p): axum::extract::Query<QuestionParams>,
) -> Result<Response, ApiError> {
    let (cid, ranked) = match compute(&s, &id).await? {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let condition = s.atlas().disease_idx(&cid).map(|d| s.atlas().disease_ref(d));
    let mut qs = ranked.as_ref().clone();
    // Cross-community questions retain their candidates, but a condition's model results must
    // not silently substitute another gene's model for the diagnosis the person searched.
    let causal: std::collections::HashSet<String> = s
        .atlas()
        .disease_idx(&cid)
        .map(|d| {
            crate::nodes::causal_genes(s.atlas(), d)
                .into_iter()
                .map(|(_, symbol, _)| symbol)
                .collect()
        })
        .unwrap_or_default();
    scope_models(&mut qs, &causal);
    let total = qs.len();
    qs.truncate(p.limit.unwrap_or(3).clamp(1, 50));
    Ok(Json(json!({
        "condition": condition, "count": qs.len(), "total": total, "questions": qs,
        "note": crate::copy_extra::msg("questions.list_note", json!({}))["fallback"],
        "note_msg": crate::copy_extra::msg("questions.list_note", json!({})),
    }))
    .into_response())
}

fn scope_models(qs: &mut [Value], causal: &std::collections::HashSet<String>) {
    for q in qs {
        if let Some(models) = q["models"].as_array_mut() {
            let (exact, related): (Vec<_>, Vec<_>) = std::mem::take(models)
                .into_iter()
                .partition(|m| m["gene"].as_str().is_some_and(|g| causal.contains(g)));
            *models = exact;
            q["related_models"] = json!(related);
        }
    }
}

/// `GET /api/questions/{qid}` (`{condition id}~{pathway id}`).
pub async fn question_by_id(State(s): State<AppState>, Path(qid): Path<String>) -> Result<Response, ApiError> {
    let (cid, _) = qid
        .split_once('~')
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.question_id", json!({}))))?;
    let (canonical, ranked) = match compute(&s, cid).await? {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let mut qs = ranked.as_ref().clone();
    let causal = crate::nodes::causal_genes(
        s.atlas(),
        s.atlas().disease_idx(&canonical).expect("resolved condition"),
    )
    .into_iter()
    .map(|(_, symbol, _)| symbol)
    .collect();
    scope_models(&mut qs, &causal);
    qs.into_iter()
        .find(|q| q["id"].as_str() == Some(qid.as_str()))
        .map(|q| Json(q.clone()).into_response())
        .ok_or_else(|| not_found(crate::copy_extra::msg("api.error.question_unknown", json!({"id": qid}))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_core::DiseaseIdentity;
    use atlas_core::disease::Disease;
    use atlas_core::evidence::GeneLink;
    use atlas_core::graph::{GraphData, GraphEdge, LinkLevel, RecordHash, SourceRecord, Study};
    use atlas_core::mechanism::{HgncGene, MechanismData};
    use atlas_core::node::EdgeKind;
    use atlas_core::provenance::{ActivityIdx, Locator, Provenance, RecordRef, SourceEntity};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    #[tokio::test]
    async fn concurrent_question_requests_share_one_build_and_complete_ranking() {
        let cache = Arc::new(QuestionCache::new(2, 1));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let first_cache = cache.clone();
        let first_calls = calls.clone();
        let first = tokio::spawn(async move {
            first_cache
                .get_or_compute("condition".into(), move || {
                    first_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(vec![json!({ "rank": 3 }), json!({ "rank": 2 }), json!({ "rank": 1 })])
                })
                .await
        });
        start.await.unwrap();
        let other_cache = cache.clone();
        let other = tokio::spawn(async move {
            other_cache
                .get_or_compute("condition".into(), || panic!("duplicate computation"))
                .await
        });
        tokio::task::yield_now().await;
        release.send(()).unwrap();
        let first = first.await.unwrap().unwrap();
        let other = other.await.unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &other));
        assert_eq!(
            first.as_ref(),
            &[json!({ "rank": 3 }), json!({ "rank": 2 }), json!({ "rank": 1 })]
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelling_the_initiating_request_does_not_cancel_shared_work() {
        let cache = Arc::new(QuestionCache::new(1, 1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let first_cache = cache.clone();
        let request = tokio::spawn(async move {
            first_cache
                .get_or_compute("condition".into(), move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(vec![json!("finished")])
                })
                .await
        });
        start.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        let other_cache = cache.clone();
        let other = tokio::spawn(async move {
            other_cache
                .get_or_compute("condition".into(), || panic!("restarted after navigation"))
                .await
        });
        release.send(()).unwrap();
        assert_eq!(other.await.unwrap().unwrap().as_ref(), &[json!("finished")]);
    }

    #[tokio::test]
    async fn failed_question_builds_can_be_retried() {
        let cache = Arc::new(QuestionCache::new(1, 1));
        assert!(
            matches!(cache.get_or_compute("condition".into(), || Err("fixture failure".into())).await,
            Err(CacheError::Failed(e)) if e == "fixture failure")
        );
        assert_eq!(
            cache
                .get_or_compute("condition".into(), || Ok(vec![json!("retry")]))
                .await
                .unwrap()
                .as_ref(),
            &[json!("retry")]
        );
    }

    #[tokio::test]
    async fn question_cache_evicts_least_recent_ready_entry_at_capacity() {
        let cache = Arc::new(QuestionCache::new(2, 1));
        cache.get_or_compute("a".into(), || Ok(vec![])).await.unwrap();
        cache.get_or_compute("b".into(), || Ok(vec![])).await.unwrap();
        cache.get_or_compute("a".into(), || panic!("cache hit")).await.unwrap();
        cache.get_or_compute("c".into(), || Ok(vec![])).await.unwrap();
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.by_condition.len(), 2);
        assert!(entries.by_condition.contains_key("a"));
        assert!(entries.by_condition.contains_key("c"));
        assert!(!entries.by_condition.contains_key("b"));
    }

    #[tokio::test]
    async fn pending_question_work_is_never_evicted_for_a_new_miss() {
        let cache = Arc::new(QuestionCache::new(1, 1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let first_cache = cache.clone();
        let first = tokio::spawn(async move {
            first_cache
                .get_or_compute("a".into(), move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(vec![])
                })
                .await
        });
        start.await.unwrap();
        assert!(matches!(
            cache.get_or_compute("b".into(), || panic!("capacity exceeded")).await,
            Err(CacheError::Busy)
        ));
        assert_eq!(cache.entries.lock().unwrap().by_condition.len(), 1);
        release.send(()).unwrap();
        first.await.unwrap().unwrap();
        cache.get_or_compute("b".into(), || Ok(vec![])).await.unwrap();
    }

    #[tokio::test]
    async fn identical_condition_ids_in_different_states_do_not_share_questions() {
        let first = Arc::new(QuestionCache::new(1, 1));
        let second = Arc::new(QuestionCache::new(1, 1));
        let a = first
            .get_or_compute("condition".into(), || Ok(vec![json!("atlas-a")]))
            .await
            .unwrap();
        let b = second
            .get_or_compute("condition".into(), || Ok(vec![json!("atlas-b")]))
            .await
            .unwrap();
        assert_eq!(a.as_ref(), &[json!("atlas-a")]);
        assert_eq!(b.as_ref(), &[json!("atlas-b")]);
    }

    #[test]
    fn condition_models_do_not_substitute_a_neighbours_gene() {
        let mut qs = vec![json!({"models":[{"id":"fixture:direct", "gene":"STXBP1"},
            {"id":"fixture:neighbour", "gene":"SNAP25"}, {"id":"fixture:unknown"}]})];
        scope_models(&mut qs, &std::collections::HashSet::from(["STXBP1".to_owned()]));
        assert_eq!(qs[0]["models"].as_array().unwrap().len(), 1);
        assert_eq!(qs[0]["related_models"].as_array().unwrap().len(), 2);
        assert_eq!(qs[0]["models"][0]["id"], "fixture:direct");
    }

    fn q_models(qs: &[Value]) -> usize {
        qs.iter().map(|q| q["models"].as_array().map_or(0, Vec::len)).sum()
    }

    #[test]
    fn pathway_name_strips_gene() {
        assert_eq!(
            pathway_name("SNARE complex assembly via STXBP1"),
            "SNARE complex assembly"
        );
        assert_eq!(pathway_name("plain"), "plain");
    }

    #[test]
    fn pair_projection_preserves_the_complete_proposal_and_selected_pathway() {
        let (atlas, _, mech) = question_fixture();
        let index = SimilarityIndex::new(atlas, Arc::new(mech), atlas_analytics::SimilarityParams::default());
        let selection = ["MONDO:0012812", "MONDO:0014590", "MONDO:9000001"].map(str::to_owned);
        let mut statements = atlas_analytics::mechanism_statements::review(&index, Some(&selection), AS_OF_YEAR)
            .unwrap()
            .statements;
        let base = statements
            .iter()
            .find(|s| s.subject == selection[0] && s.relation == Relation::Pathway)
            .unwrap()
            .clone();
        let mut scoped = base.clone();
        scoped.id = "fixture:scoped-present".into();
        scoped.kind = EdgeKind::Observed;
        scoped.context.variant = Some("fixture-variant-a".into());
        let mut contrary = scoped.clone();
        contrary.id = "fixture:scoped-absent".into();
        contrary.value = evidence::Value::Absent;
        let mut different_context = contrary.clone();
        different_context.id = "fixture:other-variant".into();
        different_context.context.variant = Some("fixture-variant-b".into());
        let mut wrong_scope = base.clone();
        wrong_scope.id = "fixture:other-scope".into();
        wrong_scope.value = evidence::Value::Absent;
        wrong_scope.context.source_disease = "fixture:unresolved-source-condition".into();
        let mut unsupported = base;
        unsupported.id = "fixture:unquoted-extraction".into();
        unsupported.kind = EdgeKind::Extracted;
        unsupported.quote = None;
        statements.extend([scoped, contrary, different_context, wrong_scope, unsupported]);
        let review = evidence::analyze(&statements, AS_OF_YEAR).unwrap();
        for kind in [
            FindingKind::DirectContradiction,
            FindingKind::ContextDifference,
            FindingKind::ScopeReview,
        ] {
            assert!(review.findings.iter().any(|f| f.kind == kind));
        }
        let mut specificity = Specificity::default();
        for partner in &selection[1..] {
            let anchor = best_shared_pathway(&review, &index, &mut specificity, &selection[0], partner).unwrap();
            let projected = restrict(&review, &selection[0], partner, &anchor);
            // Previous behavior cloned the full review, removing only the pair's other pathways.
            let mut previous = review.clone();
            previous.statements.retain(|s| {
                !(s.relation == Relation::Pathway
                    && (s.subject == selection[0] || s.subject == *partner)
                    && s.object != anchor)
            });
            let old = evidence::propose_shared_mechanism(&previous, &selection[0], partner).unwrap();
            let new = evidence::propose_shared_mechanism(&projected, &selection[0], partner).unwrap();
            assert_eq!(serde_json::to_value(&old).unwrap(), serde_json::to_value(&new).unwrap());
            assert_eq!(pathway_of(&previous, &old), pathway_of(&projected, &new));
            assert!(projected.statements.len() < previous.statements.len());
            assert!(projected.edges.is_empty());
        }
    }

    /// Synthetic evidence exercises the A10 shape independently of external cache versions.
    /// A second pathway makes the ordering assertion meaningful (at least two questions).
    fn question_fixture() -> (Arc<Atlas>, Graph, MechanismData) {
        let mut provenance = Provenance::default();
        let source = provenance.add_entity(SourceEntity {
            id: "source:question-fixture".into(),
            file: "question-fixture.json".into(),
            url: "https://example.invalid/question-fixture".into(),
            version: Some("synthetic-v1".into()),
            retrieved_at: Some("2026-10-04T00:00:00Z".into()),
            sha256: Some("a".repeat(64)),
            ..SourceEntity::default()
        });
        let genes = [
            ("MONDO:0012812", "STXBP1", "HGNC:11444", "6812"),
            ("MONDO:0014590", "SNAP25", "HGNC:11132", "6616"),
            ("MONDO:9000001", "VAMP2", "HGNC:12643", "6844"),
        ];
        let diseases = genes
            .iter()
            .map(|(id, symbol, hgnc, _)| {
                let mut d = Disease::new(*id, ActivityIdx(0));
                d.name = format!("Synthetic {symbol} condition");
                d.rare = true;
                d.genes.push(GeneLink {
                    symbol: (*symbol).into(),
                    association: "Disease-causing germline mutation(s) (loss of function) in".into(),
                    source: "Orphanet".into(),
                    source_disease: (*id).into(),
                    pmids: vec![],
                    assessed: Some(true),
                    hgnc: Some((*hgnc).into()),
                    ncbi_gene: None,
                    record: RecordRef::record(source, *id),
                });
                d
            })
            .collect();
        let atlas = Arc::new(Atlas::new(
            vec![],
            DiseaseIdentity::default(),
            provenance.clone(),
            diseases,
        ));
        let mut mech = MechanismData::default();
        mech.provenance.add_entity(SourceEntity {
            id: "source:synthetic-reactome".into(),
            file: atlas_core::mechanism::REACTOME_GENES.into(),
            ..provenance.entity(source).clone()
        });
        for (_, symbol, hgnc, entrez) in genes {
            mech.hgnc.genes.push(HgncGene {
                hgnc_id: hgnc.into(),
                symbol: symbol.into(),
                name: symbol.into(),
                locus_group: "protein-coding gene".into(),
                entrez: Some(entrez.into()),
                groups: vec![],
                aliases: vec![],
                previous: vec![],
                record: RecordRef::record(source, symbol),
            });
        }
        let snare = mech.reactome.intern("R-HSA-9000001");
        mech.reactome.names[snare as usize] = "SNARE complex assembly".into();
        let fusion = mech.reactome.intern("R-HSA-9000002");
        mech.reactome.names[fusion as usize] = "Synaptic vesicle fusion".into();
        let background = mech.reactome.intern("R-HSA-9000003");
        mech.reactome.names[background as usize] = "Unrelated background process".into();
        mech.reactome.by_gene.extend([
            ("6812".into(), vec![snare, fusion]),
            ("6616".into(), vec![snare]),
            ("6844".into(), vec![fusion]),
            ("fixture-background-gene".into(), vec![background]),
        ]);
        mech.finish();
        mech.reactome.compute_ic();
        let mut data = GraphData {
            provenance,
            ..GraphData::default()
        };
        data.records.push(SourceRecord {
            entity: source,
            locator: Locator::Record("study".into()),
            id: "NCT01238250".into(),
            url: None,
            fetched_at: None,
            hash: RecordHash::CanonicalJson,
            sha256: [0; 32],
        });
        data.studies.push(
            serde_json::from_value::<Study>(json!({
                "id": "NCT01238250", "title": "Synthetic Searchlight study", "status": "RECRUITING",
                "kind": "observational", "phases": [], "sponsor": "Fixture sponsor", "sponsor_class": "OTHER",
                "start": "2026", "completion": "2027", "enrollment": null, "countries": ["US"],
                "interventions": [], "record": 0,
            }))
            .unwrap(),
        );
        for (condition, _, _, _) in genes {
            data.edges.push(GraphEdge {
                from: "NCT01238250".into(),
                relation: atlas_core::graph::Relation::StudiesCondition,
                to: condition.into(),
                kind: EdgeKind::Observed,
                level: LinkLevel::Curated,
                reason: "synthetic source assertion".into(),
                activity: ActivityIdx(0),
                records: vec![0],
            });
        }
        data.assets.push(atlas_core::graph::Asset {
            id: atlas_ingest::graph::model_id("CVCL:fixture"),
            label: "Synthetic model".into(),
            kind: atlas_core::graph::AssetKind::CellLine,
            category: "biolink:CellLine".into(),
            holder: None,
            holder_name: None,
            access: Default::default(),
            facts: vec![],
            verify_url: None,
            release: false,
            records: vec![0],
        });
        (atlas, Graph::new(data), mech)
    }

    /// Only the optional request-time caches use files; snapshots and the user's data are untouched.
    struct TempExtras(PathBuf);

    impl TempExtras {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "atlas-questions-fx-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let fixture = Self(dir);
            fixture.write(
                "cache/people/persons.json",
                json!({"header": {"bridges": [{
                    "identity": "exact", "person": "ORCID:0000-0002-1825-0097", "name": "Dirk Fasshauer",
                    "genes": {"STXBP1": {"paper": ["PMID:fixture1"]}, "SNAP25": {"paper": ["PMID:fixture2"]}}
                }]}}),
            );
            fixture.write(
                "cache/repurposing/opentargets.json",
                json!({"header": {}, "records": [{
                    "id": "fixture-drug", "status": "included", "relation": "clinical_indication",
                    "condition_id": "MONDO:0014590", "drug_id": "CHEMBL:fixture", "drug_name": "Synthetic drug",
                    "association_max_clinical_stage": "PHASE_2"
                }]}),
            );
            fixture.write(
                "cache/models/STXBP1.json",
                json!({"schema": "models.assets", "records": [{
                    "id": "fixture-model", "gene": "STXBP1", "model_id": "CVCL:fixture", "label": "Synthetic model",
                    "asset_type": "cell_model", "organism": {"id": "NCBITaxon:9606"}, "excluded": false
                }]}),
            );
            fixture.write(
                "cache/clinvar/STXBP1.json",
                json!({"schema": "clinvar.variants", "header": {"complete": true}, "records": [{
                    "id": "fixture-variant", "clinical_significance": "Pathogenic", "excluded": false,
                    "functional_consequences": ["synthetic functional evidence"]
                }]}),
            );
            fixture
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, file: &str, mut value: Value) {
            if value["schema"] == "models.assets" {
                value["version"] = json!(1);
                value["header"] = json!({"sha256": atlas_ingest::graph::cache::hex(
                    &atlas_ingest::graph::cache::canonical_sha256(&value["records"]))});
            }
            let path = self.0.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
    }

    impl Drop for TempExtras {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn stxbp1_snap25_is_a_question() {
        let (atlas, graph, mech) = question_fixture();
        let index = SimilarityIndex::new(
            atlas.clone(),
            Arc::new(mech),
            atlas_analytics::SimilarityParams::default(),
        );
        let data = TempExtras::new();
        let people = crate::bridges::load(data.path()).expect("readable synthetic people cache");
        let drugs = crate::drugs::load(data.path()).expect("readable synthetic drugs cache");
        let extras = Extras {
            people: Some(people),
            drugs: Some(drugs),
            data: Some(data.path()),
        };
        let qs = for_condition(&atlas, &graph, &index, &extras, "MONDO:0012812").unwrap();
        assert_eq!(qs.len(), 2, "two distinct shared pathways produce ranked questions");
        assert!(q_models(&qs) > 0, "models for the slice genes");
        let c = &qs[0]["communities"][0];
        assert_eq!(c["node"]["id"], "MONDO:0012812");
        assert!(c["variants"][0]["variants"].as_u64().unwrap() > 0);
        for q in &qs {
            for d in q["drugs"].as_array().unwrap() {
                assert_eq!(d["edge_kind"], "observed");
                assert!(!d["sentence"]["cites"].as_array().unwrap().is_empty());
            }
        }
        let q = qs
            .iter()
            .take(3)
            .find(|q| {
                q["communities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["node"]["id"] == "MONDO:0014590")
            })
            .expect("STXBP1–SNAP25 question");
        assert_eq!(q["edge_kind"], "hypothesis");
        assert!(q["communities"].as_array().unwrap().len() >= 2);
        assert!(!q["for"].as_array().unwrap().is_empty());
        assert!(!q["against"].as_array().unwrap().is_empty());
        assert!(!q["sources"].as_array().unwrap().is_empty());
        assert!(q["assets"].as_array().unwrap().iter().any(|a| a["id"] == "NCT01238250"));
        assert!(!q["drugs"].as_array().unwrap().is_empty());
        let p = q["people"].as_array().unwrap();
        assert!(
            p.iter()
                .any(|x| x["person"]["label"] == "Dirk Fasshauer" && x["identity"] == "exact")
        );
        assert!(
            p.iter()
                .all(|x| x["suggestion"]["kind"] == "hypothesis" && !x["facts"].as_array().unwrap().is_empty())
        );
        let scores: Vec<f64> = qs.iter().map(|q| q["rank"]["score"].as_f64().unwrap()).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "ranked");
    }

    #[test]
    fn shared_study_without_pathway_evidence_does_not_create_a_question() {
        let (atlas, graph, _) = question_fixture();
        let index = SimilarityIndex::new(
            atlas.clone(),
            Arc::new(MechanismData::default()),
            atlas_analytics::SimilarityParams::default(),
        );
        let extras = Extras {
            people: None,
            drugs: None,
            data: None,
        };
        assert!(
            for_condition(&atlas, &graph, &index, &extras, "MONDO:0012812")
                .unwrap()
                .is_empty()
        );
    }
}
