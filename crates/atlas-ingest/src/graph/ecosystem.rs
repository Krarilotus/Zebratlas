//! Ecosystem v1 envelopes: organisation nodes, official actions and exact scope links.
//! Exclusions and unavailable actions stay in source records, never become active doors.

use super::research::{opt, s, strs};
use super::{
    builder::{Builder, NewEdge},
    cache,
};
use crate::error::IngestError;
use atlas_core::graph::{
    Channel, Coverage, Initiative, InitiativeScope, LinkLevel, OfficialAction, OrgKind, Organisation, RecordHash,
    Relation, ReportedAction, SourceRecord,
};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{EntityIdx, Locator, SourceEntity};
use atlas_core::withhold::{KeyKind, Withhold};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub const INITIATIVES: &str = "cache/ecosystem/initiatives.json";
pub const EDGES: &str = "cache/ecosystem/edges.json";
const ACT: &str = "activity:ingest-ecosystem";

fn invalid(path: &Path, detail: &str) -> IngestError {
    IngestError::Schema {
        path: path.into(),
        found: detail.into(),
        expected: "checksum-verified ecosystem v1".into(),
    }
}

fn envelope(
    b: &mut Builder<'_>,
    data: &Path,
    file: &str,
    schema: &str,
) -> Result<(cache::Envelope, EntityIdx), IngestError> {
    let path = data.join(file);
    let env = cache::read_envelope(&path, schema, &[1])?;
    if !env.header_verified {
        return Err(invalid(&path, "record checksum mismatch"));
    }
    let entity = b.entity(SourceEntity {
        id: format!("source:{file}"),
        file: file.into(),
        url: format!("file:///{}", path.to_string_lossy().replace('\\', "/")),
        version: Some(format!("{schema} v1")),
        retrieved_at: env.header_str("retrieved_at").map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(&path).map_err(IngestError::io(&path))?.len(),
        licence: Some("unknown; original factual summaries and official links only".into()),
    });
    Ok((env, entity))
}

fn record(b: &mut Builder<'_>, entity: EntityIdx, r: &Value, i: usize) -> u32 {
    b.record(SourceRecord {
        entity,
        locator: Locator::Record(format!("records[{i}]")),
        id: s(r, "id").into(),
        url: opt(r, "source_url").or_else(|| opt(r, "url")),
        fetched_at: opt(r, "retrieved_at"),
        hash: RecordHash::CanonicalJson,
        sha256: cache::canonical_sha256(r),
    })
}

