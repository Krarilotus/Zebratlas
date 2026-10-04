//! Licence-aware projection, not a dump of the graph's structs. No accounts, contributions,
//! contacts, abstracts, quotes, snapshots, definitions or arbitrary cache fields are read.
mod bundle;
// The release projection needs both accepted graph receipts and its RDF writer.
#[allow(clippy::too_many_arguments)]
mod mappings;
pub mod policy;
pub mod quarantine;
mod rdf;
pub mod suppression;
pub mod withhold_index;
use bundle::{checksums, files, write_json, write_support};
use mappings::mappings;
use quarantine::{Quarantine, QuarantineSummary};

use anyhow::{Context, Result, ensure};
use atlas_core::graph::{RecIdx, hex};
use atlas_core::node::{NodeKey, NodeKind};
use atlas_core::provenance::{ActivityIdx, EntityIdx, Locator, Provenance, RecordRef};
use atlas_core::{Atlas, Graph};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citation {
    pub source: String,
    pub source_url: String,
    pub source_url_basis: String,
    pub record_url: Option<String>,
    pub retrieved_at: Option<String>,
    pub retrieval_basis: String,
    pub version: String,
    pub sha256: String,
    pub hash_scope: String,
    pub source_sha256: String,
    pub source_hash_scope: String,
    pub record_locator: String,
    pub source_entity: String,
    pub license: String,
    pub copy_fields: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseNode {
    pub id: String,
    pub kind: String,
    pub export_status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<String>,
    pub provenance: Vec<Citation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseEdge {
    pub id: String,
    pub export_status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<String>,
    pub activities: Vec<String>,
    pub provenance: Vec<Citation>,
}

#[derive(Default, Debug, Serialize)]
pub struct SourceCounts {
    pub node_memberships: usize,
    pub edge_memberships: usize,
    pub asserted_edge_memberships: usize,
    pub link_only_edge_memberships: usize,
    pub jsonl_bytes: u64,
}

#[derive(Default, Debug, Serialize)]
pub struct ReleaseReport {
    pub version: String,
    pub nodes: BTreeMap<String, usize>,
    pub edges: BTreeMap<String, usize>,
    pub asserted_edges: usize,
    pub link_only_edges: usize,
    pub node_link_only: usize,
    pub mapping_rows: usize,
    pub sources: BTreeMap<String, SourceCounts>,
    pub asset_kinds: BTreeMap<String, usize>,
    pub files: BTreeMap<String, u64>,
    pub reconciliation: serde_json::Value,
    /// Items left out by the withholding filter (D43 suppression, quarantine): counts only.
    pub withheld_nodes: usize,
    pub withheld_edges: usize,
    pub excluded_nodes: BTreeMap<String, usize>,
    pub excluded_edges: BTreeMap<String, usize>,
    pub excluded_disease_status: BTreeMap<String, usize>,
    pub exclusion_reasons: BTreeMap<String, usize>,
    pub quarantine: QuarantineSummary,
    pub suppression: suppression::Summary,
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes).into())
}

/// Link allowlist: no credentials, query parameters, fragments or non-HTTP schemes.
/// Query/fragment hashes remain available in source entity metadata, without disclosure.
pub(crate) fn personal_reference(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    [
        "orcid:",
        "orcid%3a",
        "orcid.org/",
        "person:",
        "person%3a",
        "reporter.pi:",
        "reporter.pi%3a",
        "cache/people/",
        "cache%2fpeople%2f",
    ]
    .iter()
    .any(|p| s.contains(p))
}

pub fn public_url(s: &str) -> Option<String> {
    if personal_reference(s) {
        return None;
    }
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?;
    if authority.is_empty() || authority.contains('@') || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(s.split(['?', '#']).next()?.to_owned())
}

struct RecordDetails<'a> {
    url: Option<&'a str>,
    fetched: Option<&'a str>,
    hash: Option<String>,
    hash_scope: &'a str,
}

