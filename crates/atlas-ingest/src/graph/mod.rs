//! The connected layer (D15): ingest of the source caches (design/SOURCES.md) into a
//! [`GraphData`] linked to the atlas's conditions and genes, cached as `data/cache/graph.snapshot`
//! (separate from the atlas snapshot, so a cache refresh never rebuilds the atlas), plus re-reading
//! and re-hashing source records for verification.

pub mod builder;
pub mod cache;
mod claims;
mod contacts;
pub mod ecosystem;
#[cfg(test)]
mod ecosystem_tests;
#[cfg(test)]
pub(crate) mod fixtures;
mod funding;
mod hgnc;
pub mod kgx;
#[cfg(test)]
mod kgx_tests;
mod labels;
#[cfg(test)]
mod merge_tests;
mod openaccess;
mod orgs;
mod outcomes;
mod people;
mod programmes;
pub mod quarantine;
pub mod research;
#[cfg(test)]
mod research_tests;
mod samples;
pub use samples::model_id;
pub mod sssom;
mod trials;
pub mod verify;
mod works;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use atlas_core::graph::{GraphData, Relation, activity};
use atlas_core::{Atlas, Graph, snapshot};

use crate::error::IngestError;
use crate::{Origin, obo, sources};

/// Bump when the graph rules change (forces a rebuild of the graph snapshot).
pub const GRAPH_RULES: u32 = 16;

/// `data/cache/graph.snapshot`, or `$RARE_ATLAS_GRAPH_SNAPSHOT` (a private snapshot for test servers
/// that read a shared data dir without rewriting its snapshot).
pub fn snapshot_path(data: &Path) -> PathBuf {
    match std::env::var_os("RARE_ATLAS_GRAPH_SNAPSHOT") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => crate::snapshot_dir(data).join("graph.snapshot"),
    }
}

/// Cache files the graph reads (relative to the data dir), existing ones only, sorted.
fn inputs(data: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        data.join(trials::STUDIES),
        data.join(trials::HEADER),
        data.join(people::OVERLAP),
        data.join(contacts::FILE),
        data.join(labels::FILE),
        data.join("raw").join(hgnc::FILE),
        data.join(orgs::WIDE),
        data.join(orgs::SUPPLEMENT),
        data.join(openaccess::FILE),
        data.join(programmes::PIPELINES),
        data.join(programmes::REGULATORY),
        data.join(samples::ORG_ASSETS),
        data.join(samples::ORG_OVERLAPS),
        data.join(samples::ERDRI),
        data.join(outcomes::FILE),
        data.join(funding::CALLS),
        data.join(claims::FILE),
        data.join(ecosystem::INITIATIVES),
        data.join(ecosystem::EDGES),
        data.join(quarantine::FILE),
    ];
    for dir in [
        "cache/reporter",
        "cache/bench-fixes/reporter",
        "cache/pubmed",
        "cache/orgs",
        "cache/research_intl",
        "cache/ecosystem/responses",
        samples::MODELS_DIR,
    ] {
        for e in std::fs::read_dir(data.join(dir)).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_file()
                && p.extension()
                    .is_some_and(|x| x == "json" || (dir == "cache/ecosystem/responses" && x == "bin"))
            {
                out.push(p);
            }
        }
    }
    out.extend(kgx::inputs(data));
    out.extend(sssom::files(data));
    out.retain(|p| p.exists());
    out.sort();
    out
}

/// Rules version + atlas sources + `file:size:mtime` of every cache input.
pub fn signature(data: &Path) -> Result<String, IngestError> {
    signature_with_scope(data, kgx::Scope::from_env())
}

fn signature_with_scope(data: &Path, scope: kgx::Scope) -> Result<String, IngestError> {
    let mut sig = format!(
        "graph-rules:{GRAPH_RULES};kgx-hops:{};kgx-max-edges:{};{}",
        scope.hops,
        scope.max_edges,
        sources::signature(&crate::raw_dir(data))?
    );
    let projection = crate::identity_projection::load(data)?;
    sig.push_str(&format!(
        "identity:{}:{};",
        atlas_core::identity_policy::RULE,
        projection.accepted.manifest_sha256
    ));
    for p in inputs(data) {
        let meta = std::fs::metadata(&p).map_err(IngestError::io(&p))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let rel = p.strip_prefix(data).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        sig.push_str(&format!("{rel}:{}:{mtime};", meta.len()));
    }
    // Suppression + quarantine lists and the salt: any change rebuilds (D43: never re-ingest).
    sig.push_str(&crate::withhold::signature(data, &crate::withhold::salt_from_env())?);
    Ok(sig)
}

#[cfg(test)]
mod signature_tests {
    use super::*;

    #[test]
    fn scope_changes_invalidate_graph_snapshot() {
        let data = fixtures::TempData::new();
        for source in &sources::SOURCES {
            data.write(&format!("raw/{}", source.file), b"fixture");
        }
        let baseline = signature_with_scope(data.path(), kgx::Scope::default()).unwrap();
        for scope in [
            kgx::Scope {
                hops: 1,
                ..kgx::Scope::default()
            },
            kgx::Scope {
                max_edges: 1,
                ..kgx::Scope::default()
            },
        ] {
            assert_ne!(baseline, signature_with_scope(data.path(), scope).unwrap());
        }
        assert_eq!(
            baseline,
            signature_with_scope(data.path(), kgx::Scope::default()).unwrap()
        );
    }
}

