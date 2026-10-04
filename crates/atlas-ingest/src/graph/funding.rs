//! Funding (D39 "funding"): open calls a researcher can apply to, and who already funds research
//! on a gene outside the US (RePORTER grants are read in `works`).
//!
//! - `funders.calls` v1: official funding calls → `Asset` kind `funding_call`, `funds` → gene
//!   (the call's own gene mapping), `held_by` → funder; excluded calls counted.
//! - `research_intl.{cordis,gtr,kaken,europepmc_grants}` v1: projects → `Grant`, `about_gene`
//!   (level `text`: the gene named in title/abstract/keywords, `matched_in` kept in the reason),
//!   `awarded_to` → lead organisation.

use std::path::Path;

use atlas_core::graph::{Access, Asset, AssetKind, Grant, LicenceClass, LinkLevel, OrgKind, Relation, activity};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::ActivityIdx;
use serde_json::Value;

use super::builder::Builder;
use super::research::{self, Spec, Via, opt, s, strs};
use crate::error::IngestError;

pub const CALLS: &str = "cache/funders/calls.json";

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_FUNDING,
        "Funding calls and international research projects",
        &[],
    );
    calls(b, data, act)?;
    for g in &GRANT_SOURCES {
        grants(b, data, act, g)?;
    }
    b.finish(act, &[]);
    Ok(())
}

fn calls(b: &mut Builder<'_>, data: &Path, act: ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "funders",
        label: "Funding calls (official funder pages)",
        file: CALLS,
        schema: "funders.calls",
        versions: &[1],
        url: "curated: rare_atlas funders (official funder pages, snapshots in data/cache/funders/snapshots)",
        licence: "facts and short quotes from official funder pages",
        class: LicenceClass::Unknown,
        scope: "funding calls and programmes of patient foundations and public funders for the slice genes",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges, mut excluded) = (0, 0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        if s(r, "status") != "included" {
            excluded += 1;
            continue;
        }
        let id = research::asset_id(b, s(r, "id"));
        let funder = s(r, "funder").to_owned();
        let mut facts = Vec::new();
        for key in ["application_status", "application_status_reason", "as_of"] {
            if let Some(v) = opt(r, key) {
                facts.push((key.to_owned(), v));
            }
        }
        if let Some(sum) = opt(&r["scope"], "summary") {
            facts.push(("scope".into(), sum));
        }
        if let Some(sum) = opt(&r["eligibility"], "summary") {
            facts.push(("eligibility".into(), sum));
        }
        if let Some(a) = amount(&r["amount"]) {
            facts.push(("amount".into(), a));
        }
        for d in r["deadlines"].as_array().into_iter().flatten() {
            let text = d
                .as_str()
                .map(str::to_owned)
                .or_else(|| opt(d, "date"))
                .unwrap_or_default();
            if !text.is_empty() {
                facts.push(("deadline".into(), text));
            }
        }
        let url = opt(r, "url");
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label: s(r, "title").to_owned(),
                kind: AssetKind::FundingCall,
                category: "funding_call".into(),
                holder: None,
                holder_name: Some(funder.clone()).filter(|x| !x.is_empty()),
                access: Access {
                    route: "apply".into(),
                    url: url.clone(),
                    note: r["caveats"]
                        .as_array()
                        .and_then(|c| c.first())
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
                facts,
                verify_url: url,
                release: true,
                records: vec![rec],
            },
        ));
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Inferred,
            level: LinkLevel::Gene,
        };
        for m in r["community_mappings"].as_array().into_iter().flatten() {
            if let Some(gid) = research::gene(b, s(m, "gene")) {
                let reason = format!("{} ({})", s(m, "relation"), s(m, "basis"));
                edges += via.link(b, &id, Relation::Funds, &gid, reason);
            }
        }
        if let Some(org) = research::held_by(b, &id, None, &funder, via) {
            edges += 1;
            if let Some((NodeKind::Asset, a)) = b.node(&id) {
                b.data.assets[a as usize].holder.get_or_insert(org);
            }
        }
    }
    src.finish(b, nodes, edges, excluded);
    Ok(())
}

fn amount(a: &Value) -> Option<String> {
    let cur = s(a, "currency");
    let num = |k: &str| a.get(k).and_then(Value::as_f64).map(|v| format!("{v:.0}"));
    match (num("minimum"), num("maximum")) {
        (Some(lo), Some(hi)) => Some(format!("{lo}–{hi} {cur}")),
        (None, Some(hi)) => Some(format!("up to {hi} {cur}")),
        _ => None,
    }
    .map(|x| match opt(a, "basis") {
        Some(basis) => format!("{} ({basis})", x.trim()),
        None => x.trim().to_owned(),
    })
}

/// One international grant source of `research_intl`.
struct GrantSource {
    key: &'static str,
    file: &'static str,
    schema: &'static str,
    prefix: &'static str,
    label: &'static str,
    url: &'static str,
    licence: &'static str,
    class: LicenceClass,
}