fn citation(
    prov: &Provenance,
    idx: EntityIdx,
    locator: &Locator,
    details: RecordDetails<'_>,
    namespace: &str,
) -> Citation {
    let RecordDetails {
        url: record_url,
        fetched,
        hash: record_hash,
        hash_scope,
    } = details;
    let e = prov.entity(idx);
    let p = policy::for_entity(e);
    let source_sha256 = e.sha256.clone().unwrap_or_default();
    let upstream = public_url(&e.url);
    let record_link = record_url.and_then(public_url);
    let source_url_basis = if upstream.is_some() {
        "upstream source URL"
    } else if record_link.is_some() {
        "upstream record URL; file source URL unavailable"
    } else {
        "local derived source entity; upstream URL unrecorded"
    };
    let source_url = upstream.or_else(|| record_link.clone()).unwrap_or_else(|| {
        format!(
            "https://w3id.org/rare-disease-atlas/source/{}",
            rdf::enc(&format!("{namespace}:{}", e.id))
        )
    });
    let source_hash_scope = if e.file == "cache/trials/studies.jsonl.gz" {
        "uncompressed JSONL bytes"
    } else if e.file.starts_with("cache/kgx/") || e.file.starts_with("cache/mappings/") {
        "source-file bytes"
    } else if e.file.starts_with("cache/") {
        "canonical JSON records array, as declared in cache header"
    } else {
        "source-file bytes"
    };
    Citation {
        source: p.source.into(),
        source_url,
        source_url_basis: source_url_basis.into(),
        record_url: record_url.and_then(public_url),
        retrieved_at: fetched.map(str::to_owned).or_else(|| e.retrieved_at.clone()),
        retrieval_basis: if fetched.is_some() {
            "record fetched_at"
        } else {
            "source metadata; raw files may use filesystem mtime proxy"
        }
        .into(),
        version: e
            .version
            .clone()
            .unwrap_or_else(|| format!("snapshot-sha256:{source_sha256}")),
        sha256: record_hash.unwrap_or_else(|| source_sha256.clone()),
        hash_scope: hash_scope.into(),
        source_sha256,
        source_hash_scope: source_hash_scope.into(),
        record_locator: locator.to_string(),
        source_entity: format!("{namespace}:{}", e.id),
        license: p.license.into(),
        copy_fields: p.copy_fields,
    }
}

fn atlas_refs(atlas: &Atlas, refs: &[RecordRef]) -> Vec<Citation> {
    refs.iter()
        .map(|r| {
            citation(
                &atlas.provenance,
                r.entity,
                &r.locator,
                RecordDetails {
                    url: None,
                    fetched: None,
                    hash: None,
                    hash_scope: "source-file bytes; exact record located by record_locator",
                },
                "atlas",
            )
        })
        .collect()
}

fn graph_refs(graph: &Graph, refs: &[RecIdx]) -> Vec<Citation> {
    refs.iter()
        .map(|&i| {
            let r = graph.record(i);
            citation(
                graph.provenance(),
                r.entity,
                &r.locator,
                RecordDetails {
                    url: r.url.as_deref(),
                    fetched: r.fetched_at.as_deref(),
                    hash: Some(hex(&r.sha256)),
                    hash_scope: &format!("record:{:?}", r.hash),
                },
                "graph",
            )
        })
        .collect()
}

fn may_copy(refs: &[Citation]) -> bool {
    !refs.is_empty() && refs.iter().all(|r| r.copy_fields)
}

/// Validate the projection immediately before any serialisation. Restricted fields are a hard error.
pub fn validate_node(n: &ReleaseNode) -> Result<()> {
    ensure!(
        n.kind != "person" && !personal_reference(&n.id),
        "personal node in bulk release"
    );
    validate_refs(&n.provenance)?;
    ensure!(
        n.label.is_none() || may_copy(&n.provenance),
        "restricted node label leaked: {}",
        n.id
    );
    ensure!(
        n.export_status
            == if may_copy(&n.provenance) {
                "projected"
            } else {
                "link_only"
            },
        "invalid node policy status"
    );
    Ok(())
}

pub fn validate_edge(e: &ReleaseEdge) -> Result<()> {
    ensure!(!personal_reference(&e.id), "personal edge in bulk release");
    ensure!(
        !personal_reference(&serde_json::to_string(&e.activities)?),
        "personal activity reference in bulk release"
    );
    validate_refs(&e.provenance)?;
    ensure!(!e.activities.is_empty(), "edge without generating activity: {}", e.id);
    if e.export_status == "link_only" {
        ensure!(
            e.from.is_none() && e.to.is_none() && e.relation.is_none() && e.kind.is_none(),
            "restricted edge assertion leaked: {}",
            e.id
        );
    } else {
        ensure!(
            e.export_status == "asserted" && may_copy(&e.provenance),
            "unlicensed asserted edge: {}",
            e.id
        );
        ensure!(
            e.from.is_some() && e.to.is_some() && e.relation.is_some() && e.kind.is_some(),
            "incomplete asserted edge"
        );
    }
    Ok(())
}