/// Build the connected layer from the caches under `data` and the atlas. The withholding lists
/// are applied before the data leaves the build (fail-closed: an invalid list fails the build).
pub fn build(data: &Path, atlas: &Atlas) -> Result<GraphData, IngestError> {
    let withhold = crate::withhold::load(data, crate::withhold::salt_from_env())?;
    build_with(data, atlas, &withhold)
}

/// [`build`] with an explicit filter (tests, tools).
pub fn build_with(
    data: &Path,
    atlas: &Atlas,
    withhold: &atlas_core::withhold::Withhold,
) -> Result<GraphData, IngestError> {
    let raw = crate::raw_dir(data);
    // Data may live in a different checkout; provenance must name the executing engine's checkout.
    let engine_repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut b = builder::Builder::new(atlas, sources::agent(&engine_repo));
    // MONDO (already a source of the atlas) again for MeSH xrefs and the parent hierarchy
    let mondo_entity = atlas
        .provenance
        .entity_by_file(sources::MONDO_OBO)
        .map(|e| atlas.provenance.entity(e).clone());
    if let Some(e) = mondo_entity {
        b.entity(e);
    }
    let mondo = obo::read_obo(&raw.join(sources::MONDO_OBO))?;
    let by_id: HashMap<&str, &atlas_core::Term> = mondo
        .terms
        .iter()
        .filter(|t| t.id.starts_with("MONDO:") && !t.obsolete)
        .map(|t| (t.id.as_str(), t))
        .collect();
    let dicts = trials::Dicts::new(&b, &by_id);
    trials::ingest(&mut b, data, &dicts, &by_id)?;
    let names = works::ConditionNames::new(&b, &dicts);
    works::reporter(&mut b, data, &names)?;
    let authors = works::pubmed(&mut b, data, &names)?;
    hgnc::ingest(&mut b, &raw)?;
    contacts::ingest(&mut b, data)?;
    labels::ingest(&mut b, data)?;
    orgs::ingest(&mut b, data, &dicts)?;
    programmes::ingest(&mut b, data)?;
    samples::ingest(&mut b, data)?;
    outcomes::ingest(&mut b, data)?;
    funding::ingest(&mut b, data)?;
    claims::ingest(&mut b, data)?;
    ecosystem::ingest(&mut b, data, withhold)?;
    let identity = sssom::identity(&b, data)?;
    people::ingest(&mut b, data, authors, &identity)?;
    kgx::ingest(&mut b, data, &identity, kgx::Scope::from_env())?;
    sssom::finish(&mut b, data, &identity)?;
    openaccess::ingest(&mut b, data)?;
    drop(identity);
    licences(&mut b);
    quarantine::apply(&mut b, withhold);
    let text_links = b
        .data
        .edges
        .iter()
        .filter(|e| e.relation == Relation::AboutCondition)
        .count();
    if let Some(i) = b
        .data
        .provenance
        .activities
        .iter()
        .position(|a| a.id == activity::LINK_TEXT)
    {
        b.finish(atlas_core::provenance::ActivityIdx(i as u16), &[("edges", text_links)]);
    }
    apply_withhold(&mut b, withhold);
    Ok(b.data)
}

/// Suppression (D43): drop suppressed people, their edges and contacts; the PROV activity records
/// counts and entry ids only, never identifiers.
fn apply_withhold(b: &mut builder::Builder<'_>, withhold: &atlas_core::withhold::Withhold) {
    let act = b.start(
        activity::APPLY_SUPPRESSION,
        "Apply data/suppression.json (approved removal/objection requests, hashed identifiers)",
        &[],
    );
    {
        let a = b.data.provenance.activity_mut(act);
        a.parameters.insert("salt_id".into(), withhold.salt().id().into());
        a.parameters
            .insert("entries".into(), withhold.suppression_entries().len().to_string());
        if withhold.closed_reason().is_some() {
            a.parameters.insert("closed".into(), "true".into());
        }
    }
    let n = atlas_core::withhold::apply_suppression(&mut b.data, withhold);
    b.finish(
        act,
        &[
            ("people_removed", n.people),
            ("edges_removed", n.edges),
            ("contacts_removed", n.contacts),
            ("records_redacted", n.records_redacted),
        ],
    );
}

/// Every entity without an explicit licence entry gets one classified from its licence text
/// (conservative: unrecognised terms are `unknown`, D37 Â§3).
fn licences(b: &mut builder::Builder<'_>) {
    for i in 0..b.data.provenance.entities.len() {
        let e = &b.data.provenance.entities[i];
        let text = e.licence.clone().unwrap_or_else(|| "not stated".into());
        let class = atlas_core::graph::LicenceClass::classify(&text);
        research::licence(b, atlas_core::provenance::EntityIdx(i as u16), &text, class);
    }
}

/// Load the graph snapshot if it matches the inputs, else build and save it.
pub fn load_or_build(data: &Path, atlas: &Atlas, rebuild: bool) -> Result<(Graph, Origin), IngestError> {
    let path = snapshot_path(data);
    let sig = signature(data)?;
    if !rebuild && path.exists() && snapshot::graph_signature(&path).is_ok_and(|s| s == sig) {
        let (mut graph, _) = snapshot::load_graph(&path)?;
        graph.set_withhold(crate::withhold::load(data, crate::withhold::salt_from_env())?);
        return Ok((graph, Origin::Snapshot));
    }
    let data_ = build(data, atlas)?;
    snapshot::save_graph(&path, &data_, &sig)?;
    let mut graph = Graph::new(data_);
    graph.set_withhold(crate::withhold::load(data, crate::withhold::salt_from_env())?);
    Ok((graph, Origin::Built))
}
