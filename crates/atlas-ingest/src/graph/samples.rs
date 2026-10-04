//! Models, samples and datasets (S2, D39 "models & samples"): what exists, who holds it, how to
//! request it.
//!
//! - `org-assets.assets` v1: organisation-reported registries, natural-history studies, biobanks,
//!   iPSC and animal models (and other reported items kept as `other`). Roles stay as reported
//!   (`owner_role`: reporter / provider / funder / sponsor); the reporting organisation is linked
//!   separately when it is not the owner. Excluded records are counted, never shown.
//!   `org-assets.overlaps` v1 (`duplicates.json`) → `candidate_same_as`, never merges.
//! - `directories.erdri` v1: EU RD Platform registries flagged relevant, linked to rare conditions
//!   by their ORPHA codes.
//! - `models.assets` v1 (`models/<GENE>.json`): Alliance / IMPC model organisms and Cellosaurus
//!   cell lines, one asset per model id, `model_of` → human gene (orthology kept as the edge kind).

use std::path::Path;

use atlas_core::graph::{Access, Asset, AssetKind, LicenceClass, LinkLevel, Relation, activity};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::ActivityIdx;
use serde_json::Value;

use super::builder::Builder;
use super::research::{self, Spec, Via, opt, s, strs};
use crate::error::IngestError;

pub const ORG_ASSETS: &str = "cache/org-assets/assets.json";
pub const ORG_OVERLAPS: &str = "cache/org-assets/duplicates.json";
pub const ERDRI: &str = "cache/directories/erdri.json";
pub const MODELS_DIR: &str = "cache/models";

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_SAMPLES,
        "Models, samples, registries and datasets with their holders and access routes",
        &[],
    );
    org_assets(b, data, act)?;
    overlaps(b, data, act)?;
    erdri(b, data, act)?;
    models(b, data, act)?;
    b.finish(act, &[]);
    Ok(())
}

fn org_kind(k: &str) -> AssetKind {
    match k {
        "registry" => AssetKind::Registry,
        "natural_history" | "observational_study" => AssetKind::Dataset,
        "biobank" => AssetKind::Biobank,
        "ipsc_model" => AssetKind::CellLine,
        "animal_model" => AssetKind::Model,
        _ => AssetKind::Other,
    }
}

fn org_assets(b: &mut Builder<'_>, data: &Path, act: ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "org-assets",
        label: "Research assets reported by patient organisations",
        file: ORG_ASSETS,
        schema: "org-assets.assets",
        versions: &[1],
        url: "curated: rare_atlas.org_assets (snapshots in data/cache/org-assets/pages)",
        licence: "facts and short quotes from the organisations' public pages",
        class: LicenceClass::Unknown,
        scope: "assets the curated patient organisations report on their official websites",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges, mut excluded) = (0, 0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        if r["excluded"].as_bool() == Some(true) {
            excluded += 1;
            continue;
        }
        let id = research::asset_id(b, s(r, "id"));
        let kind_raw = s(r, "kind");
        let kind = org_kind(kind_raw);
        let owner = s(r, "owner_name");
        let owner = (!owner.is_empty() && !owner.eq_ignore_ascii_case("unknown")).then_some(owner);
        let mut facts = vec![("reported_kind".to_owned(), kind_raw.to_owned())];
        for key in ["owner_role", "status", "focus_basis", "verification"] {
            if let Some(v) = opt(r, key).filter(|v| v != "unknown") {
                facts.push((key.to_owned(), v));
            }
        }
        if let Some(c) = opt(r, "condition_focus") {
            facts.push(("condition_focus".into(), c));
        }
        if let Some(rep) = opt(r, "reporting_org_id") {
            facts.push(("reported_by".into(), rep));
        }
        for p in r["ids"].as_array().into_iter().flatten() {
            if let Some(v) = opt(p, "id").or_else(|| opt(p, "value")) {
                facts.push(("published_id".into(), v));
            }
        }
        let access = &r["access"];
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label: s(r, "name").to_owned(),
                kind,
                category: kind_raw.to_owned(),
                holder: None,
                holder_name: owner.map(str::to_owned),
                access: Access {
                    route: "official_page".into(),
                    url: opt(access, "url").or_else(|| opt(r, "url")),
                    note: opt(access, "instructions"),
                },
                facts,
                verify_url: opt(r, "url"),
                release: true,
                records: vec![rec],
            },
        ));
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Extracted,
            level: LinkLevel::Gene,
        };
        let relation = if matches!(kind, AssetKind::Model | AssetKind::CellLine) {
            Relation::ModelOf
        } else {
            Relation::ResourceFor
        };
        for g in strs(r, "genes") {
            if let Some(gid) = research::gene(b, g) {
                edges += via.link(b, &id, relation, &gid, format!("organisation reports gene \"{g}\""));
            }
        }
        let owner_id = opt(r, "owner_org_id");
        if let Some(o) = owner
            && let Some(org) = research::held_by(b, &id, owner_id.as_deref(), o, via)
        {
            edges += 1;
            if let Some((NodeKind::Asset, a)) = b.node(&id) {
                b.data.assets[a as usize].holder.get_or_insert(org);
            }
        }
        // the reporting organisation, when it is not the owner (reporter is not automatically operator)
        if let Some(rep) = opt(r, "reporting_org_id")
            && Some(&rep) != owner_id.as_ref()
            && b.node(&rep).is_some()
        {
            let via = Via {
                level: LinkLevel::Curated,
                ..via
            };
            edges += via.link(b, &id, Relation::RelatedTo, &rep, "reported by".into());
        }
    }
    src.finish(b, nodes, edges, excluded);
    Ok(())
}