fn validate_refs(refs: &[Citation]) -> Result<()> {
    ensure!(!refs.is_empty(), "record without provenance");
    for r in refs {
        ensure!(
            !personal_reference(&serde_json::to_string(r)?),
            "personal provenance in bulk release"
        );
        ensure!(
            !r.source_url.is_empty() && !r.record_locator.is_empty() && !r.version.is_empty(),
            "incomplete source/locator/version: {}",
            r.source_entity
        );
        ensure!(
            r.retrieved_at.is_some(),
            "missing retrieval metadata: {}",
            r.source_entity
        );
        for d in [&r.sha256, &r.source_sha256] {
            ensure!(
                d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()),
                "missing/invalid checksum: {}",
                r.source_entity
            );
        }
    }
    Ok(())
}

fn activity_id(prov: &Provenance, idx: ActivityIdx, namespace: &str) -> String {
    format!("{namespace}:{}", prov.activity(idx).id)
}

fn project_node(id: &str, kind: &str, label: &str, refs: Vec<Citation>, status: Option<String>) -> ReleaseNode {
    let copy = may_copy(&refs);
    ReleaseNode {
        id: id.into(),
        kind: kind.into(),
        export_status: if copy { "projected" } else { "link_only" }.into(),
        label: (copy && !label.is_empty()).then(|| label.to_owned()),
        status,
        provenance: refs,
    }
}

fn project_edge(
    id: String,
    from: &str,
    relation: &str,
    to: &str,
    kind: &str,
    activities: Vec<String>,
    refs: Vec<Citation>,
) -> ReleaseEdge {
    let copy = may_copy(&refs);
    ReleaseEdge {
        id,
        export_status: if copy { "asserted" } else { "link_only" }.into(),
        from: copy.then(|| from.into()),
        relation: copy.then(|| relation.into()),
        to: copy.then(|| to.into()),
        kind: copy.then(|| kind.into()),
        activities,
        provenance: refs,
    }
}

struct Writer<'a> {
    nodes: BufWriter<File>,
    edges: BufWriter<File>,
    turtle: rdf::Writer,
    node_ids: BTreeSet<String>,
    edge_ids: BTreeSet<String>,
    endpoint_edges: BTreeMap<String, Vec<String>>,
    report: ReleaseReport,
    quarantine: Quarantine,
    suppression: suppression::Summary,
    withhold: &'a atlas_core::withhold::Withhold,
    source_files: BTreeMap<String, String>,
    person_ids: BTreeSet<String>,
    considered_nodes: BTreeSet<String>,
    used_sources: BTreeSet<String>,
    used_activities: BTreeSet<String>,
}

