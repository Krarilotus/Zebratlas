//! Curated patient organisations (`orgs.organisations` v1, SOURCES.md and sources/orgs.md):
//! patient groups, foundations, family networks, expert centres and org-run registries, each with
//! verbatim-quoted covers (gene + condition text, exact or related) and official channels.
//!
//! Every cover becomes `serves_gene` (curated; a related cover keeps level `related`); a cover
//! whose condition text names an atlas condition also becomes `serves_condition`. Absent cache →
//! coverage says so; nothing fails.

use std::path::Path;

use atlas_core::graph::{
    Channel, Coverage, LinkLevel, OrgKind, Organisation, RecordHash, Relation, SourceRecord, activity,
};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{Locator, SourceEntity};
use serde_json::Value;

use super::builder::{Builder, NewEdge, normalize_name};
use super::cache;
use super::trials::Dicts;
use crate::error::IngestError;

pub const FILE: &str = "cache/orgs/organisations.json";
/// Round 3 (groups-wide): the same schema, a superset of [`FILE`] plus organisations found worldwide.
pub const WIDE: &str = "cache/groups-wide/organisations.json";
/// Verified additions acquired separately; never overwrite another producer's cache.
pub const SUPPLEMENT: &str = "cache/bench-fixes/organisations.json";

fn s<'v>(v: &'v Value, k: &str) -> &'v str {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim()
}

fn kind(k: &str) -> OrgKind {
    match k {
        "patient_group" | "foundation" | "family_network" | "alliance" => OrgKind::PatientGroup,
        "expert_centre" | "expert_center" => OrgKind::ExpertCentre,
        _ => OrgKind::Other,
    }
}

pub fn ingest(b: &mut Builder<'_>, data: &Path, dicts: &Dicts) -> Result<(), IngestError> {
    ingest_file(b, data, dicts, FILE, "orgs", activity::INGEST_ORGS)?;
    ingest_file(
        b,
        data,
        dicts,
        WIDE,
        "groups-wide",
        "activity:ingest-organisations-groups-wide",
    )?;
    ingest_file(
        b,
        data,
        dicts,
        SUPPLEMENT,
        "organisations-supplement",
        "activity:ingest-organisations-supplement",
    )
}

