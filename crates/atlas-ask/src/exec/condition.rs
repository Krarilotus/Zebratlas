//! `resolve`, `condition_summary_facts`, `gaps`, `node_details`.

use atlas_core::DiseaseIdx;
use atlas_core::node::{NodeKind, NodeRef};
use atlas_core::search::SearchOptions;

use super::connections::{collect, node_fact};
use super::view;
use super::{Run, Target};
use crate::facts::NewFact;

fn kind_source(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Disease => "MONDO / Orphanet / OMIM",
        NodeKind::Gene => "HGNC",
        NodeKind::Phenotype => "Human Phenotype Ontology",
        _ => "atlas graph",
    }
}

pub fn resolve(run: &mut Run<'_>) -> Result<(), String> {
    let text = run.slot("text").ok_or("slot 'text' is empty")?;
    let ctx = run.ctx;
    let opts = SearchOptions {
        limit: 6,
        include_retired: false,
    };
    let mut hits: Vec<(NodeRef, String, String)> = ctx
        .atlas
        .search()
        .search(&text, opts)
        .into_iter()
        .map(|h| {
            (
                ctx.atlas.node_ref(h.node),
                h.matched.to_owned(),
                format!("{:?}", h.match_kind).to_lowercase(),
            )
        })
        .collect();
    if hits.is_empty()
        && let Some(fixed) = ctx.atlas.search().correct(&text)
    {
        run.note(crate::copy::msg(
            "ask.note.corrected_spelling",
            serde_json::json!({"fixed": fixed}),
        ));
        hits = ctx
            .atlas
            .search()
            .search(&fixed, opts)
            .into_iter()
            .map(|h| (ctx.atlas.node_ref(h.node), h.matched.to_owned(), "typo".to_owned()))
            .collect();
    }
    for a in ctx.graph.genes_by_alias(&text).take(3) {
        if let Some(g) = ctx.atlas.gene(&a.symbol) {
            let node = ctx.target_ref(Target::Gene(g));
            if !hits.iter().any(|(n, ..)| n.id == node.id) {
                hits.push((node, text.clone(), "gene alias (HGNC)".into()));
            }
        }
    }
    for (node, matched, how) in &hits {
        run.fact(NewFact::node_msg(&node.id, crate::copy::msg("ask.fact.matched_record", serde_json::json!({"text": text, "arg0": node.kind.as_str(), "arg1": node.label, "arg2": node.id, "how": how, "matched": matched})), kind_source(node.kind)));
    }
    if let Ok((d, alts)) = ctx.resolve_condition(&text) {
        let node = ctx.atlas.disease_ref(d);
        if !hits.iter().any(|(n, ..)| n.id == node.id) {
            run.fact(NewFact::node_msg(
                &node.id,
                crate::copy::msg(
                    "ask.fact.candidate_condition",
                    serde_json::json!({"text": text, "arg0": node.label, "arg1": node.id}),
                ),
                kind_source(NodeKind::Disease),
            ));
        }
        run.chip.resolved.insert(
            "text".into(),
            crate::intent::Resolved {
                node,
                alternatives: alts,
            },
        );
    }
    Ok(())
}

