//! JSON of connection cards, the coverage block ("what we searched, when, what we found"),
//! `/api/condition/{id}/connections` and `/api/condition/{id}/gaps`.

use std::collections::BTreeMap;

use atlas_core::graph::{Coverage, Relation, StudyKind};
use atlas_core::node::NodeKind;
use atlas_core::{Atlas, Graph};
use serde_json::{Value, json};

use crate::connections::{Connections, Fact, Found, expand_kinds, is_card_org, plain_kind};
use crate::copy::msg;
use crate::{nodes, prov};

fn study_kind(kind: &str) -> Option<StudyKind> {
    [
        StudyKind::Trial,
        StudyKind::Registry,
        StudyKind::NaturalHistory,
        StudyKind::Observational,
        StudyKind::ExpandedAccess,
    ]
    .into_iter()
    .find(|k| k.as_str() == kind)
}

/// One connection card (JOURNEYS "connection card": who, why, reach, checked on, how we know).
pub fn card(atlas: &Atlas, graph: &Graph, f: &Found) -> Value {
    let who = graph.node_ref(f.who);
    let records = graph.node_records(f.who);
    let mut checked_on = records.first().and_then(|&r| nodes::checked_on(graph, r));
    let (subtitle, reach, status) = match f.who.kind {
        NodeKind::Study => {
            let s = graph.study(f.who.idx);
            let status = json!({
                "status": s.status,
                "open": s.is_open(),
                "recruiting": s.is_recruiting(),
                "phases": s.phases,
                "countries": s.countries,
                "sponsor": s.sponsor,
                "sponsor_class": s.sponsor_class,
                "start": s.start,
                "completion": s.completion,
                "enrollment_study_wide": s.enrollment,
                "enrollment_note": msg("card.enrollment_note", json!({}))["fallback"],
                "enrollment_note_msg": msg("card.enrollment_note", json!({})),
                "covers": covers(atlas, graph, &s.id),
                "officials": graph.contacts(&s.id).map(|c| &c.officials),
                "sites": graph.contacts(&s.id).map(|c| &c.sites),
                "last_update": graph.contacts(&s.id).map(|c| &c.last_update),
                "interventions": s.interventions,
                "kind_explained": msg(&format!("studykind.{}", s.kind.as_str()), json!({}))["fallback"].clone(),
                "kind_explained_msg": msg(&format!("studykind.{}", s.kind.as_str()), json!({})),
            });
            (
                msg(
                    "card.subtitle.study",
                    json!({ "kind": s.kind.as_str(), "kind_label": plain_kind(s.kind.as_str()),
                        "status": s.status, "status_label": s.status.to_lowercase().replace('_', " ") }),
                ),
                nodes::study_reach(graph, &s.id),
                status,
            )
        }
        NodeKind::Grant => {
            let g = graph.grant(f.who.idx);
            let years = match (g.fiscal_years.iter().min(), g.fiscal_years.iter().max()) {
                (Some(a), Some(b)) if a != b => format!("{a}–{b}"),
                (Some(a), _) => a.to_string(),
                _ => String::new(),
            };
            let pis: Vec<String> = graph
                .incident(&g.id)
                .filter(|i| !i.outgoing && i.edge.relation == atlas_core::graph::Relation::PrincipalInvestigatorOf)
                .filter_map(|i| graph.node(i.other).map(|k| graph.node_ref(k).label))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let status = json!({
                "activity_code": g.activity_code, "agency": g.agency, "organisation": g.organisation,
                "country": g.country, "fiscal_years": years, "award_total_usd": g.award_total,
                "start": g.start, "end": g.end, "principal_investigators": pis,
                "evidence": g.id.starts_with("REPORTER:").then(|| format!("/api/funding/{}/evidence", nodes::encode_path(&g.id))),
            });
            (
                msg(
                    "card.subtitle.grant",
                    json!({ "code": g.activity_code, "organisation": g.organisation, "years": years }),
                ),
                nodes::grant_reach(&g.url),
                status,
            )
        }
        NodeKind::Person => {
            let p = graph.person(f.who.idx);
            let status = json!({
                "affiliations": p.affiliations, "genes": p.genes, "communities": p.communities,
                "cross_community": p.cross_community, "identity": p.matched_by,
                "identity_evidence": p.merge_basis.len(), "orcids": p.orcids,
            });
            (
                json!(p.affiliations.first().cloned().unwrap_or_default()),
                nodes::person_reach(p),
                status,
            )
        }
        NodeKind::Organisation => {
            let o = graph.org(f.who.idx);
            if o.verified_on.is_some() {
                checked_on = o.verified_on.as_deref().map(nodes::day);
            }
            let status = json!({
                "description": o.description, "country": o.country, "languages": o.languages,
                "verified_on": o.verified_on, "country_basis": o.country_basis, "channels": o.channels,
            });
            (
                o.description.clone().map_or_else(
                    || {
                        msg(
                            "card.subtitle.kind",
                            json!({ "kind": o.kind.as_str(), "kind_label": plain_kind(o.kind.as_str()) }),
                        )
                    },
                    Value::from,
                ),
                nodes::org_reach(o.url.as_deref(), o.contact_url.as_deref(), o.kind),
                status,
            )
        }
        _ => (json!(""), Value::Null, Value::Null),
    };
    let edge_ids: Vec<&str> = f.facts.iter().filter_map(|x| x.edge_id.as_deref()).collect();
    let facts: Vec<Value> = f.facts.iter().map(|x| fact_json(atlas, graph, x)).collect();
    json!({
        "id": who.id,
        "who": { "id": who.id, "kind": who.kind, "label": who.label,
            "subtitle": if subtitle.is_object() { subtitle["fallback"].clone() } else { subtitle.clone() },
            "subtitle_msg": if subtitle.is_object() { subtitle.clone() } else { Value::Null } },
        "kind": f.kind,
        "kind_explained": study_kind(f.kind).map(|k| msg(&format!("studykind.{}", k.as_str()), json!({}))["fallback"].clone()),
        "kind_explained_msg": study_kind(f.kind).map(|k| msg(&format!("studykind.{}", k.as_str()), json!({}))),
        "match": if f.exact { "exact" } else { "related" },
        "level": f.level.as_str(),
        "why": { "text": f.why["fallback"], "msg": f.why, "origin": "template", "facts": facts },
        "reach": reach,
        "status": status,
        "checked_on": checked_on,
        "edge_ids": edge_ids,
    })
}