impl Writer<'_> {
    fn node(&mut self, n: ReleaseNode) -> Result<()> {
        if !self.considered_nodes.insert(n.id.clone()) {
            ensure!(!self.node_ids.contains(&n.id), "duplicate node id: {}", n.id);
            return Ok(());
        }
        let reason = if n.kind == "person"
            || self.person_ids.contains(&n.id)
            || personal_reference(&n.id)
            || n.provenance.iter().any(|r| personal_reference(&r.source_entity))
        {
            Some("bulk_person_exclusion")
        } else if n.provenance.iter().any(|r| r.retrieved_at.is_none()) {
            Some("missing_retrieval_metadata")
        } else if suppression::blocked(self.withhold, &n.id) {
            self.suppression.excluded_nodes += 1;
            Some("suppressed_lineage")
        } else if self.quarantine.blocks(&n.provenance, &self.source_files) {
            self.quarantine.summary.excluded_nodes += 1;
            Some("quarantined_lineage")
        } else {
            None
        };
        if let Some(reason) = reason {
            *self.report.excluded_nodes.entry(n.kind.clone()).or_default() += 1;
            if let Some(status) = n.status {
                *self.report.excluded_disease_status.entry(status).or_default() += 1;
            }
            *self.report.exclusion_reasons.entry(reason.into()).or_default() += 1;
            return Ok(());
        }
        self.used_sources
            .extend(n.provenance.iter().map(|r| r.source_entity.clone()));
        validate_node(&n)?;
        ensure!(self.node_ids.insert(n.id.clone()), "duplicate node id: {}", n.id);
        *self.report.nodes.entry(n.kind.clone()).or_default() += 1;
        self.report.node_link_only += usize::from(n.export_status == "link_only");
        let bytes = serde_json::to_vec(&n)?;
        for source in n.provenance.iter().map(|r| &r.source).collect::<BTreeSet<_>>() {
            let c = self.report.sources.entry(source.clone()).or_default();
            c.node_memberships += 1;
            c.jsonl_bytes += bytes.len() as u64 + 1;
        }
        self.nodes.write_all(&bytes)?;
        self.nodes.write_all(b"\n")?;
        rdf::node(&mut self.turtle, &n)?;
        Ok(())
    }
    fn edge(&mut self, e: ReleaseEdge, original_relation: &str, api_scope: bool) -> Result<()> {
        let (from, _, to) = atlas_core::node::parse_edge_id(&e.id).context("invalid edge identifier")?;
        let key = if api_scope {
            original_relation.to_owned()
        } else {
            format!("{original_relation} (supplemental)")
        };
        let reason = if self.person_ids.contains(from)
            || self.person_ids.contains(to)
            || personal_reference(&e.id)
            || e.provenance.iter().any(|r| personal_reference(&r.source_entity))
        {
            Some("bulk_person_exclusion")
        } else if e.provenance.iter().any(|r| r.retrieved_at.is_none()) {
            Some("missing_retrieval_metadata")
        } else if suppression::blocked(self.withhold, from) || suppression::blocked(self.withhold, to) {
            self.suppression.excluded_edges += 1;
            Some("suppressed_lineage")
        } else if self.quarantine.blocks(&e.provenance, &self.source_files) {
            self.quarantine.summary.excluded_edges += 1;
            Some("quarantined_lineage")
        } else if !self.node_ids.contains(from) || !self.node_ids.contains(to) {
            Some("excluded_endpoint")
        } else {
            None
        };
        if let Some(reason) = reason {
            *self.report.excluded_edges.entry(key).or_default() += 1;
            *self.report.exclusion_reasons.entry(reason.into()).or_default() += 1;
            return Ok(());
        }
        self.used_sources
            .extend(e.provenance.iter().map(|r| r.source_entity.clone()));
        self.used_activities.extend(e.activities.iter().cloned());
        validate_edge(&e)?;
        ensure!(self.edge_ids.insert(e.id.clone()), "duplicate edge id: {}", e.id);
        self.endpoint_edges.entry(from.into()).or_default().push(e.id.clone());
        if from != to {
            self.endpoint_edges.entry(to.into()).or_default().push(e.id.clone());
        }
        if let (Some(from), Some(to)) = (&e.from, &e.to) {
            ensure!(
                self.node_ids.contains(from) && self.node_ids.contains(to),
                "dangling release edge: {}",
                e.id
            );
        }
        // Supplemental retired/newly-described phenotypes are explicitly separate from /api/integrity.
        let key = if api_scope {
            original_relation.to_owned()
        } else {
            format!("{original_relation} (supplemental)")
        };
        *self.report.edges.entry(key).or_default() += 1;
        let asserted = e.export_status == "asserted";
        self.report.asserted_edges += usize::from(asserted);
        self.report.link_only_edges += usize::from(!asserted);
        let bytes = serde_json::to_vec(&e)?;
        for source in e.provenance.iter().map(|r| &r.source).collect::<BTreeSet<_>>() {
            let c = self.report.sources.entry(source.clone()).or_default();
            c.edge_memberships += 1;
            c.asserted_edge_memberships += usize::from(asserted);
            c.link_only_edge_memberships += usize::from(!asserted);
            c.jsonl_bytes += bytes.len() as u64 + 1;
        }
        self.edges.write_all(&bytes)?;
        self.edges.write_all(b"\n")?;
        rdf::edge(&mut self.turtle, &e)?;
        Ok(())
    }
}

