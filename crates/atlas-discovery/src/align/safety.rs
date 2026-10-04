//! Reversible exact-identity projection. Input assertions are never overwritten.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use super::{CLOSE, XREF, prefix};

pub use atlas_core::identity_policy::{
    LEGACY_XREF, RULE, VERSION, eligibility, known_justification, regime, valid_curie, valid_doi,
};

pub fn producer_rule(set: &str, row: &super::Row) -> Option<&'static str> {
    Some(match set {
        "disease-xrefs" if !row.object_id.starts_with("GARD:") => "R-DIS-06",
        "gene-xrefs" if !row.object_id.starts_with("UniProtKB:") => "R-GEN-03",
        "org-ror" => "R-ORG-06",
        "org-ror-affiliation" => "R-ORG-07",
        "funder-ror" if row.justification != super::LEXICAL => "R-FUN-02",
        "work-ids" => "R-WRK-02",
        _ => {
            return atlas_core::identity_rules::select(
                set,
                &row.subject_id,
                &row.object_id,
                &row.justification,
                &row.asserted,
            );
        }
    })
}

#[derive(Clone)]
struct Assertion {
    file: String,
    line: usize,
    cells: Vec<String>,
    hash: String,
    reason: Option<String>,
}
struct Intake {
    name: String,
    meta: Vec<String>,
    header: Vec<String>,
    rows: Vec<Assertion>,
    entity: Value,
}
fn get<'a>(h: &[String], cells: &'a [String], name: &str) -> &'a str {
    h.iter()
        .position(|c| c == name)
        .and_then(|i| cells.get(i))
        .map(String::as_str)
        .unwrap_or("")
}
fn evidence<'a>(h: &[String], cells: &'a [String], standard: &str, legacy: &str) -> &'a str {
    let value = get(h, cells, standard);
    if value.is_empty() { get(h, cells, legacy) } else { value }
}
fn set(h: &mut Vec<String>, cells: &mut Vec<String>, name: &str, value: &str) {
    let i = h.iter().position(|c| c == name).unwrap_or_else(|| {
        h.push(name.into());
        h.len() - 1
    });
    cells.resize(h.len(), String::new());
    cells[i] = value.into();
}

/// Hash-bound revocation overlay. A review can remove an accepted assertion, never bypass the gate.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Review {
    pub input_digest: String,
    pub actor: String,
    pub reviewed_at: String,
    pub reason: String,
    pub revoke: Vec<String>,
}