/// A card fact with its source record (B4): the existing fields plus `assertion` (the edge or node
/// the fact states), `generated_by` (the `prov:Activity`), `url`, `locator` (`file#locator`),
/// `retrieved_at`, `sha256` and `sha256_scope` (`record`: the cached record itself, `file`: the
/// source file of an atlas edge). The first source record is given; `records` counts them all.
/// Fields are null where the graph has no record for the fact (e.g. MONDO hierarchy facts).
fn fact_json(atlas: &Atlas, graph: &Graph, f: &Fact) -> Value {
    let mut v = serde_json::to_value(f).expect("fact serialises");
    v["assertion"] = json!(f.edge_id.as_deref().unwrap_or(&f.key));
    let source = f.edge_id.as_deref().and_then(|id| fact_source(atlas, graph, id));
    let source = source.unwrap_or_else(|| {
        json!({ "generated_by": null, "url": null, "locator": null, "retrieved_at": null,
                "sha256": null, "sha256_scope": null, "records": 0 })
    });
    if let (Value::Object(o), Value::Object(s)) = (&mut v, source) {
        o.extend(s);
    }
    v
}

fn fact_source(atlas: &Atlas, graph: &Graph, id: &str) -> Option<Value> {
    if let Some(i) = graph.edge_by_id(id) {
        let e = graph.edge(i);
        let prov = graph.provenance();
        let r = graph.record(*e.records.first()?);
        let ent = prov.entity(r.entity);
        return Some(json!({
            "generated_by": prov.activity(e.activity).id,
            "url": r.url.clone().unwrap_or_else(|| ent.url.clone()),
            "locator": format!("{}#{}", ent.file, r.locator),
            "retrieved_at": r.fetched_at.clone().or_else(|| ent.retrieved_at.clone()),
            "sha256": atlas_core::graph::hex(&r.sha256),
            "sha256_scope": "record",
            "records": e.records.len(),
        }));
    }
    let (from, relation, to) = atlas_core::node::parse_edge_id(id)?;
    let records = prov::edge_records(atlas, from, relation, to)?;
    let r = *records.first()?;
    let ent = atlas.provenance.entity(r.entity);
    Some(json!({
        "generated_by": atlas.provenance.generator_of(r.entity).map(|a| a.id.clone()),
        "url": ent.url,
        "locator": atlas.provenance.cite(r),
        "retrieved_at": ent.retrieved_at,
        "sha256": ent.sha256,
        "sha256_scope": "file",
        "records": records.len(),
    }))
}

