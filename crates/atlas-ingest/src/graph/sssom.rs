//! SSSOM mapping sets (D30, D37 §2): `data/cache/mappings/*.sssom.tsv`.
//!
//! Only exact rows with accepted, hash-verified gate decisions, named rules and
//! source evidence can merge. Composite and lexical methods remain candidates.
//! Original members, mapping lines, assertion IDs and decisions remain inspectable.
//! The table is constructed before KGX/person occurrences and candidates afterwards.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use atlas_core::graph::{
    Coverage, IdentityMapping, IdentityMember, IdentityMerge, LicenceClass, LinkLevel, RecordHash, Relation,
    SourceRecord, activity,
};
use atlas_core::node::EdgeKind;
use atlas_core::provenance::{Locator, SourceEntity};

use super::builder::Builder;
use super::cache;
use super::research::{self, Via, norm_curie};
use crate::error::IngestError;

pub const DIR: &str = "cache/mappings";

/// Mapping files, sorted (`*.sssom.tsv` only; conflict and precision side files are not mapping sets).
pub fn files(data: &Path) -> Vec<PathBuf> {
    let dir = std::env::var_os("RARE_ATLAS_MAPPING_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join(DIR));
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".sssom.tsv"))
        .collect();
    out.sort();
    out
}

fn is_identity(pred: &str) -> bool {
    matches!(pred, "skos:exactMatch" | "owl:sameAs" | "owl:equivalentClass")
}

/// Source-asserted methods only. Structural acceptance additionally requires gate receipts.
fn merges(pred: &str, justification: &str, conflict: &str) -> bool {
    is_identity(pred)
        && conflict.trim().is_empty()
        && matches!(
            atlas_core::identity_policy::source_method(justification),
            "semapv:BackgroundKnowledgeBasedMatching" | "semapv:ManualMappingCuration"
        )
}

/// One parsed row (borrowed from the line).
struct Row<'l> {
    line: u32,
    bytes: &'l [u8],
    subject: &'l str,
    predicate: &'l str,
    object: &'l str,
    justification: &'l str,
    confidence: &'l str,
    conflict: &'l str,
    licence: &'l str,
    class: &'l str,
    rule_id: &'l str,
    rule_version: &'l str,
    evidence_url: &'l str,
    evidence_sha256: &'l str,
    evidence_locator: &'l str,
    mapping_tool: &'l str,
    assertion_id: &'l str,
    decision_id: &'l str,
}