/// New immutable directory containing a projection, per-assertion receipt ledger and review diff.
/// A failed stage is not consumable: only a final manifest indicates completion.
pub fn directory(input: &Path, output: &Path, review: Option<&Path>) -> Result<Value> {
    ensure!(!output.exists(), "output must be a new immutable directory");
    let mut paths: Vec<_> = std::fs::read_dir(input)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".sssom.tsv"))
        .collect();
    paths.sort();
    ensure!(!paths.is_empty(), "no mapping inputs");
    let started = super::now_utc();
    let mut files = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (hash, bytes) = super::sha256_file(&path)?;
        let entity = json!({"@id": format!("urn:sha256:{hash}"), "@type": "prov:Entity",
            "path": path.display().to_string(), "sha256": hash, "bytes": bytes,
            "source_url": format!("https://w3id.org/rare-atlas/mappings/{}", name.trim_end_matches(".sssom.tsv")),
            "version": "content-addressed intake", "retrieved_at": null, "record_locator": "whole file",
            "retrieval_note": "local intake time is not the original download time"});
        let (mut meta, mut header, mut rows) = (Vec::new(), Vec::new(), Vec::new());
        for (i, line) in BufReader::new(std::fs::File::open(&path)?).lines().enumerate() {
            let line = line?;
            if line.starts_with('#') {
                meta.push(line);
                continue;
            }
            if line.is_empty() {
                continue;
            }
            if header.is_empty() {
                header = line.split('\t').map(String::from).collect::<Vec<_>>();
                for col in ["subject_id", "predicate_id", "object_id", "mapping_justification"] {
                    ensure!(header.iter().any(|h| h == col), "missing {col} in {name}");
                }
                continue;
            }
            let cells = line.split('\t').map(String::from).collect::<Vec<_>>();
            ensure!(cells.len() == header.len(), "malformed row in {name} line {}", i + 1);
            let rid = format!(
                "{:x}",
                Sha256::digest(format!("{name}|{hash}|{}|{line}", i + 1).as_bytes())
            );
            rows.push(Assertion {
                file: name.clone(),
                line: i + 1,
                cells,
                hash: rid,
                reason: None,
            });
        }
        ensure!(
            super::sha256_file(&path)?.0 == entity["sha256"],
            "input changed during intake: {name}"
        );
        files.push(Intake {
            name,
            meta,
            header,
            rows,
            entity,
        });
    }
    let input_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &files.iter().map(|f| (&f.name, &f.entity["sha256"])).collect::<Vec<_>>()
        )?)
    );
    let review = review
        .map(|p| -> Result<Review> { Ok(serde_json::from_slice(&std::fs::read(p)?)?) })
        .transpose()?;
    // Different policies and review overlays remain distinct PROV activities on identical inputs.
    let software_hash = format!(
        "{:x}",
        Sha256::digest(
            concat!(
                include_str!("safety.rs"),
                include_str!("../../../atlas-core/src/identity_policy.rs")
            )
            .replace("\r\n", "\n")
            .as_bytes()
        )
    );
    let review_hash = review
        .as_ref()
        .map(|r| serde_json::to_vec(r).map(|bytes| format!("{:x}", Sha256::digest(bytes))))
        .transpose()?
        .unwrap_or_else(|| "no-review".into());
    let activity_id =
        format!("urn:rare-atlas:identity-gate:{RULE}:{VERSION}:{input_digest}:{software_hash}:{review_hash}");
    let mut revoked = BTreeSet::new();
    if let Some(r) = &review {
        ensure!(
            r.input_digest == input_digest,
            "review is stale or for different input bytes"
        );
        ensure!(
            !r.actor.trim().is_empty() && !r.reviewed_at.trim().is_empty() && !r.reason.trim().is_empty(),
            "review requires actor, time and reason"
        );
        let all: BTreeSet<_> = files
            .iter()
            .flat_map(|f| f.rows.iter().map(|r| r.hash.as_str()))
            .collect();
        for id in &r.revoke {
            ensure!(all.contains(id.as_str()), "review names unknown assertion");
            revoked.insert(id.clone());
        }
    }
    let mut links = super::MappingSet::default();
    let mut index = Vec::new();
    let mut typed = Vec::new();
    for (fi, file) in files.iter_mut().enumerate() {
        for (ri, row) in file.rows.iter_mut().enumerate() {
            let h = &file.header;
            let c = &row.cells;
            let (a, b, p, j) = (
                get(h, c, "subject_id"),
                get(h, c, "object_id"),
                get(h, c, "predicate_id"),
                get(h, c, "mapping_justification"),
            );
            if matches!(
                p,
                "skos:broadMatch" | "skos:narrowMatch" | "skos:relatedMatch" | "RO:0002205" | "RO:HOM0000017"
            ) {
                typed.push((a.to_owned(), b.to_owned()));
            }
            if !atlas_core::identity_policy::is_identity(p) {
                continue;
            }
            let j = if j == LEGACY_XREF { XREF } else { j };
            let rule_id = get(h, c, "rule_id");
            row.reason = eligibility(a, b, j, get(h, c, "conflict"), rule_id)
                .err()
                .map(String::from);
            if row.reason.is_none()
                && !atlas_core::identity_rules::rule(rule_id, get(h, c, "rule_version"))
                    .is_some_and(|rule| rule.predicates.iter().any(|allowed| allowed == p))
            {
                row.reason = Some("unknown_rule_or_version".into());
            }
            let receipt = (
                evidence(h, c, "evidence_url", "prov_source_url"),
                evidence(h, c, "evidence_locator", "prov_locator"),
                evidence(h, c, "evidence_sha256", "prov_sha256"),
            );
            if row.reason.is_none()
                && (receipt.0.is_empty()
                    || receipt.1.is_empty()
                    || receipt.2.len() != 64
                    || !receipt.2.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                row.reason = Some("missing_evidence_receipt".into());
            }
            if revoked.contains(&row.hash) {
                row.reason = Some("review_revoked".into());
            }
            if row.reason.is_none() {
                links.rows.push(super::Row::xref(a, b, 0, &row.hash));
                index.push((fi, ri));
            }
        }
    }
    // Smallest LogMap-style consistency screen: quarantine inconsistent components with witnesses.
    // No OWL satisfiability, learned priorities or numerical probability is claimed.
    let comps = super::clusters(&links.rows);
    let mut by_id = HashMap::new();
    let mut bad = BTreeMap::<usize, String>::new();
    for (i, comp) in comps.iter().enumerate() {
        let mut namespaces = BTreeSet::new();
        let mut collisions = BTreeSet::new();
        for id in comp {
            by_id.insert(id.clone(), i);
            let occurrence_alias = matches!(prefix(id), "ICTRP" | "pubmed.author" | "epmc.author");
            if !occurrence_alias && !namespaces.insert(prefix(id)) {
                collisions.insert(prefix(id));
            }
        }
        if !collisions.is_empty() {
            bad.insert(
                i,
                format!(
                    "component_prefix_collision:{}",
                    collisions.into_iter().collect::<Vec<_>>().join(",")
                ),
            );
        }
    }
    for (a, b) in typed {
        if let (Some(&i), Some(&j)) = (by_id.get(&a), by_id.get(&b))
            && i == j
        {
            bad.insert(i, "component_collapses_typed_relation".into());
        }
    }
    for (link, (fi, ri)) in links.rows.iter().zip(index) {
        if let Some(reason) = by_id.get(&link.subject_id).and_then(|i| bad.get(i)) {
            files[fi].rows[ri].reason = Some(reason.clone());
        }
    }
    std::fs::create_dir_all(output)?;
    let mut ledger = BufWriter::new(std::fs::File::create(output.join("identity-decisions.jsonl"))?);
    let mut invalidations = BufWriter::new(std::fs::File::create(output.join("identity-invalidations.jsonl"))?);
    let mut diff = BufWriter::new(std::fs::File::create(output.join("review-diff.tsv"))?);
    writeln!(
        diff,
        "assertion_id\tfile\tline\tsubject_id\tobject_id\told_predicate\tnew_predicate\treason"
    )?;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut outputs = Vec::new();
    for file in &mut files {
        let mut writer = BufWriter::new(std::fs::File::create(output.join(&file.name))?);
        let added_cols = [
            "asserted_predicate_id",
            "conflict",
            "original_mapping_justification",
            "original_rule_id",
            "original_rule_version",
            "identity_assertion_id",
            "identity_decision_id",
            "identity_gate_rule",
            "identity_gate_version",
            "evidence_url",
            "evidence_sha256",
            "evidence_locator",
        ];
        let missing: Vec<_> = added_cols
            .iter()
            .filter(|col| !file.header.iter().any(|h| h == **col))
            .collect();
        let definitions = |writer: &mut BufWriter<std::fs::File>| -> Result<()> {
            for col in &missing {
                writeln!(
                    writer,
                    "#   - slot_name: {col}\n#     property: https://w3id.org/rare-atlas/{col}\n#     type_hint: xsd:string"
                )?;
            }
            Ok(())
        };
        let mut declared = false;
        for line in &file.meta {
            if line.starts_with("# mapping_set_version:") {
                writeln!(
                    writer,
                    "{}",
                    line.replacen("mapping_set_version:", "original_mapping_set_version:", 1)
                )?;
            } else {
                writeln!(writer, "{line}")?;
            }
            if line.trim_end() == "# curie_map:"
                && !file
                    .meta
                    .iter()
                    .any(|line| line.trim_start_matches('#').trim_start().starts_with("xsd:"))
            {
                writeln!(writer, "#   xsd: http://www.w3.org/2001/XMLSchema#")?;
            }
            if line.trim_end() == "# extension_definitions:" {
                definitions(&mut writer)?;
                declared = true;
            }
        }
        if !declared {
            writeln!(writer, "# extension_definitions:")?;
            definitions(&mut writer)?;
        }
        writeln!(writer, "# mapping_set_version: identity-gate-{VERSION}-{input_digest}")?;
        writeln!(
            writer,
            "# identity_gate: {RULE} {VERSION}; original assertions in identity-decisions.jsonl"
        )?;
        for col in added_cols {
            if !file.header.iter().any(|h| h == col) {
                file.header.push(col.into());
            }
        }
        writeln!(writer, "{}", file.header.join("\t"))?;
        for row in &mut file.rows {
            let original = row.cells.clone();
            let original_header = file.header[..original.len()].to_vec();
            let before = get(&original_header, &original, "predicate_id");
            let status = if !atlas_core::identity_policy::is_identity(before) {
                "typed_or_candidate"
            } else if row.reason.is_some() {
                "rejected"
            } else {
                "accepted"
            };
            *counts.entry(status.into()).or_default() += 1;
            let why = row.reason.as_deref().unwrap_or(status);
            let assertion_id = format!("urn:rare-atlas:assertion:{}", row.hash);
            let decision_id = format!("{assertion_id}:decision:{RULE}:{VERSION}:{software_hash}:{review_hash}");
            set(&mut file.header, &mut row.cells, "identity_decision_id", &decision_id);
            let j = get(&original_header, &original, "mapping_justification");
            set(&mut file.header, &mut row.cells, "original_mapping_justification", j);
            set(
                &mut file.header,
                &mut row.cells,
                "original_rule_id",
                get(&original_header, &original, "rule_id"),
            );
            set(
                &mut file.header,
                &mut row.cells,
                "original_rule_version",
                get(&original_header, &original, "rule_version"),
            );
            if j == LEGACY_XREF {
                set(&mut file.header, &mut row.cells, "mapping_justification", XREF);
            } else if !known_justification(j) {
                set(
                    &mut file.header,
                    &mut row.cells,
                    "mapping_justification",
                    "semapv:UnspecifiedMatching",
                );
            }
            set(&mut file.header, &mut row.cells, "identity_assertion_id", &assertion_id);
            set(&mut file.header, &mut row.cells, "identity_gate_rule", RULE);
            set(&mut file.header, &mut row.cells, "identity_gate_version", VERSION);
            for (standard, legacy) in [
                ("evidence_url", "prov_source_url"),
                ("evidence_sha256", "prov_sha256"),
                ("evidence_locator", "prov_locator"),
            ] {
                set(
                    &mut file.header,
                    &mut row.cells,
                    standard,
                    evidence(&original_header, &original, standard, legacy),
                );
            }
            if row.reason.is_some() {
                set(&mut file.header, &mut row.cells, "predicate_id", CLOSE);
                set(&mut file.header, &mut row.cells, "asserted_predicate_id", before);
                let old = get(&original_header, &original, "conflict");
                set(
                    &mut file.header,
                    &mut row.cells,
                    "conflict",
                    &if old.is_empty() {
                        why.into()
                    } else {
                        format!("{old};{why}")
                    },
                );
            }
            let after = get(&file.header, &row.cells, "predicate_id");
            if before != after || j == LEGACY_XREF {
                writeln!(
                    diff,
                    "{}\t{}\t{}\t{}\t{}\t{before}\t{after}\t{why}",
                    row.hash,
                    row.file,
                    row.line,
                    get(&original_header, &original, "subject_id"),
                    get(&original_header, &original, "object_id")
                )?;
            }
            let witness = by_id
                .get(get(&original_header, &original, "subject_id"))
                .and_then(|i| bad.get(i).map(|_| &comps[*i]));
            let record = json!({"@id": decision_id, "@type":"prov:Entity", "status":status,"reason":why,
                "@context":{"prov":"http://www.w3.org/ns/prov#","@vocab":"https://w3id.org/rare-atlas/vocab/"},
                "rule_id":RULE,"rule_version":VERSION,"prov:wasGeneratedBy":{"@id":activity_id},
                "prov:wasDerivedFrom":{"@id":assertion_id,"@type":"prov:Entity","prov:wasDerivedFrom":file.entity,
                    "record_locator":format!("line {}",row.line),"original_tsv":original.join("\t"),"original_columns":original_header,"original_values":original},
                "conflict_component_witness":witness,"review":review});
            if status == "rejected" {
                // Existing graph owners must rebuild from the accepted projection: deleting just the
                // direct edge cannot split an already materialized union or retract inferred paths.
                writeln!(
                    invalidations,
                    "{}",
                    serde_json::to_string(&json!({
                        "@context":{"prov":"http://www.w3.org/ns/prov#","@vocab":"https://w3id.org/rare-atlas/vocab/"},
                        "@id":format!("{decision_id}:invalidation"), "@type":"prov:Entity",
                        "prov:wasDerivedFrom":{"@id":decision_id},
                        "invalidated_assertion_id":assertion_id,
                        "subject_id":get(&original_header, &original, "subject_id"),
                        "object_id":get(&original_header, &original, "object_id"),
                        "file":row.file, "line":row.line, "source_sha256":file.entity["sha256"],
                        "original_rule_id":get(&original_header, &original, "rule_id"),
                        "original_rule_version":get(&original_header, &original, "rule_version"),
                        "reason":why, "scope":"identity merge and all transitive merge/inference dependents",
                        "required_action":"rebuild identity components and inference projections from accepted decisions before activation",
                        "activation_status":"pending graph-owner rebuild; no live graph changed"
                    }))?
                )?;
            }
            writeln!(ledger, "{}", serde_json::to_string(&record)?)?;
            writeln!(writer, "{}", row.cells.join("\t"))?;
        }
        writer.flush()?;
        outputs.push(json!({"file":file.name,"sha256":super::sha256_file(&output.join(&file.name))?.0}));
    }
    ledger.flush()?;
    invalidations.flush()?;
    diff.flush()?;
    for name in [
        "identity-decisions.jsonl",
        "identity-invalidations.jsonl",
        "review-diff.tsv",
    ] {
        outputs.push(json!({"file":name,"sha256":super::sha256_file(&output.join(name))?.0}));
    }
    std::fs::write(output.join("rules.json"), atlas_core::identity_rules::REGISTRY_JSON)?;
    outputs.push(json!({"file":"rules.json","sha256":super::sha256_file(&output.join("rules.json"))?.0}));
    let manifest = json!({"@context":{"prov":"http://www.w3.org/ns/prov#","rda":"https://w3id.org/rare-atlas/vocab/",
        "@vocab":"https://w3id.org/rare-atlas/vocab/",
        "status":"rda:status","reason":"rda:reason","rule_id":"rda:rule_id","rule_version":"rda:rule_version"},
        "schema":"atlas.identity.gate","version":1,"release":false,"input_digest":input_digest,"rule_id":RULE,"rule_version":VERSION,
        "software_code_sha256":software_hash,"software_code_hash_scope":"UTF-8 source with CRLF normalized to LF",
        "identity_policy_sha256": atlas_core::identity_policy::code_sha256(),
        "counts":counts,"conflicting_components":bad.len(),"outputs":outputs,
        "activation_requirements":["atomic SEMAPV reader/writer migration", "rebuild all identity components from accepted decisions",
            "retract and rebuild all dependent inferences using identity-invalidations.jsonl; do not reuse the previous inferred snapshot"],
        "prov:wasGeneratedBy":{"@id":activity_id,"@type":"prov:Activity",
            "prov:startedAtTime":started,"prov:endedAtTime":super::now_utc(),
            "prov:wasAssociatedWith":{"@type":"prov:SoftwareAgent","version":env!("CARGO_PKG_VERSION"),"name":super::TOOL},
            "prov:used":files.iter().map(|f| &f.entity).collect::<Vec<_>>(),"review":review},
        "limitations":["structural component quarantine, not full OWL/LogMap repair", "unknown lifecycle retained as unknown",
            "all original row values retained in ledger", "gate projection must be enabled together with SEMAPV merge readers"]});
    // Completion marker written last. Consumers must pin this manifest and the projected file hashes.
    std::fs::write(
        output.join("identity-gate.manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}