pub fn summary(run: &mut Run<'_>) -> Result<(), String> {
    let d = run.condition("condition")?;
    let ctx = run.ctx;
    let (atlas, graph) = (ctx.atlas, ctx.graph);
    let dis = atlas.disease_at(d);
    let (id, name) = (dis.id.clone(), dis.name.clone());
    let src = "MONDO / Orphanet / OMIM";
    if !dis.definition.trim().is_empty() {
        run.fact(NewFact::node_msg(
            &id,
            crate::copy::msg(
                "ask.fact.definition",
                serde_json::json!({"name": name, "id": id, "arg0": dis.definition.trim()}),
            ),
            "MONDO",
        ));
    }
    if dis.rare {
        run.fact(NewFact::node_msg(
            &id,
            crate::copy::msg(
                "ask.fact.rare_classification",
                serde_json::json!({"name": name, "id": id}),
            ),
            src,
        ));
    }
    let names: Vec<String> = dis
        .synonyms
        .iter()
        .map(|n| n.text.clone())
        .filter(|t| !t.eq_ignore_ascii_case(&name))
        .take(6)
        .collect();
    if !names.is_empty() {
        run.fact(NewFact::node_msg(
            &id,
            crate::copy::msg(
                "ask.fact.synonyms",
                serde_json::json!({"name": name, "arg0": names.join("; ")}),
            ),
            src,
        ));
    }
    let mut genes = view::gene_links(atlas, d);
    genes.sort_by_key(|g| !g.causal);
    for g in genes.iter().take(6) {
        run.fact(NewFact::edge_msg(
            &g.edge_id,
            "observed",
            view::gene_fact_text(atlas, d, g),
            g.sources.join(", "),
        ));
    }
    if genes.len() > 6 {
        run.note(crate::copy::msg(
            "ask.note.genes_shown",
            serde_json::json!({"arg0": genes.len()}),
        ));
    }
    for (kind, attrs) in [("inheritance", &dis.inheritance), ("onset", &dis.onset)] {
        let what = crate::copy::msg(&format!("ask.attribute.{kind}"), serde_json::json!({}));
        if !attrs.is_empty() {
            let values: Vec<&str> = attrs.keys().map(String::as_str).collect();
            run.fact(NewFact::node_msg(
                &id,
                crate::copy::msg(
                    "ask.fact.attribute",
                    serde_json::json!({"what": what, "name": name, "arg0": values.join(", ")}),
                ),
                "Orphanet / HPO annotations",
            ));
        }
    }
    let mut prev: Vec<_> = dis.prevalence.iter().collect();
    prev.sort_by_key(|p| !p.validated);
    for p in prev.iter().take(3) {
        let value = match (&p.prevalence_class, p.mean_value) {
            (Some(c), _) if !c.is_empty() && c != "Unknown" => serde_json::json!(c),
            (_, Some(m)) => crate::copy::msg("ask.prevalence.figure", serde_json::json!({"value": m})),
            _ => continue,
        };
        run.fact(NewFact::node_msg(
            &id,
            crate::copy::msg(
                "ask.fact.prevalence",
                serde_json::json!({"arg0": p.kind, "name": name, "arg1": p.geography, "value": value}),
            ),
            "Orphanet",
        ));
    }
    // Symptoms: most frequent first, then the most specific (highest information content).
    let mut ph: Vec<_> = dis
        .phenotypes
        .iter()
        .filter(|e| atlas.hpo.is_phenotype(e.term))
        .map(|e| (e, view::best_frequency(e)))
        .collect();
    ph.sort_by(|a, b| {
        let fa = a.1.as_ref().map_or(-1.0, |f| f.1);
        let fb = b.1.as_ref().map_or(-1.0, |f| f.1);
        fb.total_cmp(&fa)
            .then_with(|| atlas.hpo.ic(b.0.term).total_cmp(&atlas.hpo.ic(a.0.term)))
    });
    for (e, freq) in ph.iter().take(8) {
        let term = view::term_text(atlas, e.term);
        let text = match freq {
            Some((raw, p)) => crate::copy::msg(
                "ask.fact.symptom_frequency",
                serde_json::json!({"name": name, "term": term, "arg0": view::frequency_words(*p), "raw": raw}),
            ),
            None => crate::copy::msg(
                "ask.fact.symptom_possible",
                serde_json::json!({"name": name, "term": term}),
            ),
        };
        run.fact(NewFact::edge_msg(
            view::phenotype_edge_id(atlas, d, e.term),
            "observed",
            text,
            "HPO annotations",
        ));
    }
    if dis.phenotypes.len() > 8 {
        run.note(crate::copy::msg(
            "ask.note.symptoms_shown",
            serde_json::json!({"arg0": dis.phenotypes.len()}),
        ));
    }
    let found = collect(&ctx, d, false);
    let studies = found
        .iter()
        .filter(|f| f.exact && f.who.kind == NodeKind::Study)
        .count();
    let open = found
        .iter()
        .filter(|f| f.exact && f.who.kind == NodeKind::Study && graph.study(f.who.idx).is_open())
        .count();
    let groups = found
        .iter()
        .filter(|f| f.exact && f.who.kind == NodeKind::Organisation)
        .count();
    run.fact(NewFact::node_msg(
        &id,
        crate::copy::msg(
            "ask.fact.connections_summary",
            serde_json::json!({"studies": studies, "name": name, "open": open, "groups": groups}),
        ),
        "atlas graph (ClinicalTrials.gov, organisation list)",
    ));
    Ok(())
}

