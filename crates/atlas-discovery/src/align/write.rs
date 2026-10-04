//! Writers: SSSOM TSV (embedded YAML metadata), conflicts TSV, PROV manifest, pinned precision sample.

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{MappingSet, Row, TOOL, now_utc, today};

const COLUMNS: [&str; 26] = [
    "subject_id",
    "subject_label",
    "predicate_id",
    "object_id",
    "object_label",
    "mapping_justification",
    "confidence",
    "mapping_cardinality",
    "subject_source",
    "object_source",
    "author_id",
    "mapping_date",
    "mapping_tool",
    "comment",
    "asserted_predicate_id",
    "conflict",
    "evidence_url",
    "evidence_sha256",
    "evidence_locator",
    "rule_id",
    "rule_version",
    "original_mapping_justification",
    "source_version",
    "source_retrieved_at",
    "identity_gate_rule",
    "identity_gate_version",
];

const EXTENSIONS: [(&str, &str, &str); 12] = [
    (
        "identity_gate_rule",
        "https://w3id.org/rare-atlas/identity_gate_rule",
        "xsd:string",
    ),
    (
        "identity_gate_version",
        "https://w3id.org/rare-atlas/identity_gate_version",
        "xsd:string",
    ),
    (
        "original_mapping_justification",
        "https://w3id.org/rare-atlas/original_mapping_justification",
        "xsd:string",
    ),
    (
        "source_version",
        "https://w3id.org/rare-atlas/source_version",
        "xsd:string",
    ),
    (
        "source_retrieved_at",
        "https://w3id.org/rare-atlas/source_retrieved_at",
        "xsd:string",
    ),
    ("rule_id", "https://w3id.org/rare-atlas/rule_id", "xsd:string"),
    ("rule_version", "https://w3id.org/rare-atlas/rule_version", "xsd:string"),
    (
        "asserted_predicate_id",
        "https://w3id.org/rare-atlas/asserted_predicate_id",
        "xsd:string",
    ),
    ("conflict", "https://w3id.org/rare-atlas/conflict", "xsd:string"),
    ("evidence_url", "http://purl.org/dc/terms/source", "xsd:anyURI"),
    (
        "evidence_sha256",
        "https://w3id.org/rare-atlas/evidence_sha256",
        "xsd:string",
    ),
    (
        "evidence_locator",
        "https://w3id.org/rare-atlas/evidence_locator",
        "xsd:string",
    ),
];

pub const SAMPLE_EXACT: usize = 20;
pub const SAMPLE_CANDIDATE: usize = 10;

#[derive(Debug, Serialize)]
pub struct SetSummary {
    pub set: String,
    pub rows: usize,
    pub by_predicate: BTreeMap<String, usize>,
    pub conflicts: BTreeMap<String, usize>,
    pub excluded: BTreeMap<String, u64>,
    pub elapsed_ms: u128,
}

fn cell(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

fn yaml(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn row_line(set: &MappingSet, r: &Row, date: &str) -> String {
    let conf = format!("{:.2}", r.confidence);
    let input = &set.inputs[r.input];
    let author = "rda:atlas-discovery-align";
    let tool = format!("{TOOL} {}", version());
    let rule_id = super::safety::producer_rule(&set.id, r).expect("write_set validates rule coverage");
    let fields: [&str; 26] = [
        &r.subject_id,
        &r.subject_label,
        &r.predicate,
        &r.object_id,
        &r.object_label,
        &r.justification,
        &conf,
        &r.cardinality,
        &r.subject_source,
        &r.object_source,
        author,
        date,
        &tool,
        &r.comment,
        &r.asserted,
        &r.conflict,
        &r.evidence_url,
        &input.sha256,
        &r.locator,
        rule_id,
        atlas_core::identity_rules::VERSION,
        &r.upstream_justification,
        &input.version,
        input.retrieved_at.as_deref().unwrap_or("unknown"),
        super::safety::RULE,
        super::safety::VERSION,
    ];
    fields.iter().map(|f| cell(f)).collect::<Vec<_>>().join("\t")
}

/// Stable pseudo-random key for a pinned sample: sha256(set id + subject + object).
fn sample_key(set: &str, r: &Row) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{set}|{}|{}", r.subject_id, r.object_id).as_bytes())
    )
}