/// One `orgs.organisations` file. Organisations already read from an earlier file keep their node;
/// the record is added and the covers merge into the same edges.
fn ingest_file(
    b: &mut Builder<'_>,
    data: &Path,
    dicts: &Dicts,
    file: &str,
    source: &str,
    activity_id: &str,
) -> Result<(), IngestError> {
    let path = data.join(file);
    let mut coverage = Coverage {
        source: source.into(),
        label: "Patient organisations and expert centres (curated)".into(),
        status: "absent".into(),
        files: vec![file.into()],
        licence_class: Some(atlas_core::graph::LicenceClass::Unknown),
        scope: "organisations found by multilingual web and directory search for the slice genes, curated with verbatim quotes"
            .into(),
        ..Coverage::default()
    };
    let act = b.start(activity_id, "Curated patient organisations and expert centres", &[]);
    if !path.exists() {
        b.finish(act, &[]);
        b.data.coverage.push(coverage);
        return Ok(());
    }
    let env = cache::read_envelope(&path, "orgs.organisations", &[1])?;
    let entity = b.entity(SourceEntity {
        id: format!("source:{file}"),
        url: "curated: rare_atlas.sources.orgs_build (snapshots in data/cache/orgs/pages)".into(),
        file: file.into(),
        version: Some(format!("{} v{}", env.schema, env.version)),
        retrieved_at: env.header_str("retrieved_at").map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        licence: Some("curated facts and short quotes from the organisations' public pages".into()),
    });
    super::research::licence(
        b,
        entity,
        "curated facts and short quotes from the organisations' public pages",
        atlas_core::graph::LicenceClass::Unknown,
    );
    b.data.provenance.activity_mut(act).used.push(entity);
    coverage.status = "loaded".into();
    coverage.retrieved_at = env.header_str("retrieved_at").map(str::to_owned);
    coverage.header_checksums_verified = u64::from(env.header_verified);
    coverage.header_checksums_failed = u64::from(!env.header_verified);
    // condition names in any language: Wikidata *labels* (not aliases) of items with one condition
    let mut wiki: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for name in b.data.wiki_names.iter().filter(|n| !n.alias) {
        let item = &b.data.wiki_items[name.item as usize];
        if let [one] = item.targets.as_slice()
            && b.atlas.disease(one).is_some()
        {
            wiki.entry(normalize_name(&name.text)).or_insert_with(|| one.clone());
        }
    }
    let (mut n, mut genes_missing, mut conditions) = (0usize, 0usize, 0usize);
    for (i, r) in env.records.iter().enumerate() {
        let id = s(r, "id").to_owned();
        if id.is_empty() {
            continue;
        }
        let rec = b.record(SourceRecord {
            entity,
            locator: Locator::Record(format!("records[{i}]")),
            id: id.clone(),
            url: Some(s(r, "url").to_owned()).filter(|u| !u.is_empty()),
            fetched_at: Some(s(r, "fetched_at").to_owned()).filter(|u| !u.is_empty()),
            hash: RecordHash::CanonicalJson,
            sha256: cache::canonical_sha256(r),
        });
        if !env.header_verified {
            super::quarantine::mark(b, rec, "organisation envelope checksum failed");
        }
        if s(&r["curation"], "notes").to_ascii_lowercase().contains("placeholder") {
            super::quarantine::mark(
                b,
                rec,
                "curator reports placeholder content; operating organisation unverified",
            );
        }
        let channels: Vec<Channel> = r["channels"]
            .as_array()
            .into_iter()
            .flatten()
            // A purpose-specific address (e.g. grant enquiries) is not a family contact.
            // The complete channel, its purpose and evidence stay in the source record.
            .filter(|c| s(c, "purpose").is_empty() || s(c, "purpose") == "family support")
            .map(|c| Channel {
                kind: s(c, "type").to_owned(),
                value: s(c, "value").to_owned(),
                evidence_url: Some(s(c, "evidence_url").to_owned()).filter(|u| !u.is_empty()),
            })
            .collect();
        if let Some((NodeKind::Organisation, have)) = b.node(&id) {
            b.data.orgs[have as usize].records.push(rec);
        }
        let channel = |k: &str| channels.iter().find(|c| c.kind == k).map(|c| c.value.clone());
        let name_en = s(r, "name_en");
        let original = s(r, "name_original");
        let name = if name_en.is_empty() { original } else { name_en };
        let curation = &r["curation"];
        let known = b.node(&id).is_some();
        if !known {
            b.register(&id, NodeKind::Organisation, b.data.orgs.len());
        }
        let org = Organisation {
            id: id.clone(),
            name: name.to_owned(),
            kind: kind(s(r, "kind")),
            url: channel("website"),
            contact_url: channel("contact_form"),
            country: Some(s(r, "country").to_owned()).filter(|c| !c.is_empty()),
            country_basis: Some(s(r, "country_basis").to_owned()).filter(|c| !c.is_empty()),
            description: Some(if original != name && !original.is_empty() {
                format!("{} ({})", s(r, "kind").replace('_', " "), original)
            } else {
                s(r, "kind").replace('_', " ")
            }),
            languages: r["languages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            verified_on: Some(s(curation, "at").to_owned())
                .filter(|x| !x.is_empty())
                .or_else(|| Some(s(r, "fetched_at").to_owned()).filter(|x| !x.is_empty())),
            channels,
            records: vec![rec],
        };
        if !known {
            b.data.orgs.push(org);
        }
        n += 1;
        for c in r["covers"].as_array().into_iter().flatten() {
            let exact = s(c, "match") == "exact";
            let level = if exact { LinkLevel::Curated } else { LinkLevel::Related };
            let quote: String = s(c, "quote").chars().take(400).collect();
            let text = s(c, "condition_text");
            let reason = format!(
                "{} focus on \"{text}\": \"{quote}\" ({})",
                if exact { "exact" } else { "related" },
                s(c, "evidence_url")
            );
            let gene = s(c, "gene");
            if !gene.is_empty() && !coverage.genes.iter().any(|g| g == gene) {
                coverage.genes.push(gene.to_owned());
            }
            match b.gene_id(gene) {
                Some(g) => {
                    let e = NewEdge {
                        from: &id,
                        relation: Relation::ServesGene,
                        to: &g,
                        kind: EdgeKind::Observed,
                        level,
                        reason: reason.clone(),
                        activity: act,
                    };
                    b.edge(e, &[rec]);
                }
                None if !gene.is_empty() => genes_missing += 1,
                None => {}
            }
            let key = normalize_name(text);
            let mut targets: Vec<String> = dicts.by_name.get(&key).cloned().unwrap_or_default();
            if targets.is_empty()
                && let Some(t) = wiki.get(&key)
            {
                targets.push(t.clone());
            }
            for d in targets {
                let e = NewEdge {
                    from: &id,
                    relation: Relation::ServesCondition,
                    to: &d,
                    kind: EdgeKind::Observed,
                    level,
                    reason: reason.clone(),
                    activity: act,
                };
                b.edge(e, &[rec]);
                conditions += 1;
            }
        }
    }
    coverage.records = n as u64;
    b.finish(
        act,
        &[
            ("organisations", n),
            ("serves_condition", conditions),
            ("skipped:gene-not-in-atlas", genes_missing),
        ],
    );
    b.data.coverage.push(coverage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use atlas_core::Graph;
    use atlas_core::graph::RecordWithhold;
    use serde_json::json;

    #[test]
    fn purpose_specific_contacts_and_unverified_groups_stay_in_source_records() {
        let atlas = fixtures::atlas();
        let d = fixtures::TempData::new();
        let mut b = fixtures::builder(&atlas);
        let dicts = Dicts::new(&b, &Default::default());
        let cover =
            json!({"gene":"STXBP1", "condition_text":"developmental and epileptic encephalopathy 4", "match":"exact"});
        d.envelope(
            SUPPLEMENT,
            "orgs.organisations",
            json!([
                {"id":"org:checked", "name_en":"Fixture foundation", "kind":"foundation", "covers":[cover],
                 "channels":[{"type":"website", "value":"https://fixture.example"},
                             {"type":"email", "value":"grants@fixture.example", "purpose":"grant enquiries"}]},
                {"id":"org:unfinished", "name_en":"Fixture unfinished", "kind":"foundation", "covers":[cover],
                 "curation":{"notes":"mission and contact are placeholders"}}
            ]),
        );
        ingest(&mut b, d.path(), &dicts).unwrap();
        let g = Graph::new(b.data);
        let key = g.node("org:checked").unwrap();
        assert_eq!(g.org(key.idx).channels.len(), 1);
        assert!(g.node_withheld(key).is_none());
        assert!(
            g.node_withheld(g.node("org:unfinished").unwrap())
                .unwrap()
                .contains("placeholder")
        );
        assert_eq!(g.data().records.len(), 2);
        assert_eq!(g.coverage().last().unwrap().genes, vec!["STXBP1"]);
        let env = cache::read_envelope(&d.path().join(SUPPLEMENT), "orgs.organisations", &[1]).unwrap();
        assert_eq!(env.records[0]["channels"].as_array().unwrap().len(), 2);
    }
}
