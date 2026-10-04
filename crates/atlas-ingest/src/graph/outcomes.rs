//! Outcome measures and reusable natural-history metadata (`outcomes.resources` v1, D39 "outcome
//! measures"): how change in a condition has been measured, by whom, and how to get the
//! instrument or data. Links only through the record's own id mappings: an HGNC id
//! (`explicit_gene_symbol`) or a MONDO candidate from an exact label; never by fuzzy text.

use std::path::Path;

use atlas_core::graph::{Access, Asset, AssetKind, LicenceClass, LinkLevel, Relation, activity};
use atlas_core::node::EdgeKind;

use super::builder::Builder;
use super::research::{self, Spec, Via, opt, s, strs};
use crate::error::IngestError;

pub const FILE: &str = "cache/outcomes/resources.json";

fn kind(k: &str) -> AssetKind {
    match k {
        "controlled_access_dataset" | "dataset_catalogue_entry" | "research_reproducibility_asset" => {
            AssetKind::Dataset
        }
        _ => AssetKind::OutcomeMeasure,
    }
}

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_OUTCOMES,
        "Outcome measures and natural-history assessment metadata",
        &[],
    );
    let spec = Spec {
        source: "outcomes",
        label: "Outcome measures (FDA COA compendium, core outcome sets, natural-history protocols)",
        file: FILE,
        schema: "outcomes.resources",
        versions: &[1],
        url: "curated: docs/research/outcomes (FDA, COMET, NINDS CDE, study publications)",
        licence: "metadata from public pages; instruments keep their own terms (see each item's access licence)",
        class: LicenceClass::Unknown,
        scope: "outcome measures and assessment resources used for DEE-slice conditions",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        b.finish(act, &[]);
        return Ok(());
    };
    let (mut nodes, mut edges) = (0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        let id = research::asset_id(b, s(r, "id"));
        let access = &r["access"];
        let mut facts = vec![("resource_kind".to_owned(), s(r, "kind").to_owned())];
        for key in ["qualification_status"] {
            if let Some(v) = opt(r, key) {
                facts.push((key.to_owned(), v));
            }
        }
        for key in ["concept", "condition", "context"] {
            if let Some(v) = opt(&r["facts"], key) {
                facts.push((key.to_owned(), v));
            }
        }
        for d in strs(r, "domains") {
            facts.push(("domain".into(), d.to_owned()));
        }
        if let Some(l) = opt(access, "level") {
            facts.push(("access_level".into(), l));
        }
        if let Some(l) = opt(access, "licence") {
            facts.push(("instrument_terms".into(), l));
        }
        let route_url = opt(access, "route").filter(|u| u.starts_with("http"));
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label: s(r, "name").to_owned(),
                kind: kind(s(r, "kind")),
                category: s(r, "kind").to_owned(),
                holder: None,
                holder_name: opt(r, "source"),
                access: Access {
                    route: "official_page".into(),
                    url: route_url.or_else(|| opt(r, "url")),
                    note: opt(&r["reuse"], "next_step"),
                },
                facts,
                verify_url: opt(r, "url"),
                release: true,
                records: vec![rec],
            },
        ));
        for m in r["mappings"].as_array().into_iter().flatten() {
            if s(m, "basis") == "explicit_gene_symbol"
                && let Some(gid) = research::gene(b, s(m, "id"))
            {
                let via = Via {
                    act,
                    rec,
                    kind: EdgeKind::Observed,
                    level: LinkLevel::Gene,
                };
                edges += via.link(
                    b,
                    &id,
                    Relation::ResourceFor,
                    &gid,
                    format!("names gene {}", s(m, "gene_symbol")),
                );
            }
            for c in strs(m, "candidate_ids") {
                if let Some(cid) = research::rare_condition(b, c) {
                    let via = Via {
                        act,
                        rec,
                        kind: EdgeKind::Inferred,
                        level: LinkLevel::Exact,
                    };
                    let reason = format!("condition \"{}\" (exact MONDO label)", s(m, "condition_text"));
                    edges += via.link(b, &id, Relation::ResourceFor, &cid, reason);
                }
            }
        }
    }
    src.finish(b, nodes, edges, 0);
    b.finish(act, &[]);
    Ok(())
}
