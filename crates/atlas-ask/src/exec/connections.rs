//! `connections`, `assets`, `shared_people`: who and what connects to a condition.
//!
//! Exact vs related (same rule as atlas-server `connections.rs`): a study, organisation or work
//! linked to the condition itself, or naming one of its causal genes when it has at most
//! [`MAX_CONDITION_GENES`] of them, is exact; MONDO-umbrella links and gene links of multi-gene
//! clinical entities are related.

use std::collections::HashMap;

use atlas_core::DiseaseIdx;
use atlas_core::node::{NodeKey, NodeKind};
use atlas_journeys::connections::{self as discovery, Evidence};

use super::view;
use super::{Ctx, Run};
use crate::facts::NewFact;
use crate::intent::{ASSET_KINDS, CONNECTION_KINDS};

/// One connection with the facts that justify it.
#[derive(Clone, Debug)]
pub struct Found {
    pub who: NodeKey,
    /// Card kind (`patient_group`, `trial`, `researcher`, `grant`, ...).
    pub kind: &'static str,
    pub exact: bool,
    pub level: &'static str,
    pub facts: Vec<NewFact>,
    pub score: f64,
}

/// Ask facts adapt the same classified and deduplicated evidence as the cards API.
pub fn collect(ctx: &Ctx<'_>, d: DiseaseIdx, people: bool) -> Vec<Found> {
    let c = discovery::collect(ctx.atlas, ctx.graph, d, people);
    let genes = view::causal_genes(ctx.atlas, d);
    let dis = ctx.atlas.disease_at(d);
    let mut found: Vec<_> = c
        .found
        .iter()
        .map(|f| Found {
            who: f.who,
            kind: f.kind,
            exact: f.exact,
            level: f.level.as_str(),
            score: f.score,
            facts: f
                .evidence
                .iter()
                .map(|e| match e {
                    Evidence::Edge(edge) => NewFact::edge_msg(
                        edge.id(),
                        edge.kind.as_str(),
                        view::edge_text(ctx, edge),
                        view::edge_source(ctx, edge),
                    ),
                    Evidence::Gene(gene) => {
                        let g = &genes[*gene];
                        NewFact::edge_msg(
                            &g.edge_id,
                            "observed",
                            view::gene_fact_text(ctx.atlas, d, g),
                            g.sources.join(", "),
                        )
                    }
                    Evidence::Hierarchy { ancestor, name } => NewFact::node_msg(
                        &dis.id,
                        crate::copy::msg(
                            "ask.fact.parent_condition",
                            serde_json::json!({"label": dis.name, "cid": dis.id, "anc_name": name, "anc": ancestor}),
                        ),
                        "MONDO",
                    ),
                })
                .collect(),
        })
        .collect();
    // Preserve Ask's presentation: exact connections first, then score and stable ID.
    found.sort_by(|a, b| {
        b.exact
            .cmp(&a.exact)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| ctx.graph.node_ref(a.who).id.cmp(&ctx.graph.node_ref(b.who).id))
    });
    found
}

