//! Shared step of every research-cache reader (D37, D39): open one JSON envelope (schema and
//! version checked by [`cache::read_envelope`]), register it as a `prov:Entity` with its licence and
//! licence class, hash each record (canonical JSON, `records[i]`), add assets and links to atlas
//! conditions and genes, and report one coverage row. Concept modules (`programmes`, `samples`,
//! `outcomes`, `funding`, `claims`) only map fields.

use std::path::Path;

use atlas_core::graph::{
    Asset, Coverage, EntityLicence, LicenceClass, LinkLevel, RecIdx, RecordHash, Relation, SourceRecord,
};
use atlas_core::node::{EdgeKind, NodeKind};
use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, SourceEntity};
use serde_json::Value;

use super::builder::{Builder, NewEdge};
use super::cache::{self, Envelope};
use crate::error::IngestError;

/// Trimmed string field (`""` when absent or not a string).
pub fn s<'v>(v: &'v Value, k: &str) -> &'v str {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim()
}

/// Non-empty string field.
pub fn opt(v: &Value, k: &str) -> Option<String> {
    Some(s(v, k)).filter(|x| !x.is_empty()).map(str::to_owned)
}

/// Graph form of a CURIE: `NCT:NCT01234567` → `NCT01234567` (study node ids are bare NCT ids);
/// other CURIEs unchanged.
pub fn norm_curie(id: &str) -> &str {
    let id = id.trim();
    match id.strip_prefix("NCT:") {
        Some(rest) if rest.starts_with("NCT") => rest,
        _ => id,
    }
}

/// String items of an array field.
pub fn strs<'v>(v: &'v Value, k: &str) -> impl Iterator<Item = &'v str> {
    v.get(k)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|x| !x.is_empty())
}

/// What a cache file is, for its entity and coverage row.
pub struct Spec<'a> {
    /// Coverage source key (`pipelines`, `regulatory`, ...).
    pub source: &'a str,
    pub label: &'a str,
    /// Path relative to the data dir.
    pub file: &'a str,
    pub schema: &'a str,
    pub versions: &'a [u64],
    pub url: &'a str,
    pub licence: &'a str,
    pub class: LicenceClass,
    pub scope: &'a str,
}

/// One opened cache file.
pub struct Source {
    pub entity: EntityIdx,
    pub env: Envelope,
    pub coverage: Coverage,
    pub act: ActivityIdx,
}

/// Register the licence of an entity (D37 §3).
pub fn licence(b: &mut Builder<'_>, entity: EntityIdx, licence: &str, class: LicenceClass) {
    if !b.data.licences.iter().any(|l| l.entity == entity) {
        b.data.licences.push(EntityLicence {
            entity,
            licence: licence.to_owned(),
            class,
        });
    }
}

