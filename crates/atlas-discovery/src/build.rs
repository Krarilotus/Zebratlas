//! Loading a manifest with its data and building the reviewed-alignment bundle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use atlas_core::node::{Edge, EdgeKind, Evidence, NodeKind, NodeRef};
use atlas_core::provenance::{Activity, Agent, Provenance, RecordRef, SourceEntity};
use serde_json::Value;

use crate::export::validate;

use crate::model::*;

/// Verify immutable raw bytes BEFORE parsing. Paths must stay inside the supplied cache.
pub fn load(manifest_path: &Path) -> Result<(Manifest, Vec<Value>, String)> {
    let bytes = std::fs::read(manifest_path)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        m.schema == "atlas.discovery.sources" && m.version == 1,
        "unsupported manifest schema"
    );
    let root = manifest_path.parent().context("manifest parent")?.canonicalize()?;
    let mut ids = BTreeSet::new();
    let mut data = Vec::new();
    for s in &m.sources {
        ensure!(ids.insert(s.id.clone()), "duplicate source id");
        ensure!(
            !s.url.is_empty() && !s.version.is_empty() && !s.retrieved_at.is_empty(),
            "incomplete provenance"
        );
        let path = root.join(&s.file).canonicalize()?;
        ensure!(path.starts_with(&root), "source path escapes cache");
        let raw = std::fs::read(path)?;
        ensure!(
            sha256(&raw) == s.sha256 && raw.len() as u64 == s.bytes,
            "source integrity mismatch: {}",
            s.id
        );
        data.push(serde_json::from_slice(&raw).with_context(|| s.id.clone())?);
    }
    Ok((m, data, sha256(&bytes)))
}

fn string(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or_default().into()
}
fn array<'a>(v: &'a Value, pointer: &str) -> Result<&'a Vec<Value>> {
    v.pointer(pointer)
        .and_then(Value::as_array)
        .with_context(|| format!("missing array {pointer}; schema drift"))
}
fn source_index(m: &Manifest, name: &str) -> Result<usize> {
    m.sources
        .iter()
        .position(|s| s.id == name)
        .with_context(|| format!("missing source {name}"))
}
fn record_ref(i: usize, locator: &str) -> RecordRef {
    RecordRef::record(atlas_core::provenance::EntityIdx(i as u16), locator)
}

impl Bundle {
    fn review(&mut self, subject: &str, reason: &str, candidates: Vec<String>, evidence: RecordRef) {
        let key = sha256(format!("{subject}|{reason}|{}|{}", evidence.entity.0, evidence.locator).as_bytes());
        self.review_queue.push(ReviewItem {
            id: format!("urn:atlas:review:{key}"),
            subject_id: subject.into(),
            reason: reason.into(),
            candidates,
            evidence,
            status: "pending".into(),
        });
    }

    fn add_alignment(&mut self, source: usize, alignment: &Alignment) {
        if alignment.decision != Decision::Exact {
            self.review(
                &alignment.mention.id,
                &alignment.reason,
                alignment.targets.clone(),
                alignment.mention.evidence.clone(),
            );
            return;
        }
        let target = self.targets.iter().find(|t| t.id == alignment.targets[0]).unwrap();
        let a = &self.sources.sources[source];
        let b = &self.sources.sources[target.evidence.entity.0 as usize];
        self.mappings.push(Mapping {
            subject_id: alignment.mention.id.clone(),
            subject_label: alignment.mention.label.clone(),
            predicate_id: "skos:exactMatch".into(),
            object_id: target.id.clone(),
            object_label: target.label.clone(),
            mapping_justification: "semapv:BackgroundKnowledgeBasedMatching".into(),
            mapping_provider: "urn:atlas:software:atlas-discovery:0.1.0".into(),
            mapping_date: a.retrieved_at.chars().take(10).collect(),
            subject_source: a.url.clone(),
            object_source: b.url.clone(),
            subject_source_version: a.version.clone(),
            object_source_version: b.version.clone(),
            mapping_evidence: format!("urn:atlas:mapping:{}", sha256(alignment.mention.id.as_bytes())),
        });
    }