/// Attribute fact of a graph node (what it is, status, where, official channel).
pub fn node_fact(ctx: &Ctx<'_>, k: NodeKey) -> NewFact {
    let g = ctx.graph;
    let checked = view::node_checked_on(g, k)
        .map(|d| crate::copy::msg("ask.fragment.checked_date", serde_json::json!({"d": d})))
        .unwrap_or_default();
    match k.kind {
        NodeKind::Study => {
            let s = g.study(k.idx);
            let countries = if s.countries.is_empty() {
                serde_json::Value::Null
            } else {
                crate::copy::msg(
                    "ask.fragment.countries",
                    serde_json::json!({"arg0": view::list_some(&s.countries, 6)}),
                )
            };
            let sponsor = if s.sponsor.is_empty() {
                serde_json::Value::Null
            } else {
                crate::copy::msg("ask.fragment.sponsor", serde_json::json!({"arg0": s.sponsor}))
            };
            NewFact::node_msg(&s.id, crate::copy::msg("ask.fact.study", serde_json::json!({"arg0": s.title, "arg1": s.id, "arg2": crate::copy::msg(&format!("ask.kind.{}", s.kind.as_str()), serde_json::json!({})), "arg3": s.status.to_lowercase().replace('_', " "), "sponsor": sponsor, "countries": countries, "checked": checked})), "ClinicalTrials.gov")
            .with_url(Some(view::study_contacts_url(&s.id)))
        }
        NodeKind::Organisation => {
            let o = g.org(k.idx);
            let place = o
                .country
                .as_deref()
                .map(|c| crate::copy::msg("ask.fragment.country", serde_json::json!({"c": c})))
                .unwrap_or_default();
            let about = o
                .description
                .as_deref()
                .map(|d| crate::copy::msg("ask.fragment.description", serde_json::json!({"d": d})))
                .unwrap_or_default();
            let verified = o
                .verified_on
                .as_deref()
                .map(|d| crate::copy::msg("ask.fragment.verified_date", serde_json::json!({"d": d})))
                .unwrap_or_default();
            NewFact::node_msg(&o.id, crate::copy::msg("ask.fact.organisation", serde_json::json!({"arg0": o.name, "arg1": crate::copy::msg(&format!("ask.kind.{}", o.kind.as_str()), serde_json::json!({})), "place": place, "about": about, "verified": verified})), "organisation list")
            .with_url(o.contact_url.clone().or_else(|| o.url.clone()))
        }
        NodeKind::Person => {
            let p = g.person(k.idx);
            let aff = p
                .affiliations
                .first()
                .map(|a| crate::copy::msg("ask.fragment.affiliation", serde_json::json!({"a": a})))
                .unwrap_or_default();
            NewFact::node_msg(
                &p.id,
                crate::copy::msg(
                    "ask.fact.researcher",
                    serde_json::json!({"arg0": p.name, "aff": aff, "checked": checked}),
                ),
                "PubMed / NIH RePORTER",
            )
            .with_url(Some(view::person_url(p)))
        }
        NodeKind::Grant => {
            let gr = g.grant(k.idx);
            let years = match (gr.fiscal_years.iter().min(), gr.fiscal_years.iter().max()) {
                (Some(a), Some(b)) if a != b => {
                    crate::copy::msg("ask.fragment.funded_range", serde_json::json!({"a": a, "b": b}))
                }
                (Some(a), _) => crate::copy::msg("ask.fragment.funded_year", serde_json::json!({"a": a})),
                _ => serde_json::Value::Null,
            };
            NewFact::node_msg(
                &gr.id,
                crate::copy::msg(
                    "ask.fact.grant",
                    serde_json::json!({"arg0": gr.title, "arg1": gr.id, "arg2": gr.organisation, "years": years}),
                ),
                "NIH RePORTER",
            )
            .with_url(Some(gr.url.clone()))
        }
        NodeKind::Paper => {
            let p = g.paper(k.idx);
            let year = p
                .year
                .map(|y| crate::copy::msg("ask.fragment.paper_year", serde_json::json!({"y": y})))
                .unwrap_or_default();
            let n = p.id.trim_start_matches("PMID:");
            NewFact::node_msg(
                &p.id,
                crate::copy::msg(
                    "ask.fact.paper",
                    serde_json::json!({"arg0": p.title, "arg1": p.id, "arg2": p.journal, "year": year}),
                ),
                "PubMed",
            )
            .with_url(Some(format!("https://pubmed.ncbi.nlm.nih.gov/{n}/")))
        }
        _ => NewFact::node(g.node_ref(k).id, g.node_ref(k).label, "atlas graph"),
    }
}

/// Display order for `kind=any`: what a family can act on first.
const ORDER: [&str; 10] = [
    "patient_group",
    "expert_centre",
    "registry",
    "natural_history",
    "trial",
    "observational",
    "expanded_access",
    "organisation",
    "researcher",
    "grant",
];

/// Kinds a `kind=` value selects.
fn expand(kind: &str) -> Vec<&'static str> {
    match kind {
        "any" => vec![
            "patient_group",
            "expert_centre",
            "organisation",
            "registry",
            "natural_history",
            "trial",
            "observational",
            "expanded_access",
            "researcher",
            "grant",
        ],
        "study" => vec![
            "registry",
            "natural_history",
            "trial",
            "observational",
            "expanded_access",
        ],
        other => CONNECTION_KINDS
            .iter()
            .chain(ASSET_KINDS)
            .find(|k| **k == other)
            .map(|k| vec![*k])
            .unwrap_or_default(),
    }
}

/// Emit up to `per_kind` per card kind and `total` overall; returns how many were shown.
fn emit(run: &mut Run<'_>, found: &[Found], per_kind: usize, total: usize) -> usize {
    let mut shown: HashMap<&str, usize> = HashMap::new();
    let mut n = 0;
    for f in found {
        if n == total {
            break;
        }
        let c = shown.entry(f.kind).or_default();
        if *c == per_kind {
            continue;
        }
        *c += 1;
        n += 1;
        let ctx = run.ctx;
        run.fact(node_fact(&ctx, f.who));
        for fact in &f.facts {
            run.fact(fact.clone());
        }
    }
    n
}