/// Stream the rows of one SSSOM TSV (metadata `# ` lines skipped; columns by header name).
fn rows(path: &Path, mut f: impl FnMut(Row<'_>, &[(String, String)])) -> Result<(), IngestError> {
    let file = File::open(path).map_err(IngestError::io(path))?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut cols: HashMap<String, usize> = HashMap::new();
    for (i, line) in reader.split(b'\n').enumerate() {
        let mut bytes = line.map_err(IngestError::io(path))?;
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(m) = text.strip_prefix('#') {
            if let Some((k, v)) = m.trim().split_once(':') {
                meta.push((k.trim().to_owned(), v.trim().to_owned()));
            }
            continue;
        }
        if cols.is_empty() {
            cols = text
                .split('\t')
                .enumerate()
                .map(|(i, c)| (c.trim().to_owned(), i))
                .collect();
            continue;
        }
        let fields: Vec<&str> = text.split('\t').collect();
        let get = |name: &str| cols.get(name).and_then(|&c| fields.get(c)).map_or("", |v| v.trim());
        f(
            Row {
                line: (i + 1) as u32,
                bytes: &bytes,
                subject: get("subject_id"),
                predicate: get("predicate_id"),
                object: get("object_id"),
                justification: get("mapping_justification"),
                confidence: get("confidence"),
                conflict: get("conflict"),
                licence: get("license"),
                class: get("license_class"),
                rule_id: get("rule_id"),
                rule_version: get("rule_version"),
                evidence_url: if get("evidence_url").is_empty() {
                    get("prov_source_url")
                } else {
                    get("evidence_url")
                },
                evidence_sha256: if get("evidence_sha256").is_empty() {
                    get("prov_sha256")
                } else {
                    get("evidence_sha256")
                },
                evidence_locator: if get("evidence_locator").is_empty() {
                    get("prov_locator")
                } else {
                    get("evidence_locator")
                },
                mapping_tool: get("mapping_tool"),
                assertion_id: get("identity_assertion_id"),
                decision_id: get("identity_decision_id"),
            },
            &meta,
        );
    }
    Ok(())
}

/// Exact-match identity table built from the mapping sets.
#[derive(Default)]
pub struct Identity {
    map: HashMap<String, String>,
    conflicted: HashSet<String>,
    pub exact_rows: usize,
    projection: crate::identity_projection::Projection,
}

impl Identity {
    /// Canonical id (follows up to 4 alias hops; the id itself when unmapped).
    pub fn canonical<'a>(&'a self, id: &'a str) -> &'a str {
        let mut cur = id;
        for _ in 0..self.map.len() {
            match self.map.get(cur) {
                Some(next) if next != cur => cur = next,
                _ => break,
            }
        }
        cur
    }

    pub fn conflicts(&self) -> usize {
        self.conflicted.len()
    }

    fn add(&mut self, alias: &str, canonical: &str) {
        if alias == canonical || self.conflicted.contains(alias) {
            return;
        }
        match self.map.get(alias) {
            Some(have) if have != canonical => {
                self.map.remove(alias);
                self.conflicted.insert(alias.to_owned());
            }
            Some(_) => {}
            None => {
                self.map.insert(alias.to_owned(), canonical.to_owned());
            }
        }
    }
}

/// Atlas or graph node id that `id` resolves to without the identity table.
pub fn known(b: &Builder<'_>, id: &str) -> Option<String> {
    if let Some(d) = b.atlas.disease(id) {
        return Some(d.id.clone());
    }
    if let Some(g) = b.gene_id(id) {
        return Some(g);
    }
    if let Some(t) = b.atlas.hpo.canonical(id) {
        return Some(b.atlas.hpo.term(t).id.clone());
    }
    b.node(id).map(|_| id.to_owned())
}

/// Pass 1: the identity table (exact merges only).
pub fn identity(b: &Builder<'_>, data: &Path) -> Result<Identity, IngestError> {
    let mut id = Identity {
        projection: crate::identity_projection::load(data)?,
        ..Identity::default()
    };
    for path in files(data) {
        let mut invalid = None;
        rows(&path, |r, _| {
            let (subject, object) = (norm_curie(r.subject), norm_curie(r.object));
            if !merges(r.predicate, r.justification, r.conflict) || subject.is_empty() || object.is_empty() {
                return;
            }
            if !id
                .projection
                .permits(r.decision_id, r.assertion_id, r.subject, r.object)
            {
                invalid = Some(format!("line {}: missing accepted decision receipt", r.line));
                return;
            }
            if let Err(reason) = validate_merge_row(&r) {
                invalid = Some(format!("line {}: {reason}", r.line));
                return;
            }
            id.exact_rows += 1;
            let (s_known, o_known) = (known(b, subject), known(b, object));
            if subject.starts_with("ORCID:") || object.starts_with("ORCID:") {
                let (canonical, alias) = if subject.starts_with("ORCID:") {
                    (subject, object)
                } else {
                    (object, subject)
                };
                id.add(alias, canonical);
                return;
            }
            match (s_known, o_known) {
                (None, Some(o)) => id.add(subject, &o),
                (Some(s), _) => {
                    id.add(object, &s);
                    if s != subject {
                        id.add(subject, &s);
                    }
                }
                (None, None) => id.add(object, subject),
            }
        })?;
        if let Some(reason) = invalid {
            return Err(IngestError::Schema {
                path,
                found: reason,
                expected: "versioned rule and hash-bound evidence for every exact merge".into(),
            });
        }
    }
    Ok(id)
}

fn validate_merge_row(r: &Row<'_>) -> Result<(), String> {
    atlas_core::identity_policy::eligibility(
        r.subject,
        r.object,
        atlas_core::identity_policy::source_method(r.justification),
        r.conflict,
        r.rule_id,
    )
    .map_err(str::to_owned)?;
    let rule = atlas_core::identity_rules::rule(r.rule_id, r.rule_version)
        .ok_or_else(|| "missing or unknown rule/version".to_owned())?;
    if !rule.predicates.iter().any(|p| p == r.predicate) {
        return Err("rule does not permit this predicate".into());
    }
    if r.evidence_url.is_empty()
        || r.evidence_locator.is_empty()
        || r.evidence_sha256.len() != 64
        || !r.evidence_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("missing evidence URL, locator or SHA256".into());
    }
    Ok(())
}

