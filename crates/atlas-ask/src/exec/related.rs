//! `related`, `diseases_with`, `path`.
//!
//! Shared-gene neighbours from the gene table; shared-symptom neighbours and the breadth-first path
//! come from atlas-journeys (the facts and wording stay here).

use std::collections::{HashMap, HashSet};

use atlas_core::node::edge_id;
use atlas_core::{DiseaseIdx, TermIdx};
use atlas_journeys::path::{Hop, MAX_DEPTH};
use atlas_journeys::symptoms::{MIN_IC, MIN_SHARED};

use super::view::{self, GENE_RELATION};
use super::{Ctx, Run, Target};
use crate::facts::NewFact;

fn gene_fact(ctx: &Ctx<'_>, d: DiseaseIdx, symbol: &str) -> Option<NewFact> {
    let l = view::gene_links(ctx.atlas, d)
        .into_iter()
        .find(|l| l.symbol == symbol)?;
    Some(NewFact::edge_msg(
        &l.edge_id,
        "observed",
        view::gene_fact_text(ctx.atlas, d, &l),
        l.sources.join(", "),
    ))
}

/// Active conditions, rare and well-annotated first.
fn rank_diseases(ctx: &Ctx<'_>, ds: &mut [DiseaseIdx]) {
    ds.sort_by_key(|&d| {
        let x = ctx.atlas.disease_at(d);
        (!x.rare, std::cmp::Reverse(x.phenotypes.len()), d)
    });
}

pub fn related(run: &mut Run<'_>) -> Result<(), String> {
    let d = run.condition("condition")?;
    let by = run.slot("by").unwrap_or_else(|| "both".into());
    let ctx = run.ctx;
    let atlas = ctx.atlas;
    let dis = atlas.disease_at(d);
    let mut via_gene: HashSet<DiseaseIdx> = HashSet::new();
    if by != "symptom" {
        let mut links = view::gene_links(atlas, d);
        links.sort_by_key(|l| !l.causal);
        for l in links.iter().take(4) {
            let Some(g) = atlas.gene(&l.symbol) else { continue };
            let mut others: Vec<DiseaseIdx> = atlas
                .gene_at(g)
                .diseases
                .iter()
                .copied()
                .filter(|&o| o != d && atlas.disease_at(o).is_active())
                .collect();
            if others.is_empty() {
                continue;
            }
            rank_diseases(&ctx, &mut others);
            run.fact(NewFact::edge_msg(
                &l.edge_id,
                "observed",
                view::gene_fact_text(atlas, d, l),
                l.sources.join(", "),
            ));
            for &o in others.iter().take(5) {
                via_gene.insert(o);
                if let Some(f) = gene_fact(&ctx, o, &l.symbol) {
                    run.fact(f);
                }
            }
            if others.len() > 5 {
                run.note(crate::copy::msg(
                    "ask.note.gene_conditions",
                    serde_json::json!({"arg0": others.len(), "arg1": l.symbol}),
                ));
            }
        }
    }
    if by != "gene" {
        let ranked = atlas_journeys::shared_symptoms(atlas, d);
        for atlas_journeys::SharedSymptoms { disease: o, terms, .. } in ranked.iter().take(5) {
            let other = atlas.disease_at(*o);
            let names: Vec<String> = terms.iter().take(4).map(|&t| atlas.hpo.term(t).name.clone()).collect();
            let message = crate::copy::msg(
                "ask.fact.related_shared_symptoms",
                serde_json::json!({"condition": other.name, "id": other.id, "count": terms.len(), "query": dis.name, "symptoms": names, "shared_gene": via_gene.contains(o)}),
            );
            run.fact(NewFact {
                id: other.id.clone(),
                edge: false,
                kind: "inferred",
                text: message["fallback"].as_str().unwrap_or_default().to_owned(),
                msg: Some(message),
                source: "atlas similarity over HPO annotations".into(),
                url: None,
            });
            for &t in terms.iter().take(2) {
                run.fact(NewFact::edge_msg(
                    view::phenotype_edge_id(atlas, *o, t),
                    "observed",
                    crate::copy::msg(
                        "ask.fact.recorded_symptom",
                        serde_json::json!({"arg0": other.name, "arg1": other.id, "arg2": view::term_text(atlas, t)}),
                    ),
                    "HPO annotations",
                ));
            }
        }
        if ranked.is_empty() {
            run.note(crate::copy::msg(
                "ask.note.symptom_threshold",
                serde_json::json!({"MIN_SHARED": MIN_SHARED, "MIN_IC": MIN_IC, "arg0": dis.name}),
            ));
        }
    }
    run.note(crate::copy::msg(
        "ask.note.cause_comparison_unavailable",
        serde_json::json!({}),
    ));
    Ok(())
}