    fn add_edge(&mut self, from: &str, relation: &str, to: &str, record: &RecordRef, kind: EdgeKind) {
        let source = &self.sources.sources[record.entity.0 as usize];
        self.edges.push(Edge::new(
            from,
            relation,
            to,
            kind,
            vec![Evidence {
                source: source.id.clone(),
                record: Some(record.locator.to_string()),
                references: vec![source.url.clone(), format!("urn:sha256:{}", source.sha256)],
                evidence_code: Some(POLICY.into()),
                date: Some(source.retrieved_at.clone()),
                ..Default::default()
            }],
        ));
    }
}

/// Adapters deliberately understand only the reviewed schema paths in these two sources.
pub fn build(manifest: Manifest, data: Vec<Value>, manifest_sha256: String) -> Result<Bundle> {
    ensure!(
        manifest.sources.len() == data.len() && data.len() < u16::MAX as usize,
        "source cardinality"
    );
    let mut provenance = Provenance::default();
    for s in &manifest.sources {
        provenance.add_entity(SourceEntity {
            id: format!("urn:sha256:{}", s.sha256),
            url: s.url.clone(),
            file: s.file.clone(),
            version: Some(s.version.clone()),
            retrieved_at: Some(s.retrieved_at.clone()),
            sha256: Some(s.sha256.clone()),
            bytes: s.bytes,
            licence: s.licence.clone(),
        });
    }
    let mut targets = Vec::new();
    for gene in &manifest.seeds {
        let i = source_index(&manifest, &format!("hgnc-{gene}"))?;
        let docs = array(&data[i], "/response/docs")?;
        ensure!(docs.len() == 1, "ambiguous HGNC seed {gene}");
        let r = &docs[0];
        let id = string(r, "/hgnc_id");
        ensure!(
            id.starts_with("HGNC:") && string(r, "/symbol") == *gene,
            "HGNC schema drift"
        );
        ensure!(
            !targets.iter().any(|t: &Target| t.id == id),
            "duplicate canonical target"
        );
        targets.push(Target {
            id,
            label: gene.clone(),
            kind: NodeKind::Gene,
            active: string(r, "/status") == "Approved",
            evidence: record_ref(i, "/response/docs/0"),
        });
    }
    let mut out = Bundle {
        schema: "atlas.discovery.bundle".into(),
        version: 1,
        policy: POLICY.into(),
        manifest_sha256,
        sources: manifest,
        provenance,
        targets,
        characterizations: vec![],
        records: vec![],
        mappings: vec![],
        nodes: vec![],
        edges: vec![],
        review_queue: vec![],
        metrics: BTreeMap::new(),
    };
    for t in &out.targets {
        out.nodes.push(NodeRef {
            id: t.id.clone(),
            label: t.label.clone(),
            kind: t.kind,
        });
    }
    let panel_i = source_index(&out.sources, "panelapp")?;
    let panel = &data[panel_i];
    let panel_id = panel["id"].as_u64().context("panel id schema drift")?;
    ensure!(
        out.sources.discovery["panel_id"].as_u64() == Some(panel_id),
        "discovered panel changed"
    );
    let catalog_i = source_index(&out.sources, "panelapp-catalog")?;
    let query = out.sources.discovery["query"]
        .as_str()
        .context("discovery query required")?;
    let hits: Vec<_> = array(&data[catalog_i], "/results")?
        .iter()
        .filter(|r| {
            r["name"].as_str() == Some(query)
                || r["relevant_disorders"]
                    .as_array()
                    .is_some_and(|xs| xs.iter().any(|x| x.as_str() == Some(query)))
        })
        .collect();
    ensure!(
        hits.len() == 1 && hits[0]["id"].as_u64() == Some(panel_id),
        "catalog selection is ambiguous or unsupported"
    );
    if !data[catalog_i]["next"].is_null() {
        out.review(
            "urn:atlas:discovery:panelapp",
            "catalog pagination incomplete; discovery is bounded to this page",
            vec![],
            record_ref(catalog_i, ""),
        );
    }
    let panel_url = format!("https://panelapp.genomicsengland.co.uk/panels/{panel_id}/");
    out.nodes.push(NodeRef {
        id: panel_url.clone(),
        label: string(panel, "/name"),
        kind: NodeKind::Asset,
    });
    out.review(
        &panel_url,
        "source licence not verified; publication gate remains closed",
        vec![],
        record_ref(panel_i, ""),
    );
    out.characterizations.push(Characterization {
        source: "panelapp".into(),
        schema: "PanelApp API v1 panel".into(),
        fields: BTreeMap::from([
            (
                "/genes/*/gene_data/hgnc_id".into(),
                "gene mention -> HGNC identity".into(),
            ),
            (
                "/genes/*/phenotypes".into(),
                "unparsed original disease/phenotype assertions -> review".into(),
            ),
            (
                "/genes/*/confidence_level".into(),
                "upstream panel assessment; NOT alignment confidence".into(),
            ),
        ]),
        licence: out.sources.sources[panel_i].licence.clone(),
        update_cadence: "unknown; compare version on refresh".into(),
        identifiers: vec!["HGNC".into(), "source panel URL".into()],
        language: "en".into(),
        publication_status: "blocked_pending_licence_review".into(),
        evidence: record_ref(panel_i, ""),
    });

    for field in ["genes", "strs", "regions"] {
        for (n, row) in array(panel, &format!("/{field}"))?.iter().enumerate() {
            let locator = record_ref(panel_i, &format!("/{field}/{n}"));
            let id = format!(
                "{panel_url}#snapshot-{}-{field}-{n}",
                out.sources.sources[panel_i].sha256
            );
            let label = string(row, "/gene_data/gene_symbol");
            ensure!(
                field != "genes" || !label.is_empty(),
                "schema drift: missing panel gene symbol"
            );
            let selected = field == "genes" && out.sources.seeds.contains(&label);
            if !selected {
                out.records.push(Record {
                    id,
                    source: "panelapp".into(),
                    locator,
                    status: "excluded".into(),
                    reason: if field == "genes" {
                        "outside declared ten-gene slice"
                    } else {
                        "unsupported entity kind (retained, not a gene)"
                    }
                    .into(),
                    alignment: None,
                });
                continue;
            }
            let mention = Mention {
                id: format!("{id}-gene-mention"),
                label,
                kind: NodeKind::Gene,
                explicit_ids: row
                    .pointer("/gene_data/hgnc_id")
                    .and_then(Value::as_str)
                    .map(|x| vec![x.into()])
                    .unwrap_or_default(),
                evidence: locator.clone(),
            };
            let aligned = align(mention, &out.targets);
            out.add_alignment(panel_i, &aligned);
            if aligned.decision == Decision::Exact {
                out.add_edge(
                    &panel_url,
                    "has_panel_gene",
                    &aligned.targets[0],
                    &locator,
                    EdgeKind::Observed,
                );
            }
            if !array(row, "/phenotypes")?.is_empty() {
                out.review(
                    &id,
                    "mixed disease and phenotype free text; no gene-to-disease identity inference",
                    vec![],
                    record_ref(panel_i, &format!("/{field}/{n}/phenotypes")),
                );
            }
            out.records.push(Record {
                id,
                source: "panelapp".into(),
                locator,
                status: "staged".into(),
                reason: "publication awaits source licence review".into(),
                alignment: Some(aligned),
            });
        }
    }
    let uni_i = source_index(&out.sources, "uniprot")?;
    out.characterizations.push(Characterization {
        source: "uniprot".into(),
        schema: "UniProtKB REST JSON".into(),
        fields: BTreeMap::from([
            ("/results/*/primaryAccession".into(), "protein research asset".into()),
            (
                "/results/*/uniProtKBCrossReferences[database=HGNC]/id".into(),
                "gene mention -> HGNC identity".into(),
            ),
            (
                "/results/*/genes/*/geneName/value".into(),
                "consistency check; never sufficient for identity".into(),
            ),
        ]),
        licence: out.sources.sources[uni_i].licence.clone(),
        update_cadence: "unknown; compare x-uniprot-release on refresh".into(),
        identifiers: vec!["UniProtKB".into(), "HGNC".into()],
        language: "en".into(),
        publication_status: "staged_with_attribution".into(),
        evidence: record_ref(uni_i, ""),
    });
    if out.sources.sources[uni_i].headers.contains_key("link") {
        out.review(
            &out.sources.sources[uni_i].url.clone(),
            "paginated response; discovery coverage incomplete",
            vec![],
            record_ref(uni_i, ""),
        );
    }
    for (n, row) in array(&data[uni_i], "/results")?.iter().enumerate() {
        let accession = string(row, "/primaryAccession");
        ensure!(!accession.is_empty(), "UniProt schema drift: accession");
        let id = format!("https://www.uniprot.org/uniprotkb/{accession}");
        let locator = record_ref(uni_i, &format!("/results/{n}"));
        let genes = array(row, "/genes")?;
        let label = genes.first().map(|r| string(r, "/geneName/value")).unwrap_or_default();
        let ids = array(row, "/uniProtKBCrossReferences")?
            .iter()
            .filter(|x| x["database"] == "HGNC")
            .map(|x| string(x, "/id"))
            .collect();
        let mention = Mention {
            id: format!("{id}#snapshot-{}-gene-mention", out.sources.sources[uni_i].sha256),
            label,
            kind: NodeKind::Gene,
            explicit_ids: ids,
            evidence: locator.clone(),
        };
        let mut aligned = align(mention, &out.targets);
        if genes.len() != 1 || row.pointer("/organism/taxonId").and_then(Value::as_u64) != Some(9606) {
            aligned.decision = Decision::Review;
            aligned.reason = "not one unambiguous human gene".into();
            aligned.confidence_tier = "unverified".into();
        }
        out.nodes.push(NodeRef {
            id: id.clone(),
            label: string(row, "/proteinDescription/recommendedName/fullName/value"),
            kind: NodeKind::Asset,
        });
        out.add_alignment(uni_i, &aligned);
        if aligned.decision == Decision::Exact {
            out.add_edge(
                &id,
                "protein_product_of",
                &aligned.targets[0],
                &locator,
                EdgeKind::Observed,
            );
        }
        // Functional annotations and disease claims are intentionally not promoted by gene identity.
        out.review(
            &id,
            "protein identity does not validate disease mechanism or asset suitability",
            vec![],
            locator.clone(),
        );
        out.records.push(Record {
            id: format!("{id}#snapshot-{}", out.sources.sources[uni_i].sha256),
            source: "uniprot".into(),
            locator,
            status: "staged".into(),
            reason: "protein asset and explicit gene link only".into(),
            alignment: Some(aligned),
        });
    }
    for source in ["panelapp", "uniprot"] {
        for (suffix, count) in [
            ("records", out.records.iter().filter(|r| r.source == source).count()),
            (
                "excluded",
                out.records
                    .iter()
                    .filter(|r| r.source == source && r.status == "excluded")
                    .count(),
            ),
            (
                "automatic",
                out.records
                    .iter()
                    .filter(|r| {
                        r.source == source && r.alignment.as_ref().is_some_and(|a| a.decision == Decision::Exact)
                    })
                    .count(),
            ),
        ] {
            out.metrics.insert(format!("{source}.{suffix}"), count as u64);
        }
    }
    let activity = Activity {
        id: format!("urn:atlas:activity:{}:{POLICY}", out.manifest_sha256),
        label: "parse; explicit-ID alignment; retain exclusions; stage typed asset edges".into(),
        used: (0..data.len())
            .map(|i| atlas_core::provenance::EntityIdx(i as u16))
            .collect(),
        parameters: BTreeMap::from([
            ("policy".into(), POLICY.into()),
            ("seeds".into(), out.sources.seeds.join(",")),
            ("publication".into(), "staged; no identity mutation".into()),
        ]),
        agent: Agent {
            name: "atlas-discovery".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commit: None,
        },
        counts: out.metrics.clone(),
        ..Default::default()
    };
    out.provenance.add_activity(activity);
    for (step, label) in [
        ("discover", "catalog alias selection and seed-bounded resource search"),
        ("characterize", "versioned schema projection and licence gate"),
        ("parse-filter", "parse records; retain exclusions with reasons"),
        ("stage", "materialize typed asset edges and pending review queue"),
    ] {
        let mut activity = out.provenance.activities[0].clone();
        activity.id = format!("{}:{step}", activity.id);
        activity.label = label.into();
        out.provenance.add_activity(activity);
    }
    validate(&out)?;
    Ok(out)
}
