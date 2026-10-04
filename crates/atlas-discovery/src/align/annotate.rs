//! Lossless rule annotation of pinned historical sets; never reruns newer identity decisions.
use anyhow::{Context, Result, ensure};
use atlas_core::identity_rules;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

pub fn file(input: &Path, output: &Path, set: &str) -> Result<Value> {
    ensure!(input != output, "annotation must be staged in a copy");
    let mut writer = BufWriter::new(std::fs::File::create(output)?);
    let mut header: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0usize;
    for line in BufReader::new(std::fs::File::open(input)?).lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            writeln!(writer, "{line}")?;
            continue;
        }
        if header.is_empty() {
            header = line.split('\t').map(String::from).collect();
            ensure!(
                !header.iter().any(|h| h == "rule_id" || h == "rule_version"),
                "already annotated: {}",
                input.display()
            );
            writeln!(writer, "{line}\trule_id\trule_version")?;
            continue;
        }
        let cells: Vec<_> = line.split('\t').collect();
        ensure!(cells.len() == header.len(), "malformed TSV row {}", n + 1);
        let get = |key: &str| header.iter().position(|h| h == key).map(|i| cells[i]).unwrap_or("");
        let asserted = if get("asserted_predicate_id").is_empty() {
            get("predicate_id")
        } else {
            get("asserted_predicate_id")
        };
        let rid = identity_rules::select(
            set,
            get("subject_id"),
            get("object_id"),
            get("mapping_justification"),
            asserted,
        )
        .with_context(|| format!("unregistered producer set {set}"))?;
        let rule = identity_rules::rule(rid, identity_rules::VERSION).unwrap();
        ensure!(
            rule.predicates.iter().any(|p| p == get("predicate_id")),
            "{rid} cannot emit {}",
            get("predicate_id")
        );
        writeln!(writer, "{line}\t{rid}\t{}", identity_rules::VERSION)?;
        n += 1;
        *counts.entry(get("predicate_id").into()).or_default() += 1;
    }
    writer.flush()?;
    // Independent second read proves equality of every old field, not just row counts.
    let old = BufReader::new(std::fs::File::open(input)?).lines();
    let new = BufReader::new(std::fs::File::open(output)?).lines();
    let (mut old, mut new) = (old, new);
    loop {
        let (a, b) = match (old.next(), new.next()) {
            (None, None) => break,
            (Some(a), Some(b)) => (a?, b?),
            _ => anyhow::bail!("annotation changed line count"),
        };
        let projection = if a.starts_with('#') || a.is_empty() {
            b.as_str()
        } else {
            b.rsplit_once('\t')
                .and_then(|(s, _)| s.rsplit_once('\t'))
                .map(|(s, _)| s)
                .context("missing rule columns")?
        };
        ensure!(a == projection, "annotation changed historical data");
    }
    Ok(
        json!({"set": set, "rows": n, "by_predicate": counts, "old_columns_identical": true,
        "input_sha256": super::sha256_file(input)?.0, "output_sha256": super::sha256_file(output)?.0}),
    )
}

pub fn directory(input: &Path, output: &Path) -> Result<Value> {
    ensure!(
        input.canonicalize()? != output.canonicalize().unwrap_or_else(|_| output.to_path_buf()),
        "staging output must differ from input"
    );
    std::fs::create_dir_all(output)?;
    let mut entries: Vec<_> = std::fs::read_dir(input)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    let mut sets = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_file() || name == "rules.json" {
            continue;
        }
        if let Some(set) = name
            .strip_suffix(".sssom.tsv")
            .or_else(|| name.strip_suffix(".precision-sample.tsv"))
        {
            let report = file(&entry.path(), &output.join(&name), set)?;
            if name.ends_with(".sssom.tsv") {
                sets.push(report);
            }
        } else if name.ends_with(".json") || name.ends_with(".tsv") {
            std::fs::copy(entry.path(), output.join(name))?;
        }
    }
    std::fs::write(output.join("rules.json"), identity_rules::REGISTRY_JSON)?;
    let report = json!({"schema": "atlas.identity.rule-annotation", "version": 1,
        "prov:wasGeneratedBy": {"@type": "prov:Activity", "label": "lossless rule annotation",
            "prov:endedAtTime": super::now_utc(), "tool": super::TOOL, "version": env!("CARGO_PKG_VERSION"),
            "parameters": {"old_columns": "preserved verbatim", "identity_decisions": "pinned input rows"}},
        "sets": sets});
    std::fs::write(
        output.join("rule-annotation.manifest.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_annotation_keeps_demoted_assertions_and_typed_links() {
        let dir = std::env::temp_dir().join(format!("atlas-rule-annotation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.tsv");
        let output = dir.join("out.tsv");
        std::fs::write(&input, "# license: CC0\nsubject_id\tpredicate_id\tobject_id\tmapping_justification\tasserted_predicate_id\tconflict\nHGNC:1\tskos:closeMatch\tNCBIGene:1\tsemapv:DatabaseCrossReference\tskos:exactMatch\tretired\nHGNC:1\tRO:0002205\tUniProtKB:P1\tsemapv:DatabaseCrossReference\tRO:0002205\t\n").unwrap();
        let report = file(&input, &output, "gene-xrefs").unwrap();
        assert_eq!(report["rows"], 2);
        let text = std::fs::read_to_string(output).unwrap();
        assert!(text.contains("retired\tR-GEN-01\t1.0.0"));
        assert!(text.contains("\tR-GEN-02\t1.0.0"));
        std::fs::remove_file(input).unwrap();
        std::fs::remove_file(dir.join("out.tsv")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
