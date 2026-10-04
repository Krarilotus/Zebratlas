//! Therapy programmes and regulatory signals (S4, D39 "therapy programmes"): who is working on
//! what, at which stage, and who to talk to. Never a treatment recommendation (D36 §4).
//!
//! - `pipelines.evidence` v1 (SOURCES.md): company-reported programmes → `Asset` kind
//!   `programme` (`targets` → gene, `held_by` → sponsor); financing rounds become facts of the
//!   programmes they name; excluded records are counted.
//! - `regulatory.signals` v1: FDA / EMA / EC orphan designations, register entries and
//!   authorisations → `Asset` kind `designation` (`studied_for` → rare condition named exactly in
//!   the record, `related_to` → gene named as a token, `held_by` → sponsor).

use std::path::Path;

use atlas_core::graph::{Access, Asset, AssetKind, LicenceClass, LinkLevel, Relation, activity};
use atlas_core::node::EdgeKind;
use serde_json::Value;

use super::builder::Builder;
use super::research::{self, Spec, Via, opt, s, strs};
use crate::error::IngestError;

pub const PIPELINES: &str = "cache/pipelines/evidence.json";
pub const REGULATORY: &str = "cache/regulatory/signals.json";

/// Shortest condition name a regulatory text match may link by (shorter names are too generic).
const MIN_NAME: usize = 6;

pub fn ingest(b: &mut Builder<'_>, data: &Path) -> Result<(), IngestError> {
    let act = b.start(
        activity::INGEST_PROGRAMMES,
        "Therapy programmes (company reports) and regulatory signals (FDA, EMA, EC)",
        &[],
    );
    pipelines(b, data, act)?;
    regulatory(b, data, act)?;
    b.finish(act, &[]);
    Ok(())
}

fn pipelines(b: &mut Builder<'_>, data: &Path, act: atlas_core::provenance::ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "pipelines",
        label: "Therapy programmes (company and investor reports)",
        file: PIPELINES,
        schema: "pipelines.evidence",
        versions: &[1],
        url: "curated: rare_atlas pipelines recipe (company press releases, investor reports, registries)",
        licence: "facts and short quotes from company and investor pages; each site's terms apply",
        class: LicenceClass::Unknown,
        scope: "company-reported programmes and modality backers for the DEE slice genes",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges, mut excluded) = (0, 0, 0);
    let mut financing = Vec::new();
    for i in 0..src.env.records.len() {
        let r = &src.env.records[i];
        if r["excluded"].as_bool() == Some(true) {
            excluded += 1;
            continue;
        }
        let rec = src.record(b, i);
        let kind = s(r, "record_type");
        if kind == "financing" {
            financing.push((i, rec));
            continue;
        }
        let id = research::asset_id(b, &opt(r, "program_id").unwrap_or_else(|| s(r, "id").to_owned()));
        let sponsor = s(r, "sponsor").to_owned();
        let name = opt(r, "program_name")
            .or_else(|| opt(r, "platform_name"))
            .unwrap_or_else(|| id.clone());
        let mut facts = Vec::new();
        for key in [
            "modality",
            "mechanism",
            "variant_effect",
            "stage",
            "stage_scope",
            "evidence_status",
        ] {
            if let Some(v) = opt(r, key) {
                facts.push((key.to_owned(), v));
            }
        }
        if let Some(d) = opt(r, "source_date") {
            facts.push(("reported_on".into(), d));
        }
        for t in strs(r, "trial_ids") {
            facts.push(("trial".into(), t.to_owned()));
        }
        let url = opt(r, "url");
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label: name,
                kind: AssetKind::Programme,
                category: kind.to_owned(),
                holder: None,
                holder_name: Some(sponsor.clone()).filter(|x| !x.is_empty()),
                access: Access {
                    route: "contact_holder".into(),
                    url: url.clone(),
                    note: None,
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
            kind: EdgeKind::Observed,
            level: LinkLevel::Gene,
        };
        for g in strs(r, "genes") {
            if let Some(gid) = research::gene(b, g) {
                edges += via.link(b, &id, Relation::Targets, &gid, format!("programme gene \"{g}\""));
            }
        }
        if let Some(org) = research::held_by(b, &id, None, &sponsor, via) {
            edges += 1;
            set_holder(b, &id, org);
        }
    }
    // financing rounds: facts of the programmes they name
    for (i, rec) in financing {
        let r = &src.env.records[i];
        let what = [s(r, "round"), s(r, "amount"), s(r, "currency")]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let fact = format!(
            "{} ({})",
            if what.is_empty() { "financing" } else { &what },
            s(r, "source_date")
        );
        for p in strs(r, "program_ids") {
            if let Some((atlas_core::node::NodeKind::Asset, a)) = b.node(p) {
                let a = &mut b.data.assets[a as usize];
                a.facts.push(("financing".into(), fact.clone()));
                a.records.push(rec);
            }
        }
    }
    src.finish(b, nodes, edges, excluded);
    Ok(())
}