fn counts_note(found: &[Found], what: serde_json::Value, shown: usize) -> serde_json::Value {
    let exact = found.iter().filter(|f| f.exact).count();
    crate::copy::msg(
        "ask.note.counts",
        serde_json::json!({"arg0": found.len(), "what": what, "exact": exact, "arg1": found.len() - exact, "shown": shown}),
    )
}

pub fn connections(run: &mut Run<'_>) -> Result<(), String> {
    let d = run.condition("condition")?;
    let kind = run.slot("kind").unwrap_or_else(|| "any".into());
    let kinds = expand(&kind);
    let ctx = run.ctx;
    let mut found: Vec<Found> = collect(&ctx, d, kinds.contains(&"researcher"))
        .into_iter()
        .filter(|f| kinds.contains(&f.kind))
        .collect();
    if kind == "any" {
        // Stable: keeps exact-first and score order within a kind.
        found.sort_by_key(|f| (!f.exact, ORDER.iter().position(|k| *k == f.kind).unwrap_or(ORDER.len())));
    }
    let (per_kind, total) = if kind == "any" { (3, 14) } else { (8, 8) };
    let shown = emit(run, &found, per_kind, total);
    run.note(counts_note(
        &found,
        crate::copy::msg(&format!("ask.kind.{kind}"), serde_json::json!({})),
        shown,
    ));
    if found.is_empty() {
        super::condition::coverage_facts(run, d, &kinds);
    }
    Ok(())
}

fn current_year() -> u16 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (1970 + secs / 31_556_952) as u16
}

pub fn assets(run: &mut Run<'_>) -> Result<(), String> {
    let d = run.condition("condition")?;
    let kind = run.slot("kind").unwrap_or_else(|| "any".into());
    let status = run.slot("status").unwrap_or_else(|| "any".into());
    let mut kinds = expand(&kind);
    kinds.retain(|k| ASSET_KINDS.contains(k) && *k != "any" && *k != "study");
    let ctx = run.ctx;
    let year = current_year();
    let found: Vec<Found> = collect(&ctx, d, false)
        .into_iter()
        .filter(|f| kinds.contains(&f.kind))
        .filter(|f| {
            let (open, recruiting) = match f.who.kind {
                NodeKind::Study => {
                    let s = ctx.graph.study(f.who.idx);
                    (s.is_open(), s.is_recruiting())
                }
                _ => {
                    let last = ctx
                        .graph
                        .grant(f.who.idx)
                        .fiscal_years
                        .iter()
                        .max()
                        .copied()
                        .unwrap_or(0);
                    (last + 1 >= year, false)
                }
            };
            match status.as_str() {
                "open" => open,
                "recruiting" => recruiting,
                "closed" => !open,
                _ => true,
            }
        })
        .collect();
    let shown = emit(run, &found, 6, 12);
    run.note(counts_note(
        &found,
        crate::copy::msg(
            "ask.assets.selection",
            serde_json::json!({
                "kind": crate::copy::msg(&format!("ask.kind.{kind}"), serde_json::json!({})),
                "status": crate::copy::msg(&format!("ask.status.{status}"), serde_json::json!({}))
            }),
        ),
        shown,
    ));
    if found.is_empty() {
        super::condition::coverage_facts(run, d, &kinds);
    }
    Ok(())
}

pub fn shared_people(run: &mut Run<'_>) -> Result<(), String> {
    let a = run.condition("a")?;
    let b = run.condition("b")?;
    if a == b {
        return Err("both slots resolve to the same condition".into());
    }
    let ctx = run.ctx;
    let side = |d| -> HashMap<NodeKey, Found> {
        collect(&ctx, d, true)
            .into_iter()
            .filter(|f| f.kind == "researcher")
            .map(|f| (f.who, f))
            .collect()
    };
    let (pa, pb) = (side(a), side(b));
    let mut shared: Vec<(&Found, &Found)> = pa.values().filter_map(|x| pb.get(&x.who).map(|y| (x, y))).collect();
    shared.sort_by(|x, y| {
        (y.0.score + y.1.score)
            .total_cmp(&(x.0.score + x.1.score))
            .then_with(|| x.0.who.cmp(&y.0.who))
    });
    for (x, y) in shared.iter().take(6) {
        run.fact(node_fact(&ctx, x.who));
        for f in x.facts.iter().take(2).chain(y.facts.iter().take(2)) {
            run.fact(f.clone());
        }
    }
    let (la, lb) = (
        ctx.atlas.disease_at(a).name.clone(),
        ctx.atlas.disease_at(b).name.clone(),
    );
    run.note(crate::copy::msg("ask.note.shared_researchers", serde_json::json!({"arg0": pa.len(), "la": la, "arg1": pb.len(), "lb": lb, "arg2": shared.len(), "arg3": shared.len().min(6)})));
    Ok(())
}