/// Open a cache file; `None` (and an `absent` coverage row) when it does not exist.
pub fn open(
    b: &mut Builder<'_>,
    data: &Path,
    spec: &Spec<'_>,
    act: ActivityIdx,
) -> Result<Option<Source>, IngestError> {
    let path = data.join(spec.file);
    let coverage = Coverage {
        source: spec.source.into(),
        label: spec.label.into(),
        status: "absent".into(),
        files: vec![spec.file.into()],
        scope: spec.scope.into(),
        licence_class: Some(spec.class),
        ..Coverage::default()
    };
    if !path.exists() {
        b.data.coverage.push(coverage);
        return Ok(None);
    }
    let env = match cache::read_envelope(&path, spec.schema, spec.versions) {
        Ok(env) => env,
        Err(IngestError::Schema { found, .. }) => {
            b.data.coverage.push(Coverage {
                status: format!("rejected: schema {found}"),
                ..coverage
            });
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let entity = b.entity(SourceEntity {
        id: format!("source:{}", spec.file),
        url: spec.url.into(),
        file: spec.file.into(),
        version: Some(format!("{} v{}", env.schema, env.version)),
        retrieved_at: env
            .header_str("retrieved_at")
            .or_else(|| env.header_str("generated_at"))
            .map(str::to_owned),
        sha256: env.header_str("sha256").map(str::to_owned),
        bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        licence: Some(spec.licence.into()),
    });
    licence(b, entity, spec.licence, spec.class);
    b.data.provenance.activity_mut(act).used.push(entity);
    let coverage = Coverage {
        status: "loaded".into(),
        retrieved_at: b.data.provenance.entity(entity).retrieved_at.clone(),
        records: env.records.len() as u64,
        header_checksums_verified: u64::from(env.header_verified),
        header_checksums_failed: u64::from(!env.header_verified),
        ..coverage
    };
    Ok(Some(Source {
        entity,
        env,
        coverage,
        act,
    }))
}

impl Source {
    /// Source record of `records[i]` (canonical-JSON sha256; id, url, fetched_at from the record).
    pub fn record(&self, b: &mut Builder<'_>, i: usize) -> RecIdx {
        let r = &self.env.records[i];
        let id = opt(r, "id").unwrap_or_else(|| format!("records[{i}]"));
        let rec = b.record(SourceRecord {
            entity: self.entity,
            locator: Locator::Record(format!("records[{i}]")),
            id,
            url: opt(r, "url"),
            fetched_at: opt(r, "fetched_at").or_else(|| opt(r, "retrieved_at")),
            hash: RecordHash::CanonicalJson,
            sha256: cache::canonical_sha256(r),
        });
        if !self.env.header_verified {
            super::quarantine::mark(b, rec, "source envelope checksum failed");
        }
        rec
    }

    /// Push the coverage row with the counts of this source.
    pub fn finish(mut self, b: &mut Builder<'_>, nodes: usize, edges: usize, excluded: usize) {
        self.coverage.nodes = nodes as u64;
        self.coverage.edges = edges as u64;
        self.coverage.excluded = excluded as u64;
        b.data.coverage.push(self.coverage);
    }
}

/// Asset node id for a source id: the id itself, or `asset:<id>` when another node kind already
/// uses it (an outcome resource identified by a PMID is not the paper).
pub fn asset_id(b: &Builder<'_>, id: &str) -> String {
    match b.node(id) {
        Some((kind, _)) if kind != NodeKind::Asset => format!("asset:{id}"),
        _ => id.to_owned(),
    }
}

/// Add an asset, or merge records/facts into the asset with the same id. Returns `true` when new.
/// Callers pass ids from [`asset_id`].
pub fn asset(b: &mut Builder<'_>, a: Asset) -> bool {
    if let Some((NodeKind::Asset, i)) = b.node(&a.id) {
        let have = &mut b.data.assets[i as usize];
        for r in a.records {
            if !have.records.contains(&r) {
                have.records.push(r);
            }
        }
        for f in a.facts {
            if !have.facts.contains(&f) {
                have.facts.push(f);
            }
        }
        return false;
    }
    if b.node(&a.id).is_some() {
        return false;
    }
    b.register(&a.id, NodeKind::Asset, b.data.assets.len());
    b.data.assets.push(a);
    true
}

/// Atlas gene id of a symbol or HGNC/NCBIGene id.
pub fn gene(b: &Builder<'_>, key: &str) -> Option<String> {
    b.gene_id(key.trim())
}

/// Canonical atlas id of a **rare** condition id (any source id the identity layer knows).
pub fn rare_condition(b: &Builder<'_>, id: &str) -> Option<String> {
    b.atlas.disease(id.trim()).filter(|d| d.rare).map(|d| d.id.clone())
}

/// How the edges of one record are made: activity, record, edge kind and link level.
#[derive(Clone, Copy, Debug)]
pub struct Via {
    pub act: ActivityIdx,
    pub rec: RecIdx,
    pub kind: EdgeKind,
    pub level: LinkLevel,
}

impl Via {
    /// Add (or merge into) the edge `from -relation-> to`; returns 1 for counting.
    pub fn link(self, b: &mut Builder<'_>, from: &str, relation: Relation, to: &str, reason: String) -> usize {
        b.edge(
            NewEdge {
                from,
                relation,
                to,
                kind: self.kind,
                level: self.level,
                reason,
                activity: self.act,
            },
            &[self.rec],
        );
        1
    }
}

/// `held_by` to the organisation `org_id`, creating a sponsor-like organisation node from the
/// source's own name when none exists (`org:<slug>`, the scheme CT.gov sponsors use).
pub fn held_by(b: &mut Builder<'_>, asset_id: &str, org_id: Option<&str>, name: &str, via: Via) -> Option<String> {
    let rec = via.rec;
    let id = match org_id.filter(|o| b.node(o).is_some()) {
        Some(o) => o.to_owned(),
        None => org(b, name, atlas_core::graph::OrgKind::Sponsor, rec)?,
    };
    Via {
        kind: EdgeKind::Observed,
        level: LinkLevel::Curated,
        ..via
    }
    .link(b, asset_id, Relation::HeldBy, &id, format!("holder \"{name}\""));
    Some(id)
}

/// Organisation node `org:<slug>` for a source-given name (the scheme CT.gov sponsors use),
/// created with `kind` when new; the record is added either way. `None` for an empty name or an
/// id taken by another node kind.
pub fn org(b: &mut Builder<'_>, name: &str, kind: atlas_core::graph::OrgKind, rec: RecIdx) -> Option<String> {
    if name.trim().is_empty() {
        return None;
    }
    let id = Builder::org_id(name);
    match b.node(&id) {
        Some((NodeKind::Organisation, i)) => {
            let o = &mut b.data.orgs[i as usize];
            if !o.records.contains(&rec) {
                o.records.push(rec);
            }
        }
        Some(_) => return None,
        None => {
            b.register(&id, NodeKind::Organisation, b.data.orgs.len());
            b.data.orgs.push(atlas_core::graph::Organisation {
                id: id.clone(),
                name: name.trim().to_owned(),
                kind,
                url: None,
                contact_url: None,
                country: None,
                country_basis: None,
                description: None,
                languages: Vec::new(),
                verified_on: None,
                channels: Vec::new(),
                records: vec![rec],
            });
        }
    }
    Some(id)
}