/// Coverage facts: which sources were searched for the condition, when, and what scope.
pub fn coverage_facts(run: &mut Run<'_>, d: DiseaseIdx, kinds: &[&str]) {
    let ctx = run.ctx;
    let dis = ctx.atlas.disease_at(d);
    let genes: Vec<String> = view::causal_genes(ctx.atlas, d).into_iter().map(|g| g.symbol).collect();
    let wanted = |k: &[&str]| kinds.iter().any(|x| k.contains(x));
    let mut sources: Vec<(&str, &str)> = Vec::new();
    if wanted(&["patient_group", "expert_centre", "organisation"]) {
        sources.push(("orgs", "patient groups or expert centres"));
    }
    if wanted(&[
        "trial",
        "registry",
        "natural_history",
        "observational",
        "expanded_access",
    ]) {
        sources.push(("ctgov", "studies"));
    }
    if wanted(&["researcher"]) {
        sources.push(("pubmed", "papers"));
    }
    if wanted(&["grant", "researcher"]) {
        sources.push(("reporter", "NIH research projects"));
    }
    for (source, what) in sources {
        let Some(c) = ctx.graph.coverage_of(source) else {
            run.note(crate::copy::msg(
                "ask.note.source_unloaded",
                serde_json::json!({"what": crate::copy::need(what), "source": source}),
            ));
            continue;
        };
        let when = c
            .retrieved_at
            .as_deref()
            .map(|t| {
                crate::copy::msg(
                    "ask.fragment.retrieved_date",
                    serde_json::json!({"date": &t[..t.len().min(10)]}),
                )
            })
            .unwrap_or_default();
        let text = if c.status != "loaded" {
            crate::copy::msg(
                "ask.coverage.source_unloaded",
                serde_json::json!({"arg0": c.label, "arg1": c.status}),
            )
        } else if !c.genes.is_empty() && !genes.is_empty() && !genes.iter().any(|g| c.genes.contains(g)) {
            crate::copy::msg(
                "ask.coverage.gene_subset",
                serde_json::json!({"arg0": c.label, "arg1": view::list_some(&c.genes, 8), "arg2": genes.join(", "), "what": crate::copy::need(what), "arg3": dis.name}),
            )
        } else {
            crate::copy::msg(
                "ask.coverage.none_found",
                serde_json::json!({"what": crate::copy::need(what), "arg0": dis.name, "arg1": dis.id, "arg2": c.label, "arg3": c.scope, "when": when}),
            )
        };
        run.fact(NewFact::coverage_msg(
            format!("coverage:{source}"),
            text,
            c.label.clone(),
        ));
    }
}

pub fn gaps(run: &mut Run<'_>) -> Result<(), String> {
    let d = run.condition("condition")?;
    let ctx = run.ctx;
    let dis = ctx.atlas.disease_at(d);
    let (id, name) = (dis.id.clone(), dis.name.clone());
    let found = collect(&ctx, d, true);
    let has = |kinds: &[&str], open: bool| {
        found.iter().any(|f| {
            f.exact
                && kinds.contains(&f.kind)
                && (!open || f.who.kind != NodeKind::Study || ctx.graph.study(f.who.idx).is_open())
        })
    };
    let needs: [(&[&str], bool, &str); 4] = [
        (&["patient_group"], false, "patient group"),
        (
            &[
                "trial",
                "registry",
                "natural_history",
                "observational",
                "expanded_access",
            ],
            true,
            "open study or registry",
        ),
        (&["researcher"], false, "researcher"),
        (&["grant"], false, "research project"),
    ];
    let mut missing = Vec::new();
    for (kinds, open, what) in needs {
        if has(kinds, open) {
            run.note(crate::copy::msg(
                "ask.note.exact_connection",
                serde_json::json!({"what": crate::copy::need(what)}),
            ));
        } else {
            missing.push(what);
            coverage_facts(run, d, kinds);
        }
    }
    if dis.prevalence.is_empty() {
        run.fact(NewFact::coverage_msg(
            &id,
            crate::copy::msg(
                "ask.fact.prevalence_missing",
                serde_json::json!({"name": name, "id": id}),
            ),
            "Orphanet",
        ));
    }
    if view::causal_genes(ctx.atlas, d).is_empty() {
        run.fact(NewFact::coverage_msg(
            &id,
            crate::copy::msg(
                "ask.fact.causal_gene_missing",
                serde_json::json!({"name": name, "id": id}),
            ),
            "Orphanet / OMIM",
        ));
    }
    if dis.phenotypes.is_empty() {
        run.fact(NewFact::coverage_msg(
            &id,
            crate::copy::msg("ask.fact.symptoms_missing", serde_json::json!({"name": name, "id": id})),
            "HPO annotations",
        ));
    }
    let related = found
        .iter()
        .filter(|f| !f.exact && f.who.kind == NodeKind::Organisation)
        .count();
    if missing.contains(&"patient group") && related > 0 {
        run.note(crate::copy::msg(
            "ask.note.broader_groups",
            serde_json::json!({"related": related}),
        ));
    }
    Ok(())
}