pub fn write_set(set: &MappingSet, out: &Path, started: &str, elapsed_ms: u128) -> Result<SetSummary> {
    // Apply one final row gate to every producer. Keep the source assertion when demoting.
    let mut projection = set.clone();
    for row in &mut projection.rows {
        anyhow::ensure!(
            super::safety::known_justification(&row.justification),
            "unknown mapping justification {}",
            row.justification
        );
        if row.is_exact() {
            let rule = super::safety::producer_rule(&projection.id, row).unwrap_or("");
            if let Err(reason) =
                super::safety::eligibility(&row.subject_id, &row.object_id, &row.justification, &row.conflict, rule)
            {
                row.demote(reason);
            }
        }
    }
    let set = &projection;
    for r in &set.rows {
        let rule_id = super::safety::producer_rule(&set.id, r)
            .ok_or_else(|| anyhow::anyhow!("no implemented rule for {}", set.id))?;
        let rule = atlas_core::identity_rules::rule(rule_id, atlas_core::identity_rules::VERSION).unwrap();
        anyhow::ensure!(
            rule.predicates.contains(&r.predicate),
            "rule {rule_id} cannot emit {}",
            r.predicate
        );
    }
    std::fs::write(out.join("rules.json"), atlas_core::identity_rules::REGISTRY_JSON)?;
    let date = today();
    let path = out.join(format!("{}.sssom.tsv", set.id));
    let tmp = path.with_extension("tsv.part");
    {
        let mut w = BufWriter::new(std::fs::File::create(&tmp)?);
        let mut meta = vec![
            format!("mapping_set_id: https://w3id.org/rare-atlas/mappings/{}", set.id),
            format!("mapping_set_version: {}", yaml(&now_utc())),
            format!("mapping_set_description: {}", yaml(&set.description)),
            format!("license: {}", set.license),
            format!("mapping_date: {date}"),
            format!("mapping_tool: {}", yaml(TOOL)),
            format!("mapping_tool_version: {}", yaml(version())),
            "mapping_provider: https://github.com/Krarilotus/rare-disease-atlas".into(),
            "creator_label:".into(),
            format!(
                "  - {}",
                yaml("rare-disease-atlas align agent (software, not a human curator)")
            ),
            "curie_map:".into(),
        ];
        for (k, v) in &set.curie_map {
            meta.push(format!("  {k}: {v}"));
        }
        meta.push("extension_definitions:".into());
        for (slot, prop, ty) in EXTENSIONS {
            meta.push(format!("  - slot_name: {slot}"));
            meta.push(format!("    property: {prop}"));
            meta.push(format!("    type_hint: {ty}"));
        }
        meta.push("other: |-".into());
        meta.push("  Merge rule: skos:exactMatch rows only (1:1, source-asserted, cluster-consistent); see docs/design/IDENTITY.md.".into());
        meta.push(format!("  Provenance and inputs: {}.manifest.json", set.id));
        for m in meta {
            writeln!(w, "# {m}")?;
        }
        writeln!(w, "{}", COLUMNS.join("\t"))?;
        for r in &set.rows {
            writeln!(w, "{}", row_line(set, r, &date))?;
        }
    }
    std::fs::rename(&tmp, &path)?;

    // Conflicts: every demoted or flagged row, grouped by kind and the id that causes it.
    let mut conflicts: BTreeMap<String, usize> = BTreeMap::new();
    {
        let mut w = BufWriter::new(std::fs::File::create(out.join(format!("{}.conflicts.tsv", set.id)))?);
        writeln!(
            w,
            "conflict\tsubject_id\tasserted_predicate_id\tobject_id\tfinal_predicate_id\tmapping_cardinality\tevidence_locator\tcomment"
        )?;
        for r in set.rows.iter().filter(|r| !r.conflict.is_empty()) {
            for k in r.conflict.split(';') {
                *conflicts.entry(k.into()).or_default() += 1;
            }
            writeln!(
                w,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                r.conflict,
                r.subject_id,
                r.asserted,
                r.object_id,
                r.predicate,
                r.cardinality,
                cell(&r.locator),
                cell(&r.comment)
            )?;
        }
    }

    // Pinned precision sample: the first N exact and N candidate rows by a content hash.
    {
        let mut exact: Vec<_> = set.rows.iter().filter(|r| r.is_exact()).collect();
        exact.sort_by_cached_key(|r| sample_key(&set.id, r));
        exact.dedup_by(|a, b| a.subject_id == b.subject_id && a.object_id == b.object_id);
        let mut cand: Vec<_> = set.rows.iter().filter(|r| r.predicate == super::CLOSE).collect();
        cand.sort_by_cached_key(|r| sample_key(&set.id, r));
        cand.dedup_by(|a, b| a.subject_id == b.subject_id && a.object_id == b.object_id);
        let mut w = BufWriter::new(std::fs::File::create(
            out.join(format!("{}.precision-sample.tsv", set.id)),
        )?);
        writeln!(w, "{}", COLUMNS.join("\t"))?;
        for r in exact.iter().take(SAMPLE_EXACT).chain(
            cand.iter().take(
                set.extra
                    .get("precision_candidate_sample_size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(SAMPLE_CANDIDATE as u64) as usize,
            ),
        ) {
            writeln!(w, "{}", row_line(set, r, &date))?;
        }
    }

    let mut by_predicate: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_card: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_pair: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for r in &set.rows {
        *by_predicate.entry(r.predicate.clone()).or_default() += 1;
        if !r.cardinality.is_empty() {
            *by_card.entry(r.cardinality.clone()).or_default() += 1;
        }
        *by_pair
            .entry(format!(
                "{}->{}",
                super::prefix(&r.subject_id),
                super::prefix(&r.object_id)
            ))
            .or_default()
            .entry(r.predicate.clone())
            .or_default() += 1;
    }
    let excluded: BTreeMap<String, u64> = set.excluded.iter().map(|(k, v)| (k.clone(), v.0)).collect();
    let exclusion_file = format!("{}.exclusions.jsonl", set.id);
    {
        let mut ledger = BufWriter::new(std::fs::File::create(out.join(&exclusion_file))?);
        for (i, e) in set.exclusion_ledger.iter().enumerate() {
            let inputs: Vec<_> = match e.input {
                Some(j) => vec![&set.inputs[j]],
                None => set.inputs.iter().collect(),
            };
            let value = json!({"@id":format!("https://w3id.org/rare-atlas/mappings/{}/exclusion/{i}",set.id),
                "@type":"prov:Entity", "status":"excluded", "reason":e.reason,"record_locator":e.locator,
                "source_binding":if e.input.is_some() { "exact input" } else { "input unspecified by legacy producer; possible inputs retained" },
                "prov:wasDerivedFrom":inputs,
                "prov:wasGeneratedBy":{"@type":"prov:Activity","name":TOOL,"version":version(),"prov:startedAtTime":started}});
            writeln!(ledger, "{}", serde_json::to_string(&value)?)?;
        }
    }
    let manifest = json!({
        "@context": {"prov": "http://www.w3.org/ns/prov#", "@vocab":"https://w3id.org/rare-atlas/vocab/"},
        "@type":"prov:Entity",
        "mapping_set_id": format!("https://w3id.org/rare-atlas/mappings/{}", set.id),
        "description": set.description,
        "license": set.license,
        "rule_registry": {"file": "rules.json", "sha256": format!("{:x}", Sha256::digest(atlas_core::identity_rules::REGISTRY_JSON.as_bytes()))},
        "release": set.extra.get("release").and_then(serde_json::Value::as_bool).unwrap_or(true),
        "files": {
            "sssom": format!("{}.sssom.tsv", set.id),
            "conflicts": format!("{}.conflicts.tsv", set.id),
            "precision_sample": format!("{}.precision-sample.tsv", set.id),
            "exclusions": exclusion_file,
        },
        "output_sha256": super::sha256_file(&path)?.0,
        "exclusion_ledger_sha256": super::sha256_file(&out.join(&exclusion_file))?.0,
        "exclusion_ledger_complete": set.exclusion_ledger.len() as u64 == excluded.values().sum::<u64>(),
        "metadata": {"sssom_version":"1.0", "semapv_reference":"2026-06-10", "confidence_interpretation":"rule tier, not calibrated probability"},
        "prov:wasGeneratedBy": {
            "@type": "prov:Activity",
            "label": format!("identity alignment: {}", set.id),
            "prov:startedAtTime": started,
            "prov:endedAtTime": now_utc(),
            "prov:wasAssociatedWith": {"@type": "prov:SoftwareAgent", "name": TOOL, "version": version()},
            "parameters": set.parameters,
            "prov:used": set.inputs,
        },
        "counts": {
            "rows": set.rows.len(),
            "by_predicate": by_predicate,
            "by_cardinality_of_asserted_exact": by_card,
            "by_prefix_pair": by_pair,
            "conflicts": conflicts,
        },
        "excluded": set.excluded.iter().map(|(k, (n, ex))| (k.clone(), json!({"count": n, "examples": ex}))).collect::<BTreeMap<_, _>>(),
        "notes": set.notes,
        "extra": set.extra,
    });
    std::fs::write(
        out.join(format!("{}.manifest.json", set.id)),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(SetSummary {
        set: set.id.clone(),
        rows: set.rows.len(),
        by_predicate,
        conflicts,
        excluded,
        elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_producer_rows_and_precision_samples_reference_the_same_registered_rule() {
        let out = std::env::temp_dir().join(format!("atlas-rule-writer-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let mut set = MappingSet {
            id: "gene-xrefs".into(),
            license: "CC0-1.0".into(),
            ..Default::default()
        };
        set.inputs.push(super::super::Input {
            role: "synthetic fixture".into(),
            path: "fixture.tsv".into(),
            url: "https://example.org/fixture".into(),
            version: "fixture-v1".into(),
            sha256: "a".repeat(64),
            bytes: 0,
            license: "CC0-1.0".into(),
            retrieved_at: Some("2026-10-04T00:00:00Z".into()),
        });
        let mut row = Row::xref("HGNC:1", "NCBIGene:1", 0, "fixture row 1");
        row.evidence_url = "https://example.org/fixture".into();
        set.rows.push(row);
        let result = write_set(&set, &out, "2026-10-04T00:00:00Z", 0).unwrap();
        assert_eq!(result.rows, 1);
        for name in ["gene-xrefs.sssom.tsv", "gene-xrefs.precision-sample.tsv"] {
            let text = std::fs::read_to_string(out.join(name)).unwrap();
            let lines: Vec<_> = text.lines().filter(|l| !l.starts_with('#')).collect();
            assert!(lines[0].contains("rule_id\trule_version"));
            assert!(lines[1].contains("R-GEN-03\t1.0.0"));
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("gene-xrefs.manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["rule_registry"]["file"], "rules.json");
        for name in [
            "gene-xrefs.sssom.tsv",
            "gene-xrefs.precision-sample.tsv",
            "gene-xrefs.conflicts.tsv",
            "gene-xrefs.manifest.json",
            "gene-xrefs.exclusions.jsonl",
            "rules.json",
        ] {
            std::fs::remove_file(out.join(name)).unwrap();
        }
        std::fs::remove_dir(out).unwrap();
    }
}