/// Pass 2 (after KGX): persist aliases that reach a node, and add `candidate_same_as` edges.
pub fn finish(b: &mut Builder<'_>, data: &Path, ident: &Identity) -> Result<(), IngestError> {
    let act = b.start(
        activity::IDENTITY_SSSOM,
        "SSSOM identity: exact database cross-references merge, everything else stays a candidate",
        &[],
    );
    let mut aliases: Vec<(String, String)> = ident
        .map
        .keys()
        .filter_map(|a| {
            let c = ident.canonical(a);
            (c != a.as_str() && known(b, c).is_some()).then(|| (a.clone(), c.to_owned()))
        })
        .collect();
    aliases.sort();
    let merged = aliases.len();
    b.data.aliases = aliases;
    let mut clusters: BTreeMap<String, IdentityMerge> = BTreeMap::new();
    for (alias, canonical) in &b.data.aliases {
        let cluster = clusters.entry(canonical.clone()).or_insert_with(|| {
            let mut activity = b.data.provenance.activity(act).clone();
            activity.id = format!(
                "activity:identity-merge:{}",
                cache::hex(&cache::sha256(canonical.as_bytes()))
            );
            activity.label = "identity merge".into();
            activity.used.clear();
            IdentityMerge {
                canonical: canonical.clone(),
                activity,
                members: vec![IdentityMember {
                    id: canonical.clone(),
                    derived_from: vec![],
                }],
                mappings: vec![],
            }
        });
        cluster.members.push(IdentityMember {
            id: alias.clone(),
            derived_from: vec![],
        });
    }
    // Keep original source record identity even when KGX resolved it to a canonical node.
    for (i, record) in b.data.records.iter().enumerate() {
        let id = norm_curie(&record.id);
        if let Some(cluster) = clusters.get_mut(ident.canonical(id))
            && let Some(member) = cluster.members.iter_mut().find(|m| m.id == id)
        {
            member.derived_from.push(i as u32);
        }
    }
    b.param(
        act,
        "identity_gate_manifest_sha256",
        &ident.projection.accepted.manifest_sha256,
    );
    b.param(act, "identity_policy", atlas_core::identity_policy::RULE);
    b.param(act, "merge_rule", "skos:exactMatch|owl:sameAs|owl:equivalentClass + semapv:DatabaseCrossReference|ManualMappingCuration, no conflict flag");
    b.param(
        act,
        "identity_boundary",
        "source-asserted-v2: composite organisation matches remain candidates",
    );
    for path in files(data) {
        let rel = path
            .strip_prefix(data)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let meta_licence = std::cell::RefCell::new(None::<String>);
        let mut pending: Vec<(u32, Vec<u8>, String, String, String, String)> = Vec::new();
        let mut exact: Vec<(String, IdentityMapping, u32, Vec<u8>)> = Vec::new();
        let mut set_meta = Vec::new();
        let (mut read, mut other) = (0usize, 0usize);
        rows(&path, |r, meta| {
            if set_meta.is_empty() {
                set_meta = meta.to_vec();
            }
            read += 1;
            if meta_licence.borrow().is_none() {
                let l = meta.iter().find(|(k, _)| k == "license").map(|(_, v)| v.clone());
                let l = l.or_else(|| (!r.licence.is_empty()).then(|| format!("{}|{}", r.licence, r.class)));
                *meta_licence.borrow_mut() = l;
            }
            let (subject, object) = (norm_curie(r.subject), norm_curie(r.object));
            if ident
                .projection
                .permits(r.decision_id, r.assertion_id, r.subject, r.object)
                && merges(r.predicate, r.justification, r.conflict)
                && ident.canonical(subject) == ident.canonical(object)
                && clusters.contains_key(ident.canonical(subject))
            {
                let metadata = |key: &str| {
                    meta.iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.trim_matches('"').to_owned())
                };
                exact.push((
                    ident.canonical(subject).into(),
                    IdentityMapping {
                        subject: r.subject.into(),
                        object: r.object.into(),
                        mapping_set_id: metadata("mapping_set_id").unwrap_or_else(|| {
                            format!(
                                "https://w3id.org/rare-atlas/mappings/{}",
                                path.file_name()
                                    .unwrap()
                                    .to_string_lossy()
                                    .trim_end_matches(".sssom.tsv")
                            )
                        }),
                        mapping_set_version: metadata("mapping_set_version").unwrap_or_else(|| "unrecorded".into()),
                        record: 0,
                        rule_id: r.rule_id.into(),
                        rule_version: r.rule_version.into(),
                        evidence_url: r.evidence_url.into(),
                        evidence_sha256: r.evidence_sha256.into(),
                        evidence_locator: r.evidence_locator.into(),
                        mapping_tool: r.mapping_tool.into(),
                        assertion_id: r.assertion_id.into(),
                        decision_id: r.decision_id.into(),
                        gate_manifest_sha256: ident.projection.accepted.manifest_sha256.clone(),
                    },
                    r.line,
                    r.bytes.to_vec(),
                ));
            }
            let candidate = (is_identity(r.predicate) || r.predicate == "skos:closeMatch")
                && !(merges(r.predicate, r.justification, r.conflict)
                    && ident.canonical(subject) == ident.canonical(object));
            if !candidate {
                other += 1;
                return;
            }
            let (s, o) = (ident.canonical(subject), ident.canonical(object));
            let (Some(s), Some(o)) = (known(b, s), known(b, o)) else {
                other += 1;
                return;
            };
            if s == o {
                return;
            }
            let reason = format!("{} ({}, {})", r.predicate, r.justification, r.confidence);
            pending.push((r.line, r.bytes.to_vec(), s, o, reason, r.subject.into()));
        })?;
        let (licence, class) = match meta_licence.into_inner() {
            Some(l) => match l.split_once('|') {
                Some((l, c)) => (
                    l.to_owned(),
                    LicenceClass::parse(c).unwrap_or_else(|| LicenceClass::classify(l)),
                ),
                None => (l.clone(), LicenceClass::classify(&l)),
            },
            None => ("not stated in the mapping set".to_owned(), LicenceClass::Unknown),
        };
        let metadata = |key: &str| {
            set_meta
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.trim_matches('"').to_owned())
        };
        let file_hash = file_sha256(&path)?;
        let entity = b.entity(SourceEntity {
            id: format!("source:{rel}"),
            url: metadata("mapping_set_id").unwrap_or_else(|| {
                format!(
                    "https://w3id.org/rare-atlas/mappings/{}",
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .trim_end_matches(".sssom.tsv")
                )
            }),
            file: rel.clone(),
            version: metadata("mapping_set_version").or_else(|| Some(format!("snapshot-sha256:{file_hash}"))),
            retrieved_at: metadata("mapping_date"),
            sha256: Some(file_hash.clone()),
            bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            licence: Some(licence.clone()),
        });
        research::licence(b, entity, &licence, class);
        b.data.provenance.activity_mut(act).used.push(entity);
        for (canonical, mut mapping, line, bytes) in exact {
            if mapping.mapping_set_version == "unrecorded" {
                mapping.mapping_set_version = format!("snapshot-sha256:{file_hash}");
            }
            let rec = b.record(SourceRecord {
                entity,
                locator: Locator::Line(line),
                id: mapping.subject.clone(),
                url: Some(mapping.evidence_url.clone()),
                fetched_at: metadata("mapping_date"),
                hash: RecordHash::TsvLine,
                sha256: cache::sha256(&bytes),
            });
            mapping.record = rec;
            let cluster = clusters.get_mut(&canonical).unwrap();
            for endpoint in [&mapping.subject, &mapping.object] {
                if let Some(member) = cluster.members.iter_mut().find(|m| m.id == norm_curie(endpoint)) {
                    member.derived_from.push(rec);
                }
                if endpoint.as_str() != norm_curie(endpoint) && !cluster.members.iter().any(|m| &m.id == endpoint) {
                    cluster.members.push(IdentityMember {
                        id: endpoint.clone(),
                        derived_from: vec![rec],
                    });
                }
            }
            // The canonical output derives from every mapping input, including implicit atlas aliases.
            cluster.members[0].derived_from.push(rec);
            if !cluster.activity.used.contains(&entity) {
                cluster.activity.used.push(entity);
            }
            cluster.mappings.push(mapping);
        }
        let mut edges = 0;
        for (line, bytes, s, o, reason, source_id) in pending {
            let rec = b.record(SourceRecord {
                entity,
                locator: Locator::Line(line),
                id: source_id,
                url: None,
                fetched_at: None,
                hash: RecordHash::TsvLine,
                sha256: cache::sha256(&bytes),
            });
            let via = Via {
                act,
                rec,
                kind: EdgeKind::Inferred,
                level: LinkLevel::Related,
            };
            edges += via.link(b, &s, Relation::CandidateSameAs, &o, reason);
        }
        b.data.coverage.push(Coverage {
            source: format!("sssom:{}", rel.rsplit('/').next().unwrap_or(&rel)),
            label: "SSSOM mapping set".into(),
            status: "loaded".into(),
            files: vec![rel],
            scope: "identity mappings (exact merges, candidates)".into(),
            records: read as u64,
            edges: edges as u64,
            excluded: other as u64,
            licence_class: Some(class),
            ..Coverage::default()
        });
    }
    b.count(act, "exact_rows", ident.exact_rows);
    b.count(act, "aliases_kept", merged);
    b.count(act, "conflicts_not_merged", ident.conflicts());
    b.finish(act, &[]);
    for (_, mut cluster) in clusters {
        if cluster.mappings.is_empty() || cluster.members.iter().any(|m| m.derived_from.is_empty()) {
            return Err(IngestError::Schema {
                path: data.join(DIR),
                found: format!("unsupported identity cluster {}", cluster.canonical),
                expected: "every merged member derives from a versioned exact mapping row".into(),
            });
        }
        cluster.activity.ended_at = b.data.provenance.activity(act).ended_at.clone();
        cluster.activity.parameters.insert(
            "rule_registry_sha256".into(),
            cache::hex(&cache::sha256(atlas_core::identity_rules::REGISTRY_JSON.as_bytes())),
        );
        cluster
            .activity
            .parameters
            .insert("graph_rules".into(), super::GRAPH_RULES.to_string());
        for member in &mut cluster.members {
            member.derived_from.sort_unstable();
            member.derived_from.dedup();
        }
        cluster.activity.parameters.insert(
            "rules".into(),
            serde_json::to_string(
                &cluster
                    .mappings
                    .iter()
                    .map(|m| format!("{}@{}", m.rule_id, m.rule_version))
                    .collect::<std::collections::BTreeSet<_>>(),
            )
            .unwrap(),
        );
        cluster.activity.parameters.insert(
            "mapping_rows".into(),
            serde_json::to_string(
                &cluster
                    .mappings
                    .iter()
                    .map(|m| {
                        format!(
                            "{}@{}#{}:{}",
                            m.mapping_set_id,
                            m.mapping_set_version,
                            b.data.records[m.record as usize].locator,
                            cache::hex(&b.data.records[m.record as usize].sha256)
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
        cluster.activity.count("members", cluster.members.len() as u64);
        cluster.activity.count("mapping_rows", cluster.mappings.len() as u64);
        b.data.identity_merges.push(cluster);
    }
    Ok(())
}

/// Streamed sha256 of a file (hex).
pub fn file_sha256(path: &Path) -> Result<String, IngestError> {
    use sha2::Digest;
    let mut f = File::open(path).map_err(IngestError::io(path))?;
    let mut h = sha2::Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(IngestError::io(path))?;
    Ok(cache::hex(&h.finalize().into()))
}