fn overlaps(b: &mut Builder<'_>, data: &Path, act: ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "org-assets-overlaps",
        label: "Possible duplicates among organisation-reported assets (candidates only)",
        file: ORG_OVERLAPS,
        schema: "org-assets.overlaps",
        versions: &[1],
        url: "derived: rare_atlas.org_assets (same published id / shared platform)",
        licence: "derived from organisation-reported assets",
        class: LicenceClass::Unknown,
        scope: "pairs of reported assets sharing a published id or platform",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let mut edges = 0;
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        let ids: Vec<&str> = strs(r, "asset_ids").filter(|a| b.node(a).is_some()).collect();
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Inferred,
            level: LinkLevel::Related,
        };
        let reason = format!("{}: {}", s(r, "relation"), s(r, "basis"));
        for (k, a) in ids.iter().enumerate() {
            for c in &ids[k + 1..] {
                if a != c {
                    edges += via.link(b, a, Relation::CandidateSameAs, c, reason.clone());
                }
            }
        }
    }
    src.finish(b, 0, edges, 0);
    Ok(())
}

fn erdri(b: &mut Builder<'_>, data: &Path, act: ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "erdri",
        label: "EU RD Platform registry directory (ERDRI.dor)",
        file: ERDRI,
        schema: "directories.erdri",
        versions: &[1],
        url: "https://eu-rd-platform.jrc.ec.europa.eu/erdridor/search",
        licence: "European Commission JRC (reuse with source acknowledgement, Decision 2011/833/EU)",
        class: LicenceClass::Unknown,
        scope: "rare-disease registries in the EU directory flagged relevant to epilepsy/DEE or the slice",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges, mut excluded) = (0, 0, 0);
    for i in 0..src.env.records.len() {
        let r = &src.env.records[i];
        if r["relevance"]["relevant"].as_bool() != Some(true) || r["enabled"].as_bool() == Some(false) {
            excluded += 1;
            continue;
        }
        let rec = src.record(b, i);
        let id = research::asset_id(b, s(r, "id"));
        let mut facts = Vec::new();
        for key in [
            "acronym",
            "medical_area",
            "country",
            "recruitment_start",
            "recruitment_end",
        ] {
            if let Some(v) = opt(r, key) {
                facts.push((key.to_owned(), v));
            }
        }
        for t in strs(r, "registry_types") {
            facts.push(("registry_type".into(), t.to_owned()));
        }
        let homepage = opt(r, "homepage");
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label: s(r, "name").to_owned(),
                kind: AssetKind::Registry,
                category: "erdri_registry".into(),
                holder: None,
                holder_name: None,
                access: Access {
                    route: if homepage.is_some() {
                        "official_page"
                    } else {
                        "contact_holder"
                    }
                    .into(),
                    url: homepage.or_else(|| opt(r, "human_url")),
                    note: None,
                },
                facts,
                verify_url: opt(r, "human_url"),
                release: true,
                records: vec![rec],
            },
        ));
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Observed,
            level: LinkLevel::Exact,
        };
        for c in r["orpha_codes"].as_array().into_iter().flatten() {
            let code = s(c, "code");
            if let Some(cid) = research::rare_condition(b, code) {
                edges += via.link(b, &id, Relation::ResourceFor, &cid, format!("registry lists {code}"));
            }
        }
    }
    src.finish(b, nodes, edges, excluded);
    Ok(())
}

/// `Cellosaurus:CVCL_0G87` → `CVCL:0G87`, the contract prefix the KGX caches and SSSOM sets use,
/// so the same cell line from both caches is one node.
pub fn model_id(id: &str) -> String {
    match id.strip_prefix("Cellosaurus:CVCL_") {
        Some(rest) => format!("CVCL:{rest}"),
        None => id.to_owned(),
    }
}

/// Repository that distributes a model (a provider / collection registry entry), when listed.
fn provider(r: &Value) -> Option<String> {
    r["registries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|x| {
            let c = s(x, "category").to_ascii_lowercase();
            c.contains("provider") || c.contains("collection") || c.contains("biobank")
        })
        .and_then(|p| opt(p, "database"))
}