/// Register and verify the source-byte entities without storing copied page text in the graph.
fn pages(b: &mut Builder<'_>, data: &Path, v: &Value, used: &mut Vec<EntityIdx>) -> Result<(), IngestError> {
    match v {
        Value::Object(map) => {
            if matches!(
                s(v, "sha256_basis"),
                "original HTTP response bytes" | "provided context document bytes; original email not supplied"
            ) && !s(v, "sha256").is_empty()
            {
                let hash = s(v, "sha256");
                if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err(invalid(&data.join(INITIATIVES), "invalid source-byte hash"));
                }
                let file = format!("cache/ecosystem/responses/{hash}.bin");
                let path = data.join(&file);
                let bytes = std::fs::read(&path).map_err(IngestError::io(&path))?;
                if cache::hex(&cache::sha256(&bytes)) != hash {
                    return Err(invalid(&path, "source-byte checksum mismatch"));
                }
                let entity = b.entity(SourceEntity {
                    id: format!("ecosystem-page:{}:{hash}", s(v, "source_url")),
                    file,
                    url: s(v, "source_url").into(),
                    version: opt(v, "version"),
                    retrieved_at: opt(v, "retrieved_at"),
                    sha256: Some(hash.into()),
                    bytes: bytes.len() as u64,
                    licence: Some("unknown; id_link_only".into()),
                });
                if !used.contains(&entity) {
                    used.push(entity);
                }
            }
            for child in map.values() {
                pages(b, data, child, used)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                pages(b, data, child, used)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn public_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty() && !host.contains('@') && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn ingest(b: &mut Builder<'_>, data: &Path, withhold: &Withhold) -> Result<(), IngestError> {
    let act = b.start(ACT, "Official ecosystem scope and action routes", &[]);
    let mut coverage = Coverage {
        source: "ecosystem".into(),
        label: "Existing rare-disease initiatives".into(),
        files: vec![INITIATIVES.into(), EDGES.into()],
        status: "absent".into(),
        scope: "publisher-stated exact scope; broad support is never a gene-specific assertion".into(),
        ..Coverage::default()
    };
    if !data.join(INITIATIVES).exists() {
        b.finish(act, &[]);
        b.data.coverage.push(coverage);
        return Ok(());
    }
    let (env, entity) = envelope(b, data, INITIATIVES, "ecosystem.initiatives")?;
    let mut used = vec![entity];
    let mut ids = HashSet::new();
    let mut active = HashSet::new();
    for (i, r) in env.records.iter().enumerate() {
        let id = s(r, "id");
        if id.is_empty() || !ids.insert(id.to_owned()) || b.node(id).is_some() {
            return Err(invalid(&data.join(INITIATIVES), "empty or duplicate initiative ID"));
        }
        // Suppression removes the node, its metadata and all incident edges on re-ingestion.
        let keys: Vec<_> = withhold.salt().key(KeyKind::Node, id).into_iter().collect();
        if withhold.keys(&keys).is_some() {
            continue;
        }
        let rec = record(b, entity, r, i);
        pages(b, data, &r["prov:wasDerivedFrom"], &mut used)?;
        let mut reported_actions = vec![];
        for route in r["reported_action_routes"].as_array().into_iter().flatten() {
            if !public_url(s(route, "url")) {
                continue;
            }
            let proof = &route["prov:wasDerivedFrom"][0];
            if s(route, "verification") != "founder-reported; original email not supplied"
                || s(proof, "sha256_basis") != "provided context document bytes; original email not supplied"
                || ["sha256", "version", "retrieved_at", "record_locator"]
                    .iter()
                    .any(|key| s(proof, key).is_empty())
            {
                return Err(invalid(&data.join(INITIATIVES), "incomplete reported route evidence"));
            }
            pages(b, data, route, &mut used)?;
            let route_rec = b.record(SourceRecord {
                entity,
                locator: Locator::Record(format!("records[{i}]")),
                id: id.into(),
                url: Some(s(route, "url").into()),
                fetched_at: opt(proof, "retrieved_at"),
                hash: RecordHash::CanonicalJson,
                sha256: cache::canonical_sha256(r),
            });
            reported_actions.push(ReportedAction {
                url: s(route, "url").into(),
                action: s(route, "action").into(),
                outcome: s(route, "outcome").into(),
                audience: s(route, "audience").into(),
                verification: s(route, "verification").into(),
                destination_status: s(route, "destination_status").into(),
                source_version: s(proof, "version").into(),
                sha256: s(proof, "sha256").into(),
                retrieved_at: s(proof, "retrieved_at").into(),
                record_locator: s(proof, "record_locator").into(),
                records: vec![route_rec],
            });
        }
        let verified = r["excluded"] == false && s(r, "status") == "verified";
        if verified {
            pages(b, data, &r["scope"], &mut used)?;
        }
        let scopes = if verified {
            r["scope"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|scope| InitiativeScope {
                    kind: s(scope, "type").into(),
                    label: s(scope, "label").into(),
                    target: opt(scope, "target"),
                })
                .collect()
        } else {
            vec![]
        };
        let mut actions = vec![];
        if verified {
            active.insert(id.to_owned());
            for route in r["action_routes"].as_array().into_iter().flatten() {
                if route["official_route_verified"] != true
                    || s(route, "destination_status") != "ok"
                    || matches!(s(route, "availability"), "closed" | "retired")
                {
                    continue;
                }
                let proof = &route["prov:wasDerivedFrom"][0];
                if !public_url(s(route, "url"))
                    || s(route, "action").is_empty()
                    || s(route, "sha256").is_empty()
                    || s(proof, "sha256_basis") != "original HTTP response bytes"
                    || s(proof, "version").is_empty()
                    || s(route, "sha256") != s(proof, "sha256")
                    || s(route, "retrieved_at") != s(proof, "retrieved_at")
                    || s(proof, "source_url").is_empty()
                    || s(proof, "record_locator").is_empty()
                {
                    return Err(invalid(&data.join(INITIATIVES), "incomplete official route evidence"));
                }
                pages(b, data, route, &mut used)?;
                let mut route_records = vec![];
                for url in [s(proof, "source_url"), s(route, "url")] {
                    route_records.push(b.record(SourceRecord {
                        entity,
                        locator: Locator::Record(format!("records[{i}]")),
                        id: id.into(),
                        url: Some(url.into()),
                        fetched_at: opt(route, "retrieved_at"),
                        hash: RecordHash::CanonicalJson,
                        sha256: cache::canonical_sha256(r),
                    }));
                }
                actions.push(OfficialAction {
                    action: s(route, "action").into(),
                    url: s(route, "url").into(),
                    outcome: s(route, "outcome").into(),
                    audience: s(route, "audience").into(),
                    retrieved_at: s(route, "retrieved_at").into(),
                    sha256: s(route, "sha256").into(),
                    source_url: s(proof, "source_url").into(),
                    source_version: s(proof, "version").into(),
                    record_locator: s(proof, "record_locator").into(),
                    availability: opt(route, "availability"),
                    records: route_records,
                });
            }
        }
        let channels = actions
            .iter()
            .map(|a| Channel {
                kind: "official_action".into(),
                value: a.url.clone(),
                evidence_url: Some(a.source_url.clone()),
            })
            .collect();
        b.register(id, NodeKind::Organisation, b.data.orgs.len());
        b.data.orgs.push(Organisation {
            id: id.into(),
            name: s(r, "name").into(),
            kind: OrgKind::Other,
            url: opt(r, "url"),
            contact_url: actions.first().map(|a| a.url.clone()),
            description: if verified { opt(r, "description") } else { None },
            country: None,
            country_basis: None,
            languages: if verified {
                strs(r, "languages").map(str::to_owned).collect()
            } else {
                vec![]
            },
            verified_on: if verified { opt(r, "retrieved_at") } else { None },
            channels,
            records: vec![rec],
        });
        b.data.initiatives.push(Initiative {
            id: id.into(),
            verified,
            exclusion_reason: opt(r, "exclusion_reason"),
            scopes,
            actions,
            reported_actions,
            research_only: r["research_only"] == true,
            records: vec![rec],
        });
    }
    let mut linked = 0;
    let mut checked_headers = 1;
    if data.join(EDGES).exists() {
        let (edges, entity) = envelope(b, data, EDGES, "ecosystem.edges")?;
        checked_headers += 1;
        used.push(entity);
        for (i, r) in edges.records.iter().enumerate() {
            let from = s(r, "subject");
            if !ids.contains(from) {
                return Err(invalid(
                    &data.join(EDGES),
                    "edge subject is absent from the initiatives envelope",
                ));
            }
            if !active.contains(from) {
                continue;
            }
            let rec = record(b, entity, r, i);
            let raw_target = s(r, "object");
            let relation = match s(r, "relation") {
                "serves_gene" => Relation::ServesGene,
                "serves_condition" => Relation::ServesCondition,
                _ => return Err(invalid(&data.join(EDGES), "unsupported ecosystem relation")),
            };
            let Some(node) = b.data.initiatives.iter().find(|n| n.id == from) else {
                continue;
            };
            if !node.scopes.iter().any(|scope| {
                scope.target.as_deref() == Some(raw_target)
                    && scope.kind
                        == if relation == Relation::ServesGene {
                            "gene"
                        } else {
                            "condition"
                        }
            }) {
                return Err(invalid(
                    &data.join(EDGES),
                    "edge target is absent from exact publisher-stated scope",
                ));
            }
            let target = match relation {
                Relation::ServesGene => b.atlas.gene(raw_target).map(|g| b.atlas.gene_at(g).id().to_owned()),
                _ => b
                    .atlas
                    .disease_idx(raw_target)
                    .map(|d| b.atlas.disease_at(d).id.clone()),
            };
            let Some(target) = target else {
                continue;
            };
            pages(b, data, &r["prov:wasDerivedFrom"], &mut used)?;
            b.edge(
                NewEdge {
                    from,
                    to: &target,
                    relation,
                    kind: EdgeKind::Observed,
                    level: LinkLevel::Curated,
                    reason: "Exact publisher-stated initiative scope".into(),
                    activity: act,
                },
                &[rec],
            );
            linked += 1;
        }
    }
    b.data.provenance.activity_mut(act).used = used;
    coverage.status = "loaded".into();
    coverage.records = env.records.len() as u64;
    coverage.retrieved_at = env.header_str("retrieved_at").map(str::to_owned);
    coverage.header_checksums_verified = checked_headers;
    coverage.nodes = b.data.initiatives.len() as u64;
    coverage.edges = linked as u64;
    coverage.excluded = env.records.iter().filter(|r| r["excluded"] == true).count() as u64;
    b.finish(
        act,
        &[("initiatives", b.data.initiatives.len()), ("exact_links", linked)],
    );
    b.data.coverage.push(coverage);
    Ok(())
}
