//! Copy-level search accounting. Never implies that an absent match does not exist elsewhere.
use super::{Item, msg};
use atlas_core::provenance::SourceEntity;
use atlas_core::{Atlas, Graph};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn source(file: &str) -> String {
    let s = file.to_lowercase().replace('\\', "/");
    for (needle, name) in [
        ("mondo", "mondo"),
        ("orpha", "orphanet"),
        ("omim", "omim"),
        ("hp.obo", "hpo"),
        ("hpo", "hpo"),
        ("phenotype", "hpo"),
        ("hgnc", "hgnc"),
        ("g2p", "g2p"),
        ("gene2phenotype", "g2p"),
        ("trials", "ctgov"),
        ("ctgov", "ctgov"),
        ("reporter", "reporter"),
        ("pubmed", "pubmed"),
        ("orgs", "orgs"),
        ("groups", "orgs"),
        ("models", "models"),
        ("opentargets", "opentargets"),
        ("wikidata", "wikidata"),
    ] {
        if s.contains(needle) {
            return name.into();
        }
    }
    s.split('/')
        .find(|p| !["data", "cache", "raw", "kgx"].contains(p))
        .unwrap_or("unknown")
        .trim_end_matches(".json")
        .into()
}

#[derive(Default)]
struct Copy {
    entities: Vec<Value>,
    loaded: bool,
    version: Option<String>,
    retrieved: Option<String>,
    examples: BTreeMap<String, Value>,
}

fn add_entity(c: &mut Copy, e: &SourceEntity) {
    c.loaded = true;
    let version = e
        .version
        .clone()
        .or_else(|| e.retrieved_at.as_deref().map(|s| s.chars().take(10).collect()));
    if c.version.is_none() {
        c.version = version;
    }
    if c.retrieved.is_none() {
        c.retrieved = e.retrieved_at.clone();
    }
    // Additive: full copy lineage makes version/date ambiguity visible when several files were searched.
    c.entities.push(crate::prov::entity(e));
}

pub fn build(atlas: &Atlas, graph: &Graph, items: &[Item], input_kind: &str) -> Value {
    let mut copies = BTreeMap::<String, Copy>::new();
    for name in [
        "mondo",
        "orphanet",
        "omim",
        "hpo",
        "hgnc",
        "g2p",
        "ctgov",
        "reporter",
        "pubmed",
        "orgs",
        "models",
        "opentargets",
        "wikidata",
    ] {
        if input_kind != "document" || ["mondo", "orphanet", "omim", "hpo", "hgnc", "g2p", "wikidata"].contains(&name) {
            copies.entry(name.into()).or_default();
        }
    }
    for e in &atlas.provenance.entities {
        add_entity(copies.entry(source(&e.file)).or_default(), e);
    }
    for cov in graph.coverage() {
        // Intake validates terms against the atlas, HGNC aliases and multilingual Wikidata
        // names. It does not search study/group/paper titles, so do not claim to have checked them.
        if input_kind == "document" && !["hgnc", "wikidata"].contains(&cov.source.as_str()) {
            continue;
        }
        let c = copies.entry(cov.source.clone()).or_default();
        c.loaded |= cov.status == "loaded";
        c.retrieved = cov.retrieved_at.clone();
        c.version = cov.retrieved_at.as_deref().map(|s| s.chars().take(10).collect());
        for file in &cov.files {
            if let Some(idx) = graph.provenance().entity_by_file(file) {
                add_entity(c, graph.provenance().entity(idx));
            }
        }
    }
    for i in items {
        let mut names = BTreeSet::new();
        if i.match_kind.starts_with("wikidata_") {
            names.insert("wikidata".into());
        }
        if i.match_kind == "alias" && graph.gene_alias(&i.node.id).is_some() {
            names.insert("hgnc".into());
        }
        if let Some(k) = graph.node(&i.node.id) {
            for &r in graph.node_records(k) {
                let entity = graph.provenance().entity(graph.record(r).entity);
                let name = graph
                    .coverage()
                    .iter()
                    .find(|c| c.files.contains(&entity.file))
                    .map(|c| c.source.clone())
                    .unwrap_or_else(|| source(&entity.file));
                names.insert(name);
            }
        } else if let Some(d) = atlas.disease_idx(&i.node.id) {
            for r in &atlas.disease_at(d).derived_from {
                names.insert(source(&atlas.provenance.entity(r.entity).file));
            }
        } else if atlas.hpo.canonical(&i.node.id).is_some() {
            if let Some(e) = atlas.provenance.entity_by_file("hp.obo") {
                names.insert(source(&atlas.provenance.entity(e).file));
            }
        } else if let Some(g) = atlas.gene(&i.node.id) {
            let gene = atlas.gene_at(g);
            for &d in &gene.diseases {
                for link in atlas.disease_at(d).genes.iter().filter(|l| l.symbol == gene.symbol) {
                    names.insert(source(&atlas.provenance.entity(link.record.entity).file));
                }
            }
        }
        for name in names {
            copies
                .entry(name)
                .or_default()
                .examples
                .insert(i.node.id.clone(), json!(i.node));
        }
    }
    let loaded = copies.values().filter(|c| c.loaded).count();
    let matched = copies.values().filter(|c| !c.examples.is_empty()).count();
    let sources: Vec<_> = copies.into_iter().map(|(name,c)| {
        let n = c.examples.len();
        let status = if n > 0 { "matched" } else if c.loaded { "no_match" } else { "not_loaded" };
        let fallback = match status { "matched" => format!("{name}: {n} matching items in our copy."), "no_match" => format!("No match in our {name} copy."), _ => format!("{name} is not loaded.") };
        json!({"source":name,"label":msg(&format!("checked.source.{name}"),json!({"source":name}),name.clone()),"version":c.version,"retrieved_at":c.retrieved,
            "status":status,"found":n,"examples":c.examples.values().take(3).collect::<Vec<_>>(),"copies":c.entities,
            "msg":msg(&format!("checked.{status}"),json!({"source":name,"n":n}),fallback)})
    }).collect();
    json!({"input_kind":input_kind,"at":atlas_core::provenance::rfc3339(std::time::SystemTime::now()),"sources":sources,
        "summary":msg("checked.summary",json!({"sources":loaded,"matched":matched}),format!("We checked {loaded} loaded sources; {matched} had matches."))})
}