/// Access route of a model: a provider/collection registry entry (order or request page) first.
fn model_access(r: &Value) -> Access {
    let provider = r["registries"].as_array().into_iter().flatten().find(|x| {
        let c = s(x, "category").to_ascii_lowercase();
        c.contains("provider") || c.contains("collection") || c.contains("biobank")
    });
    match provider {
        Some(p) => Access {
            route: "repository_order".into(),
            url: opt(p, "url"),
            note: Some(format!("{} {}", s(p, "database"), s(p, "accession")).trim().to_owned()),
        },
        None => Access {
            route: "official_page".into(),
            url: opt(r, "url"),
            note: None,
        },
    }
}

fn models(b: &mut Builder<'_>, data: &Path, act: ActivityIdx) -> Result<(), IngestError> {
    let mut files: Vec<String> = std::fs::read_dir(data.join(MODELS_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .map(|n| format!("{MODELS_DIR}/{n}"))
        .collect();
    files.sort();
    for file in &files {
        let spec = Spec {
            source: "models",
            label: "Research models: Alliance / IMPC model organisms, Cellosaurus cell lines",
            file,
            schema: "models.assets",
            versions: &[1],
            url: "https://www.alliancegenome.org ; https://www.cellosaurus.org",
            licence: "CC-BY-4.0 (Alliance of Genome Resources, Cellosaurus, IMPC)",
            class: LicenceClass::Open,
            scope: "models and cell lines for the slice genes (one file per gene)",
        };
        let Some(src) = research::open(b, data, &spec, act)? else {
            continue;
        };
        let (mut nodes, mut edges, mut excluded) = (0, 0, 0);
        for i in 0..src.env.records.len() {
            let r = &src.env.records[i];
            if r["excluded"].as_bool() == Some(true) {
                excluded += 1;
                continue;
            }
            let Some(id) = opt(r, "model_id").map(|m| research::asset_id(b, &model_id(&m))) else {
                excluded += 1;
                continue;
            };
            let rec = src.record(b, i);
            let kind = if s(r, "asset_type") == "cell_model" {
                AssetKind::CellLine
            } else {
                AssetKind::Model
            };
            let mut facts = vec![("source".to_owned(), s(r, "source").to_owned())];
            if let Some(o) = opt(&r["organism"], "label") {
                facts.push(("organism".into(), o));
            }
            if let Some(a) = opt(&r["allele"], "label") {
                facts.push(("allele".into(), a));
            }
            if let Some(z) = opt(r, "zygosity") {
                facts.push(("zygosity".into(), z));
            }
            if let Some(g) = opt(&r["model_gene"], "symbol") {
                facts.push(("model_gene".into(), g));
            }
            if let Some(basis) = opt(&r["gene_link"], "basis") {
                facts.push(("gene_link".into(), basis));
            }
            let label = opt(r, "label").unwrap_or_else(|| id.clone());
            nodes += usize::from(research::asset(
                b,
                Asset {
                    id: id.clone(),
                    label: strip_tags(&label),
                    kind,
                    category: s(r, "asset_type").to_owned(),
                    holder: None,
                    holder_name: provider(r),
                    access: model_access(r),
                    facts,
                    verify_url: opt(r, "url"),
                    release: true,
                    records: vec![rec],
                },
            ));
            let observed = s(&r["gene_link"], "kind") == "observed";
            let via = Via {
                act,
                rec,
                kind: if observed {
                    EdgeKind::Observed
                } else {
                    EdgeKind::Inferred
                },
                level: LinkLevel::Gene,
            };
            let human = s(&r["gene_link"], "human_gene");
            let human = if human.is_empty() { s(r, "gene") } else { human };
            if let Some(gid) = research::gene(b, human) {
                let reason = format!("{} model of {}", s(r, "source"), s(r, "gene"));
                edges += via.link(b, &id, Relation::ModelOf, &gid, reason);
            }
            for d in r["diseases"].as_array().into_iter().flatten() {
                let did = d
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| opt(d, "id"))
                    .unwrap_or_default();
                if let Some(cid) = research::rare_condition(b, &did) {
                    let via = Via {
                        level: LinkLevel::Exact,
                        ..via
                    };
                    edges += via.link(b, &id, Relation::ModelOf, &cid, format!("source disease {did}"));
                }
            }
        }
        src.finish(b, nodes, edges, excluded);
    }
    Ok(())
}

/// `stxbp1b<sup>s3038/s3038</sup>` → `stxbp1b s3038/s3038` (Alliance labels carry HTML markup).
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                tag = true;
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
            }
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn strip_markup() {
        assert_eq!(
            super::strip_tags("stxbp1b<sup>s3038/s3038</sup>"),
            "stxbp1b s3038/s3038"
        );
    }
}