const GRANT_SOURCES: [GrantSource; 4] = [
    GrantSource {
        key: "cordis",
        file: "cache/research_intl/cordis.json",
        schema: "research_intl.cordis",
        prefix: "CORDIS",
        label: "EU research projects (CORDIS)",
        url: "https://cordis.europa.eu/datalab/sparql",
        licence: "CC-BY-4.0 (European Commission, CORDIS)",
        class: LicenceClass::Open,
    },
    GrantSource {
        key: "gtr",
        file: "cache/research_intl/gtr.json",
        schema: "research_intl.gtr",
        prefix: "GTR",
        label: "UK research projects (UKRI Gateway to Research)",
        url: "https://gtr.ukri.org/gtr/api/projects",
        licence: "Open Government Licence v3.0 (UKRI)",
        class: LicenceClass::Open,
    },
    GrantSource {
        key: "kaken",
        file: "cache/research_intl/kaken.json",
        schema: "research_intl.kaken",
        prefix: "KAKEN",
        label: "Japanese research projects (KAKEN)",
        url: "https://kaken.nii.ac.jp",
        licence: "KAKEN terms of use (MEXT; cite the source)",
        class: LicenceClass::Unknown,
    },
    GrantSource {
        key: "europepmc_grants",
        file: "cache/research_intl/europepmc_grants.json",
        schema: "research_intl.europepmc_grants",
        prefix: "EPMC.GRANT",
        label: "Grants of Europe PMC funders (GRIST)",
        url: "https://www.ebi.ac.uk/europepmc/GristAPI/rest/get/",
        licence: "Europe PMC terms of use (grant metadata)",
        class: LicenceClass::Unknown,
    },
];

/// Lead organisation name of a project record, per source shape.
fn lead_org(r: &Value) -> Option<String> {
    opt(&r["coordinator"], "name")
        .or_else(|| {
            r["organisations"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|o| s(o, "role") == "LEAD_ORG")
                .and_then(|o| opt(o, "name"))
        })
        .or_else(|| strs(r, "institutions").next().map(str::to_owned))
}

fn funder_name(r: &Value, g: &GrantSource) -> String {
    opt(&r["funder"], "name")
        .or_else(|| opt(r, "lead_funder"))
        .or_else(|| opt(&r["funding_scheme"], "type"))
        .or_else(|| opt(r, "research_category"))
        .unwrap_or_else(|| g.prefix.to_owned())
}

fn grants(b: &mut Builder<'_>, data: &Path, act: ActivityIdx, g: &GrantSource) -> Result<(), IngestError> {
    let spec = Spec {
        source: g.key,
        label: g.label,
        file: g.file,
        schema: g.schema,
        versions: &[1],
        url: g.url,
        licence: g.licence,
        class: g.class,
        scope: "projects naming a slice gene, its protein or condition in title, abstract or keywords",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges) = (0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        let raw = s(r, "id");
        if raw.is_empty() {
            continue;
        }
        let id = format!("{}:{raw}", g.prefix);
        let org = lead_org(r).unwrap_or_default();
        match b.node(&id) {
            Some((NodeKind::Grant, gi)) => b.data.grants[gi as usize].records.push(rec),
            Some(_) => continue,
            None => {
                b.register(&id, NodeKind::Grant, b.data.grants.len());
                let year = |k: &str| s(r, k).get(..4).and_then(|y| y.parse::<u16>().ok());
                let years: Vec<u16> = match (year("start"), year("end")) {
                    (Some(a), Some(z)) if a <= z && z - a < 30 => (a..=z).collect(),
                    (Some(a), _) => vec![a],
                    _ => Vec::new(),
                };
                b.data.grants.push(Grant {
                    id: id.clone(),
                    title: s(r, "title").to_owned(),
                    activity_code: opt(&r["funding_scheme"], "type")
                        .or_else(|| opt(r, "grant_category"))
                        .or_else(|| opt(r, "research_category"))
                        .or_else(|| opt(r, "type"))
                        .unwrap_or_default(),
                    agency: funder_name(r, g),
                    organisation: org.clone(),
                    country: String::new(),
                    fiscal_years: years,
                    award_total: None,
                    start: s(r, "start").to_owned(),
                    end: s(r, "end").to_owned(),
                    url: s(r, "url").to_owned(),
                    records: vec![rec],
                });
                nodes += 1;
            }
        }
        let matched: Vec<&str> = strs(r, "matched_in").collect();
        let via = Via {
            act,
            rec,
            kind: EdgeKind::Observed,
            level: LinkLevel::Text,
        };
        for gene in strs(r, "genes") {
            if let Some(gid) = research::gene(b, gene) {
                let reason = format!("gene \"{gene}\" in {}", matched.join(", "));
                edges += via.link(b, &id, Relation::AboutGene, &gid, reason);
            }
        }
        if let Some(oid) = research::org(b, &org, OrgKind::Institution, rec) {
            let via = Via {
                level: LinkLevel::Curated,
                ..via
            };
            edges += via.link(
                b,
                &id,
                Relation::AwardedTo,
                &oid,
                format!("lead organisation \"{org}\""),
            );
        }
    }
    src.finish(b, nodes, edges, 0);
    Ok(())
}