/// Conditions and genes a study names (J2.3 "covers which diseases"): exact links first, at most
/// [`COVERS`] listed, with the total.
const COVERS: usize = 25;

fn covers(atlas: &Atlas, graph: &Graph, nct: &str) -> Value {
    let mut links: Vec<_> = graph
        .incident(nct)
        .filter(|i| i.outgoing && matches!(i.edge.relation, Relation::StudiesCondition | Relation::NamesGene))
        .collect();
    links.sort_by_key(|i| (i.edge.level, i.edge.relation));
    let items: Vec<Value> = links
        .iter()
        .take(COVERS)
        .map(|i| {
            json!({
                "node": nodes::any_ref(atlas, graph, i.other),
                "relation": i.edge.relation.as_str(),
                "level": i.edge.level.as_str(),
                "reason": i.edge.reason,
                "edge_id": i.edge.id(),
            })
        })
        .collect();
    json!({ "total": links.len(), "items": items })
}

fn coverage_entry(c: &Coverage, genes: &[(String, String, String)], found: Value) -> Value {
    let symbols: Vec<&str> = genes.iter().map(|(_, s, _)| s.as_str()).collect();
    let (covers, note): (bool, Option<Value>) = if c.genes.is_empty() {
        (c.source == "ctgov", None)
    } else {
        let covered: Vec<&str> = symbols
            .iter()
            .copied()
            .filter(|s| c.genes.iter().any(|g| g == s))
            .collect();
        let note = (covered.len() < symbols.len() || symbols.is_empty()).then(|| {
            let missing: Vec<&str> = symbols.iter().filter(|s| !covered.contains(s)).copied().collect();
            if symbols.is_empty() {
                msg(
                    "coverage.partial_no_gene",
                    json!({ "source": c.label, "genes": c.genes }),
                )
            } else {
                msg(
                    "coverage.partial",
                    json!({ "source": c.label, "genes": c.genes, "missing": missing }),
                )
            }
        });
        (!covered.is_empty(), note)
    };
    json!({
        "source": c.source, "label": c.label, "status": c.status, "retrieved_at": c.retrieved_at,
        "checked_on": c.retrieved_at.as_deref().map(nodes::day), "scope": c.scope,
        "scope_msg": crate::copy::scope(&c.source, &c.scope), "records": c.records,
        "covers_this_condition": covers && c.status == "loaded" && c.header_checksums_failed == 0,
        "check_kind": "cached_index", "genes_queried": c.genes,
        "note": note.as_ref().map(|m| m["fallback"].clone()), "note_msg": note, "found": found,
    })
}

/// Counts per kind: `{kind: {exact, related}}`.
fn counts(conn: &Connections, kinds: &[&str]) -> BTreeMap<&'static str, (usize, usize)> {
    let mut out = BTreeMap::new();
    for f in &conn.found {
        if kinds.contains(&f.kind) {
            let e = out.entry(f.kind).or_insert((0, 0));
            if f.exact { e.0 += 1 } else { e.1 += 1 }
        }
    }
    out
}