pub fn details(run: &mut Run<'_>) -> Result<(), String> {
    let t = run.node("id")?;
    let ctx = run.ctx;
    match t {
        Target::Graph(k) => {
            run.fact(node_fact(&ctx, k));
            let id = ctx.graph.node_ref(k).id;
            for (n, inc) in ctx.graph.incident(&id).enumerate() {
                if n == 10 {
                    run.note(crate::copy::msg("ask.note.links_shown", serde_json::json!({})));
                    break;
                }
                let e = inc.edge;
                run.fact(NewFact::edge_msg(
                    e.id(),
                    e.kind.as_str(),
                    view::edge_text(&ctx, e),
                    view::edge_source(&ctx, e),
                ));
            }
        }
        Target::Disease(d) => {
            let dis = ctx.atlas.disease_at(d);
            if !dis.definition.is_empty() {
                run.fact(NewFact::node_msg(
                    &dis.id,
                    crate::copy::msg(
                        "ask.fact.record_definition",
                        serde_json::json!({"arg0": dis.name, "arg1": dis.id, "arg2": dis.definition}),
                    ),
                    "MONDO",
                ));
            }
            for g in view::gene_links(ctx.atlas, d).iter().take(5) {
                run.fact(NewFact::edge_msg(
                    &g.edge_id,
                    "observed",
                    view::gene_fact_text(ctx.atlas, d, g),
                    g.sources.join(", "),
                ));
            }
        }
        Target::Gene(g) => {
            let gene = ctx.atlas.gene_at(g);
            if let Some(a) = ctx.graph.gene_alias(&gene.symbol) {
                let other: Vec<_> = a.aliases.iter().chain(&a.previous).take(6).collect();
                let text = crate::copy::msg(
                    "ask.fact.gene_identity",
                    serde_json::json!({"gene": a.symbol, "id": a.hgnc, "name": a.name, "aliases": other}),
                );
                run.fact(NewFact::node_msg(&a.hgnc, text, "HGNC"));
            }
            for &d in gene.diseases.iter().take(8) {
                if let Some(l) = view::gene_links(ctx.atlas, d)
                    .into_iter()
                    .find(|l| l.symbol == gene.symbol)
                {
                    run.fact(NewFact::edge_msg(
                        &l.edge_id,
                        "observed",
                        view::gene_fact_text(ctx.atlas, d, &l),
                        l.sources.join(", "),
                    ));
                }
            }
            if gene.diseases.len() > 8 {
                run.note(crate::copy::msg(
                    "ask.note.conditions_shown",
                    serde_json::json!({"arg0": gene.diseases.len(), "arg1": gene.symbol}),
                ));
            }
        }
        Target::Phenotype(p) => {
            let term = ctx.atlas.hpo.term(p);
            let def = if term.definition.is_empty() {
                String::new()
            } else {
                format!(": {}", term.definition)
            };
            run.fact(NewFact::node_msg(
                &term.id,
                crate::copy::msg(
                    "ask.fact.term_definition",
                    serde_json::json!({"arg0": view::term_text(ctx.atlas, p), "def": def}),
                ),
                "Human Phenotype Ontology",
            ));
        }
    }
    Ok(())
}