/// Export a release. `withhold` is the shared suppression/quarantine filter (D43); a closed
/// filter (unreadable list) stops the export before anything is written.
pub fn export(
    atlas: &Atlas,
    graph: &Graph,
    withhold: &atlas_core::withhold::Withhold,
    target: &Path,
    version: &str,
    quarantine: Quarantine,
) -> Result<ReleaseReport> {
    export_with_summary(
        atlas,
        graph,
        withhold,
        target,
        version,
        quarantine,
        suppression::Summary {
            input_entries: withhold.suppression_entries().len(),
            ..Default::default()
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn export_with_summary(
    atlas: &Atlas,
    graph: &Graph,
    withhold: &atlas_core::withhold::Withhold,
    target: &Path,
    version: &str,
    quarantine: Quarantine,
    suppression: suppression::Summary,
) -> Result<ReleaseReport> {
    ensure!(
        withhold.closed_reason().is_none(),
        "withholding list unavailable; refusing to export (fail-closed)"
    );
    ensure!(
        !target.exists(),
        "refusing to overwrite an existing release: {}",
        target.display()
    );
    let integrity = atlas_core::integrity::check(atlas, graph);
    ensure!(
        integrity.passed,
        "upstream graph integrity failed: {:?}",
        integrity.violations
    );
    fs::create_dir_all(target.join("mappings"))?;
    let mut w = Writer {
        nodes: BufWriter::new(File::create(target.join("nodes.jsonl"))?),
        edges: BufWriter::new(File::create(target.join("edges.jsonl"))?),
        turtle: rdf::Writer::new(File::create(target.join("graph.ttl"))?),
        node_ids: BTreeSet::new(),
        edge_ids: BTreeSet::new(),
        endpoint_edges: BTreeMap::new(),
        quarantine,
        suppression,
        withhold,
        source_files: quarantine::source_files(&atlas.provenance, graph.provenance()),
        person_ids: (0..graph.node_count(NodeKind::Person) as u32)
            .map(|idx| graph.person(idx).id.clone())
            .collect(),
        considered_nodes: BTreeSet::new(),
        used_sources: BTreeSet::new(),
        used_activities: BTreeSet::new(),
        report: ReleaseReport {
            version: version.into(),
            ..Default::default()
        },
    };
    rdf::header(&mut w.turtle)?;
    let hp = atlas
        .provenance
        .entity_by_file("hp.obo")
        .context("missing hp.obo provenance")?;
    for t in atlas.hpo.terms() {
        w.node(project_node(
            &t.id,
            "phenotype",
            &t.name,
            atlas_refs(atlas, &[RecordRef::record(hp, &t.id)]),
            None,
        ))?;
    }
    for d in atlas.diseases() {
        let status = if !d.is_active() {
            "retired"
        } else if d.is_newly_described() {
            "newly_described"
        } else {
            "active"
        };
        w.node(project_node(
            &d.id,
            "disease",
            &d.name,
            atlas_refs(atlas, &d.derived_from),
            Some(status.into()),
        ))?;
    }
    // Gene node symbols have independent HGNC provenance when available. Never infer label permissions
    // from an unrelated disease source. Gene links carry their separate, original upstream lineage.
    for g in atlas.genes() {
        let refs = graph
            .gene_alias(&g.symbol)
            .map(|a| graph_refs(graph, &[a.record]))
            .unwrap_or_else(|| {
                let refs: Vec<_> = g
                    .diseases
                    .iter()
                    .flat_map(|&d| atlas.disease_at(d).genes.iter())
                    .filter(|l| l.symbol == g.symbol)
                    .map(|l| l.record.clone())
                    .collect();
                atlas_refs(atlas, &refs)
            });
        w.node(project_node(g.id(), "gene", &g.symbol, refs, None))?;
    }
    for kind in [
        NodeKind::Study,
        NodeKind::Grant,
        NodeKind::Paper,
        NodeKind::Person,
        NodeKind::Organisation,
        NodeKind::Asset,
    ] {
        for idx in 0..graph.node_count(kind) as u32 {
            let key = NodeKey { kind, idx };
            if kind == NodeKind::Asset && !graph.asset(idx).release {
                *w.report.excluded_nodes.entry("asset".into()).or_default() += 1;
                *w.report
                    .exclusion_reasons
                    .entry("asset_release_disabled".into())
                    .or_default() += 1;
                continue;
            }
            if let Some(hit) = withhold.node(graph, key) {
                w.report.withheld_nodes += 1;
                *w.report.excluded_nodes.entry(kind.as_str().into()).or_default() += 1;
                let reason = match hit.list {
                    atlas_core::withhold::ListKind::Quarantine => {
                        w.quarantine.summary.excluded_nodes += 1;
                        "quarantined_lineage"
                    }
                    atlas_core::withhold::ListKind::Suppression => {
                        w.suppression.excluded_nodes += 1;
                        "suppressed"
                    }
                };
                *w.report.exclusion_reasons.entry(reason.into()).or_default() += 1;
                continue;
            }
            let n = graph.node_ref(key);
            // Person nodes are counted as exclusions and never serialised.
            let label = if kind == NodeKind::Person { "" } else { &n.label };
            w.node(project_node(
                &n.id,
                kind.as_str(),
                label,
                graph_refs(graph, graph.node_records(key)),
                None,
            ))?;
            if kind == NodeKind::Asset && w.node_ids.contains(&n.id) {
                *w.report
                    .asset_kinds
                    .entry(graph.asset(idx).kind.as_str().into())
                    .or_default() += 1;
            }
        }
    }
    for d in atlas.diseases() {
        // Parents are the direct MONDO is_a assertions retained by atlas-ingest.
        // Cite MONDO alone: unrelated restricted annotations must not hide this licensed assertion.
        let refs = atlas_refs(atlas, &d.derived_from)
            .into_iter()
            .filter(|r| r.source == "mondo")
            .collect::<Vec<_>>();
        for parent in &d.parents {
            if refs.is_empty() {
                continue; // No upstream hierarchy evidence: never manufacture it.
            }
            w.edge(
                project_edge(
                    atlas_core::node::edge_id(&d.id, "subclass_of", parent),
                    &d.id,
                    "subclass_of",
                    parent,
                    "observed",
                    vec![format!("atlas:{}", atlas.provenance.activity(d.generated_by).id)],
                    refs.clone(),
                ),
                "subclass_of",
                false,
            )?;
        }
        for absent in [false, true] {
            let relation = atlas_journeys::genes::phenotype_relation(absent);
            for e in if absent { &d.excluded } else { &d.phenotypes } {
                let to = &atlas.hpo.term(e.term).id;
                let refs = e.annotations.iter().map(|a| a.record.clone()).collect::<Vec<_>>();
                let activities = refs
                    .iter()
                    .filter_map(|r| atlas.provenance.generator_of(r.entity))
                    .map(|a| format!("atlas:{}", a.id))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                w.edge(
                    project_edge(
                        atlas_core::node::edge_id(&d.id, relation, to),
                        &d.id,
                        relation,
                        to,
                        "observed",
                        activities,
                        atlas_refs(atlas, &refs),
                    ),
                    &format!("{relation} (atlas)"),
                    d.is_active() && !d.is_newly_described(),
                )?;
            }
        }
        for e in atlas_journeys::genes::gene_edges(atlas, &d.id, &d.genes) {
            // A retired disease can refer to a gene not present in the active gene table.
            if !w.node_ids.contains(&e.gene_id) {
                w.node(project_node(
                    &e.gene_id,
                    "gene",
                    e.symbol,
                    atlas_refs(atlas, &e.links.iter().map(|l| l.record.clone()).collect::<Vec<_>>()),
                    None,
                ))?;
            }
            let refs = e.links.iter().map(|l| l.record.clone()).collect::<Vec<_>>();
            let activities = refs
                .iter()
                .filter_map(|r| atlas.provenance.generator_of(r.entity))
                .map(|a| format!("atlas:{}", a.id))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            w.edge(
                project_edge(
                    e.edge_id,
                    &d.id,
                    atlas_journeys::genes::GENE_RELATION,
                    &e.gene_id,
                    "observed",
                    activities,
                    atlas_refs(atlas, &refs),
                ),
                atlas_journeys::genes::GENE_RELATION,
                true,
            )?;
        }
    }
    for (i, e) in graph.edges().iter().enumerate() {
        if let Some(hit) = withhold.edge(graph, i as u32) {
            w.report.withheld_edges += 1;
            *w.report.excluded_edges.entry(e.relation.as_str().into()).or_default() += 1;
            let reason = match hit.list {
                atlas_core::withhold::ListKind::Quarantine => {
                    w.quarantine.summary.excluded_edges += 1;
                    "quarantined_lineage"
                }
                atlas_core::withhold::ListKind::Suppression => {
                    w.suppression.excluded_edges += 1;
                    "suppressed"
                }
            };
            *w.report.exclusion_reasons.entry(reason.into()).or_default() += 1;
            continue;
        }
        w.edge(
            project_edge(
                e.id(),
                &e.from,
                e.relation.as_str(),
                &e.to,
                e.kind.as_str(),
                vec![activity_id(graph.provenance(), e.activity, "graph")],
                graph_refs(graph, &e.records),
            ),
            e.relation.as_str(),
            true,
        )?;
    }
    // Identity provenance is identifier/evidence metadata, with the same release and withholding gates.
    let mut merge_writer = BufWriter::new(File::create(target.join("identity-merges.jsonl"))?);
    let mut dependencies = BufWriter::new(File::create(target.join("identity-statement-dependencies.jsonl"))?);
    for merge in &graph.data().identity_merges {
        if !w.node_ids.contains(&merge.canonical)
            || merge.mappings.iter().any(|m| {
                m.rule_id.starts_with("R-PER-")
                    || personal_reference(&m.subject)
                    || personal_reference(&m.object)
                    || w.person_ids.contains(&m.subject)
                    || w.person_ids.contains(&m.object)
            })
            || merge.members.iter().any(|m| personal_reference(&m.id))
        {
            continue;
        }
        let refs = graph_refs(graph, &merge.mappings.iter().map(|m| m.record).collect::<Vec<_>>());
        if w.quarantine.blocks(&refs, &w.source_files)
            || merge
                .members
                .iter()
                .any(|m| withhold.records(graph.data(), &m.derived_from).is_some())
        {
            continue;
        }
        let mut public = merge.clone();
        for mapping in &mut public.mappings {
            mapping.evidence_url = public_url(&mapping.evidence_url).unwrap_or_default();
        }
        // Activity parameters are reconstructed from safe structured fields, not arbitrary upstream text.
        public.activity.parameters.clear();
        public.activity.parameters.insert(
            "rules".into(),
            serde_json::to_string(
                &public
                    .mappings
                    .iter()
                    .map(|m| format!("{}@{}", m.rule_id, m.rule_version))
                    .collect::<BTreeSet<_>>(),
            )?,
        );
        let mut value = serde_json::to_value(&public)?;
        value["@type"] = serde_json::json!("prov:Entity");
        value["prov:wasGeneratedBy"] = serde_json::json!(public.activity.id);
        value["members"] = serde_json::json!(
            public
                .members
                .iter()
                .map(|m| serde_json::json!({
                    "@id": m.id, "prov:wasDerivedFrom": graph_refs(graph, &m.derived_from)
                }))
                .collect::<Vec<_>>()
        );
        value["mapping_rows"] = serde_json::json!(
            public
                .mappings
                .iter()
                .map(|m| {
                    let record = graph.record(m.record);
                    serde_json::json!({"mapping_set_id": m.mapping_set_id, "rule_id": m.rule_id,
                "rule_version": m.rule_version, "evidence_url": m.evidence_url,
                "evidence_sha256": m.evidence_sha256, "evidence_locator": m.evidence_locator,
                "row_sha256": atlas_core::graph::hex(&record.sha256), "row_locator": record.locator.to_string()})
                })
                .collect::<Vec<_>>()
        );
        serde_json::to_writer(&mut merge_writer, &value)?;
        writeln!(merge_writer)?;
        for r in &refs {
            w.used_sources.insert(r.source_entity.clone());
        }
        rdf::identity_merge(&mut w.turtle, &public, graph)?;
        let dependent_edges: BTreeSet<_> = std::iter::once(&public.canonical)
            .chain(public.members.iter().map(|member| &member.id))
            .flat_map(|endpoint| w.endpoint_edges.get(endpoint).into_iter().flatten())
            .collect();
        for edge in dependent_edges {
            rdf::identity_statement(&mut w.turtle, edge, &public)?;
            serde_json::to_writer(
                &mut dependencies,
                &serde_json::json!({
                    "statement_id":edge,"canonical_endpoint":public.canonical,
                    "dependency_scope":"all accepted decisions in endpoint component; rebuild on revocation",
                    "prov:hadActivity":public.activity.id,
                    "prov:wasDerivedFrom":public.mappings.iter().filter(|m| !m.decision_id.is_empty()).map(|m| &m.decision_id).collect::<Vec<_>>()
                }),
            )?;
            writeln!(dependencies)?;
        }
    }
    merge_writer.flush()?;
    dependencies.flush()?;
    std::fs::write(
        target.join("mappings/rules.json"),
        atlas_core::identity_rules::REGISTRY_JSON,
    )?;
    let (mapping_rows, excluded_mapping_rows) = mappings(
        atlas,
        graph,
        target,
        version,
        &w.quarantine,
        withhold,
        &w.source_files,
        &w.node_ids,
        &mut w.turtle,
    )?;
    w.report.mapping_rows = mapping_rows;
    w.quarantine.summary.excluded_mapping_rows = excluded_mapping_rows;
    rdf::provenance(
        &mut w.turtle,
        &atlas.provenance,
        "atlas",
        &w.used_sources,
        &w.used_activities,
    )?;
    rdf::provenance(
        &mut w.turtle,
        graph.provenance(),
        "graph",
        &w.used_sources,
        &w.used_activities,
    )?;
    rdf::release_activity(
        &mut w.turtle,
        &atlas.provenance,
        graph.provenance(),
        version,
        &w.used_sources,
    )?;
    w.nodes.flush()?;
    w.edges.flush()?;
    w.turtle.flush()?;
    w.report.quarantine = w.quarantine.summary;
    w.report.suppression = w.suppression;
    let actual = &w.report.nodes;
    for kind in [
        "phenotype",
        "study",
        "grant",
        "paper",
        "person",
        "organisation",
        "asset",
    ] {
        ensure!(
            actual.get(kind).copied().unwrap_or(0)
                == integrity
                    .nodes
                    .get(kind)
                    .copied()
                    .unwrap_or(0)
                    .checked_sub(w.report.excluded_nodes.get(kind).copied().unwrap_or(0))
                    .context("excluded node count exceeds upstream count")?,
            "node count mismatch for {kind}"
        );
    }
    ensure!(
        actual.get("disease").copied().unwrap_or(0) + w.report.excluded_nodes.get("disease").copied().unwrap_or(0)
            == integrity.nodes["disease"]
                + integrity.nodes["disease_retired"]
                + integrity.nodes["disease_newly_described"],
        "disease counts do not reconcile"
    );
    for (relation, expected) in &integrity.edges_by_relation {
        ensure!(
            w.report.edges.get(*relation).copied().unwrap_or(0)
                == expected
                    .checked_sub(w.report.excluded_edges.get(*relation).copied().unwrap_or(0))
                    .context("excluded edge count exceeds upstream count")?,
            "edge count mismatch for {relation}"
        );
    }
    w.report.reconciliation = serde_json::json!({
        "passed": true, "basis": "same atlas_core::integrity::check used by /api/integrity",
        "api_nodes": integrity.nodes, "api_edges_by_relation": integrity.edges_by_relation,
        "disease_rule": "release disease = active + retired + newly_described",
        "gene_supplement": actual.get("gene").copied().unwrap_or(0).saturating_sub(integrity.nodes["gene"]),
        "gene_rule": "API excludes genes found only in newly-described/retired diseases; release retains all",
        "edge_rule": "exported + explicitly excluded = API counts; persons, their edges, suppression and quarantined lineages excluded; supplemental scopes separately reported"
    });
    write_json(
        target.join("integrity.json"),
        &serde_json::json!({
            "passed": integrity.passed, "contracts": integrity.contracts,
            "nodes": integrity.nodes, "edges_by_relation": integrity.edges_by_relation,
            "edges_by_kind": integrity.edges_by_kind, "edges_by_level": integrity.edges_by_level,
            "identity_conflict_counts": { "disease": integrity.identity_conflicts.disease_total, "person": integrity.identity_conflicts.person_total }
        }),
    )?;
    write_support(target, version, atlas, graph, &w.used_sources, &w.report.quarantine)?;
    for file in files(target)? {
        w.report
            .files
            .insert(file.clone(), fs::metadata(target.join(&file))?.len());
    }
    write_json(target.join("release-report.json"), &w.report)?;
    checksums(target)?;
    Ok(w.report)
}