/// The coverage block every connections/gaps response carries (J3.1: never an empty page).
pub fn coverage(graph: &Graph, conn: &Connections, kinds: &[&str]) -> Value {
    let c = counts(conn, kinds);
    let sum = |ks: &[&str]| {
        ks.iter()
            .filter_map(|k| c.get(k))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    };
    let study = sum(&[
        "registry",
        "natural_history",
        "trial",
        "observational",
        "expanded_access",
    ]);
    let searched: Vec<Value> = graph
        .coverage()
        .iter()
        .map(|cov| {
            let found = match cov.source.as_str() {
                "ctgov" => json!({ "studies_exact": study.0, "studies_related": study.1 }),
                "reporter" => json!({ "grants": conn.grants }),
                "pubmed" => json!({ "papers": conn.papers }),
                "people" => json!({ "researchers": sum(&["researcher"]).0 + sum(&["researcher"]).1 }),
                "orgs" => {
                    let o = sum(&["patient_group", "expert_centre", "organisation"]);
                    json!({ "organisations_exact": o.0, "organisations_related": o.1 })
                }
                _ => Value::Null,
            };
            coverage_entry(cov, &conn.genes, found)
        })
        .collect();
    let any_exact = c.values().any(|v| v.0 > 0);
    let any = c.values().any(|v| v.0 + v.1 > 0);
    let status = if any_exact {
        "ok"
    } else if any {
        "related_only"
    } else {
        "none_found"
    };
    let covered: Vec<&Value> = searched.iter().filter(|c| c["covers_this_condition"] == true).collect();
    let dates: Vec<String> = covered
        .iter()
        .filter_map(|c| c["retrieved_at"].as_str().map(nodes::day))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let note_msg = (status != "ok").then(|| {
        let sources: Vec<&str> = covered.iter().filter_map(|c| c["label"].as_str()).collect();
        msg(
            "coverage.none",
            json!({ "condition": conn.label, "sources": sources, "dates": dates }),
        )
    });
    let note = note_msg.as_ref().map(|m| m["fallback"].clone());
    json!({ "status": status, "note": note, "note_msg": note_msg, "searched": searched })
}

/// `/api/condition/{id}/connections` (without LLM sentences; see `explain`).
pub fn connections(atlas: &Atlas, graph: &Graph, conn: &Connections, kind: Option<&str>, limit: usize) -> Value {
    let kinds = expand_kinds(kind);
    let c = counts(conn, &kinds);
    let mut shown: BTreeMap<&str, usize> = BTreeMap::new();
    let mut cards = Vec::new();
    for f in &conn.found {
        if !kinds.contains(&f.kind) || (f.who.kind == NodeKind::Organisation && !is_card_org(graph.org(f.who.idx).kind))
        {
            continue;
        }
        let n = shown.entry(f.kind).or_default();
        if *n < limit {
            *n += 1;
            cards.push(card(atlas, graph, f));
        }
    }
    // exact cards first, then by kind order, keeping the score order inside
    cards.sort_by_key(|card| {
        let exact = card["match"] == "exact";
        let k = card["kind"].as_str().unwrap_or("");
        (
            !exact,
            crate::connections::KINDS.iter().position(|x| *x == k).unwrap_or(99),
        )
    });
    let none_found: Vec<Value> = kinds
        .iter()
        .filter(|k| c.get(*k).is_none_or(|v| v.0 == 0))
        .map(|k| {
            let related = c.get(k).map_or(0, |v| v.1);
            let note = msg(
                "connections.none_found",
                json!({ "kind": k, "kind_label": plain_kind(k), "condition": conn.label, "related": related }),
            );
            json!({ "kind": k, "exact": 0, "related": related, "note": note["fallback"], "note_msg": note })
        })
        .collect();
    let disease = atlas.disease_at(conn.condition);
    json!({
        "condition": atlas.disease_ref(conn.condition),
        "newly_described": disease.is_newly_described(),
        "confidence": disease.classification("confidence"),
        "genes": conn.genes.iter().map(|(id, s, e)| json!({ "id": id, "symbol": s, "edge_id": e })).collect::<Vec<_>>(),
        "kinds": kinds,
        "counts": c.iter().map(|(k, v)| (k.to_string(), json!({ "exact": v.0, "related": v.1, "shown": shown.get(k).copied().unwrap_or(0) }))).collect::<serde_json::Map<_, _>>(),
        "cards": cards,
        "none_found": none_found,
        "coverage": coverage(graph, conn, &kinds),
        "subject_coverage": crate::coverage::condition(atlas, graph, conn.condition),
        "rare": disease.rare,
    })
}