pub fn diseases_with(run: &mut Run<'_>) -> Result<(), String> {
    let ctx = run.ctx;
    let atlas = ctx.atlas;
    if run.slot("gene").is_some() {
        let g = run.gene("gene")?;
        let gene = atlas.gene_at(g);
        let mut ds: Vec<DiseaseIdx> = gene.diseases.clone();
        rank_diseases(&ctx, &mut ds);
        for &d in ds.iter().take(12) {
            if let Some(f) = gene_fact(&ctx, d, &gene.symbol) {
                run.fact(f);
            }
        }
        run.note(crate::copy::msg(
            "ask.note.gene_results",
            serde_json::json!({"arg0": ds.len(), "arg1": gene.symbol, "arg2": ds.len().min(12)}),
        ));
        return Ok(());
    }
    if run.slot("symptom").is_some() {
        let t = run.symptom("symptom")?;
        let mut hit: HashMap<DiseaseIdx, TermIdx> = HashMap::new();
        for &sub in atlas.hpo.descendants(t) {
            for &d in atlas.by_phenotype(sub) {
                let e = hit.entry(d).or_insert(sub);
                if sub == t {
                    *e = t;
                }
            }
        }
        let term = view::term_text(atlas, t);
        run.fact(NewFact::node_msg(
            &atlas.hpo.term(t).id,
            crate::copy::msg(
                "ask.fact.symptom_conditions",
                serde_json::json!({"arg0": hit.len(), "term": term}),
            ),
            "HPO annotations",
        ));
        let mut ds: Vec<(DiseaseIdx, TermIdx, f64)> = hit
            .into_iter()
            .map(|(d, sub)| {
                let p = atlas
                    .disease_at(d)
                    .phenotype(sub)
                    .and_then(view::best_frequency)
                    .map_or(0.0, |f| f.1);
                (d, sub, p)
            })
            .collect();
        ds.sort_by(|a, b| {
            let (x, y) = (atlas.disease_at(a.0), atlas.disease_at(b.0));
            (!x.rare, a.1 != t)
                .cmp(&(!y.rare, b.1 != t))
                .then(b.2.total_cmp(&a.2))
                .then(a.0.cmp(&b.0))
        });
        for (d, sub, _) in ds.iter().take(10) {
            let x = atlas.disease_at(*d);
            run.fact(NewFact::edge_msg(
                view::phenotype_edge_id(atlas, *d, *sub),
                "observed",
                crate::copy::msg(
                    "ask.fact.recorded_symptom",
                    serde_json::json!({"arg0": x.name, "arg1": x.id, "arg2": view::term_text(atlas, *sub)}),
                ),
                "HPO annotations",
            ));
        }
        return Ok(());
    }
    let process = run.slot("process").unwrap_or_default();
    run.note(crate::copy::msg(
        "ask.note.process_unavailable",
        serde_json::json!({"process": process}),
    ));
    Ok(())
}

pub fn path(run: &mut Run<'_>) -> Result<(), String> {
    let a = run.node("from")?;
    let b = run.node("to")?;
    let ctx = run.ctx;
    let (start, goal) = (ctx.target_id(a), ctx.target_id(b));
    if matches!(a, Target::Phenotype(_)) || matches!(b, Target::Phenotype(_)) {
        return Err(
            "paths run over conditions, genes, studies, people and organisations; symptoms are not path nodes".into(),
        );
    }
    if start == goal {
        return Err("start and end are the same node".into());
    }
    let Some(hops) = atlas_journeys::shortest_path(ctx.atlas, ctx.graph, &start, &goal) else {
        run.note(crate::copy::msg(
            "ask.note.no_path",
            serde_json::json!({"MAX_DEPTH": MAX_DEPTH, "arg0": ctx.label(&start), "arg1": ctx.label(&goal)}),
        ));
        return Ok(());
    };
    for hop in &hops {
        let f = match hop {
            Hop::Graph(i) => {
                let e = ctx.graph.edge(*i);
                NewFact::edge_msg(
                    e.id(),
                    e.kind.as_str(),
                    view::edge_text(&ctx, e),
                    view::edge_source(&ctx, e),
                )
            }
            Hop::Gene(d, symbol) => match gene_fact(&ctx, *d, symbol) {
                Some(f) => f,
                None => NewFact::edge_msg(
                    edge_id(&ctx.atlas.disease_at(*d).id, GENE_RELATION, symbol),
                    "observed",
                    crate::copy::msg(
                        "ask.fact.gene_condition_link",
                        serde_json::json!({"symbol": symbol, "arg0": ctx.atlas.disease_at(*d).name}),
                    ),
                    "Orphanet / OMIM",
                ),
            },
        };
        run.fact(f);
    }
    run.note(crate::copy::msg(
        "ask.note.path_length",
        serde_json::json!({"arg0": hops.len()}),
    ));
    Ok(())
}