fn set_holder(b: &mut Builder<'_>, asset: &str, org: String) {
    if let Some((atlas_core::node::NodeKind::Asset, i)) = b.node(asset) {
        b.data.assets[i as usize].holder.get_or_insert(org);
    }
}

/// First non-empty string among `keys`.
fn first(r: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| opt(r, k))
}

fn regulatory(b: &mut Builder<'_>, data: &Path, act: atlas_core::provenance::ActivityIdx) -> Result<(), IngestError> {
    let spec = Spec {
        source: "regulatory",
        label: "Regulatory signals (FDA orphan designations, EMA, European Commission)",
        file: REGULATORY,
        schema: "regulatory.signals",
        versions: &[1],
        url: "https://www.accessdata.fda.gov/scripts/opdlisting/oopd/ ; https://www.ema.europa.eu/en/medicines",
        licence: "FDA records: US public domain; EMA / European Commission: reuse with source acknowledgement",
        class: LicenceClass::Unknown,
        scope: "official orphan designations, register entries and authorisations whose text names a DEE-slice condition or gene",
    };
    let Some(src) = research::open(b, data, &spec, act)? else {
        return Ok(());
    };
    let (mut nodes, mut edges) = (0, 0);
    for i in 0..src.env.records.len() {
        let rec = src.record(b, i);
        let r = &src.env.records[i];
        if s(r, "id").is_empty() {
            continue;
        }
        let id = research::asset_id(b, s(r, "id"));
        let substance = first(r, &["active_substance", "generic_name", "product_name", "trade_name"]);
        let use_ = first(
            r,
            &[
                "intended_use",
                "orphan_designation",
                "therapeutic_indication",
                "condition_name",
            ],
        );
        let label = match (&substance, &use_) {
            (Some(x), Some(u)) => format!("{x}: {u}"),
            (Some(x), None) => x.clone(),
            (None, Some(u)) => u.clone(),
            (None, None) => id.clone(),
        };
        let mut facts = vec![(
            "record_type".to_owned(),
            first(r, &["record_type", "event_type"]).unwrap_or_else(|| "regulatory_record".into()),
        )];
        for (key, from) in [
            (
                "status",
                &[
                    "orphan_designation_status",
                    "designation_status",
                    "medicine_status",
                    "marketing_authorisation_status",
                ][..],
            ),
            (
                "date",
                &[
                    "designation_or_refusal_date",
                    "designation_date",
                    "marketing_authorisation_date",
                    "event_date",
                ][..],
            ),
            (
                "indication",
                &["intended_use", "orphan_designation", "therapeutic_indication"][..],
            ),
            ("trade_name", &["trade_name"][..]),
        ] {
            if let Some(v) = first(r, from).filter(|v| v != "not_assessed") {
                facts.push((key.to_owned(), v));
            }
        }
        facts.push(("source".into(), s(r, "source").to_owned()));
        let sponsor = first(r, &["sponsor", "sponsor_name"]).unwrap_or_default();
        let url = opt(r, "url");
        nodes += usize::from(research::asset(
            b,
            Asset {
                id: id.clone(),
                label,
                kind: AssetKind::Designation,
                category: first(r, &["record_type"]).unwrap_or_else(|| "regulatory_record".into()),
                holder: None,
                holder_name: Some(sponsor.clone()).filter(|x| !x.is_empty()),
                access: Access {
                    route: "official_page".into(),
                    url: url.clone(),
                    note: None,
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
            level: LinkLevel::Exact,
        };
        for m in r["mapping_candidates"].as_array().into_iter().flatten() {
            let matched = s(m, "matched_name");
            let target = s(m, "id");
            match s(m, "entity_type") {
                "disease"
                    if matches!(s(m, "basis"), "exact_name_mention" | "exact_synonym_mention")
                        && matched.chars().count() >= MIN_NAME =>
                {
                    if let Some(cid) = research::rare_condition(b, target) {
                        edges += via.link(b, &id, Relation::StudiedFor, &cid, format!("names \"{matched}\""));
                    }
                }
                "gene" => {
                    if let Some(gid) = research::gene(b, target) {
                        let via = Via {
                            level: LinkLevel::Gene,
                            ..via
                        };
                        edges += via.link(b, &id, Relation::RelatedTo, &gid, format!("names gene \"{matched}\""));
                    }
                }
                _ => {}
            }
        }
        if !sponsor.is_empty()
            && let Some(org) = research::held_by(b, &id, None, &sponsor, via)
        {
            edges += 1;
            set_holder(b, &id, org);
        }
    }
    src.finish(b, nodes, edges, 0);
    Ok(())
}