/// `/api/condition/{id}/gaps` (J3.4): what was searched, what is missing, what would change the answer.
pub fn gaps(atlas: &Atlas, graph: &Graph, conn: &Connections) -> Value {
    let d = atlas.disease_at(conn.condition);
    let kinds = expand_kinds(None);
    let c = counts(conn, &kinds);
    let exact = |k: &str| c.get(k).map_or(0, |v| v.0);
    let open_exact = conn
        .found
        .iter()
        .filter(|f| f.exact && f.who.kind == NodeKind::Study && graph.study(f.who.idx).is_open())
        .count();
    let symbols: Vec<&str> = conn.genes.iter().map(|(_, s, _)| s.as_str()).collect();
    // Each gap keeps its three short parts (`what`, `why`, `how_to_close`) and adds the whole
    // plain-language sentence as a catalog message (`msg`, D26) that the web renders as one line.
    let mut missing: Vec<Value> = Vec::new();
    let mut gap = |key: &str, params: Value, what: &str, why: String, how: &str| {
        let m = msg(key, params);
        missing.push(
            json!({ "id": key.trim_start_matches("gaps.missing."), "what": what, "why": why,
            "how_to_close": how, "text": m["fallback"], "msg": m }),
        );
    };
    let orgs_loaded = graph.coverage_of("orgs").is_some_and(|c| c.status == "loaded");
    if exact("patient_group") == 0 {
        let (key, why) = if orgs_loaded {
            (
                "gaps.missing.patient_group",
                "no patient group on our list names this condition or its gene",
            )
        } else {
            (
                "gaps.missing.patient_group_unloaded",
                "our list of patient groups is not loaded",
            )
        };
        gap(
            key,
            json!({}),
            "patient group for this exact condition",
            why.to_owned(),
            "ask your genetic counsellor or a related patient group whether families with this condition are already in touch",
        );
    }
    if open_exact == 0 {
        gap(
            "gaps.missing.open_study",
            json!({}),
            "open study for this exact condition",
            "no study on ClinicalTrials.gov that is recruiting or running names this condition, its MeSH term or its gene".into(),
            "ask the nearest specialist centre about studies that are not registered yet, and check studies for related conditions",
        );
    }
    if exact("registry") + exact("natural_history") == 0 {
        gap(
            "gaps.missing.registry",
            json!({}),
            "patient registry or natural history study",
            "none names this condition or its gene".into(),
            "ask a related patient group whether its registry can include this gene, or join a registry for the broader group of conditions",
        );
    }
    if conn.genes.is_empty() {
        gap(
            "gaps.missing.gene",
            json!({}),
            "known cause (gene)",
            "Orphanet and OMIM record no gene that causes this condition".into(),
            "ask whether genetic testing was done and which gene change was found",
        );
    }
    let effects: Vec<&'static str> = d
        .genes
        .iter()
        .map(|g| g.variant_effect())
        .filter(|e| *e != "unknown")
        .collect();
    if !conn.genes.is_empty() && effects.is_empty() {
        gap(
            "gaps.missing.variant_effect",
            json!({}),
            "how the gene change causes the condition (less or altered gene function)",
            "Orphanet records no effect of the gene change for this condition".into(),
            "ask a specialist whether the change reduces what the gene does or alters it",
        );
    }
    if d.phenotypes.len() < 5 {
        gap(
            "gaps.missing.symptoms",
            json!({ "n": d.phenotypes.len() }),
            "list of symptoms",
            format!(
                "only {} symptoms recorded in the Human Phenotype Ontology (HPO)",
                d.phenotypes.len()
            ),
            "a patient registry or natural history study would record them",
        );
    }
    if d.prevalence.is_empty() {
        gap(
            "gaps.missing.prevalence",
            json!({}),
            "how common it is",
            "Orphanet has no figure".into(),
            "patient registries and published case reports give a first count",
        );
    }
    let mut asks: Vec<Value> = vec![msg("gaps.question.centre", json!({}))];
    if let Some(s) = symbols.first() {
        asks.insert(0, msg("gaps.question.variant", json!({ "gene": s })));
        asks.push(msg("gaps.question.researchers", json!({ "gene": s })));
    }
    if open_exact == 0 {
        asks.push(msg("gaps.question.unregistered", json!({})));
    }
    let questions: Vec<Value> = asks.iter().map(|m| m["fallback"].clone()).collect();
    let note = msg("gaps.note", json!({}));
    json!({
        "condition": atlas.disease_ref(conn.condition),
        "found": c.iter().map(|(k, v)| (k.to_string(), json!({ "exact": v.0, "related": v.1 }))).collect::<serde_json::Map<_, _>>(),
        "missing": missing,
        "next_questions": questions,
        "next_questions_msg": asks,
        "searched": coverage(graph, conn, &kinds),
        "subject_coverage": crate::coverage::condition(atlas, graph, conn.condition),
        "known": {
            "causal_genes": symbols,
            "phenotypes": d.phenotypes.len(),
            "prevalence_records": d.prevalence.len(),
            "papers": conn.papers,
            "grants": conn.grants,
        },
        "note": note["fallback"],
        "note_msg": note,
    })
}
