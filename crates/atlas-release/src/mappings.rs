//! Authoritative cross-reference projection into SSSOM 1.1-compatible TSV.
use crate::{atlas_refs, may_copy, policy, quarantine::Quarantine};
use anyhow::Result;
use atlas_core::Atlas;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub(crate) fn mappings(
    atlas: &Atlas,
    graph: &atlas_core::Graph,
    target: &Path,
    version: &str,
    quarantine: &Quarantine,
    withhold: &atlas_core::withhold::Withhold,
    files: &BTreeMap<String, String>,
    node_ids: &BTreeSet<String>,
    turtle: &mut crate::rdf::Writer,
) -> Result<(usize, usize)> {
    // Export authoritative ID equivalences from the identity layer. No labels or quoted evidence.
    let mut writer = BufWriter::new(File::create(target.join("mappings/disease-identity.sssom.tsv"))?);
    writeln!(
        writer,
        "# mapping_set_id: https://w3id.org/rare-disease-atlas/mappings/disease-identity\n# mapping_set_version: {version}\n# license: {}\n# mapping_date: {}\n# creator_id:\n#   - ra:atlas-release\n# mapping_set_description: Authoritative exact identifier alignments from the atlas identity layer; labels withheld\n# curie_map:\n#   MONDO: http://purl.obolibrary.org/obo/MONDO_\n#   OMIM: https://omim.org/entry/\n#   ORPHA: https://www.orpha.net/en/disease/detail/\n#   skos: http://www.w3.org/2004/02/skos/core#\n#   semapv: https://w3id.org/semapv/vocab/\n#   ra: https://w3id.org/rare-disease-atlas/vocab#",
        policy::CC_BY,
        policy::CHECKED
    )?;
    let mut tsv = csv::WriterBuilder::new().delimiter(b'\t').from_writer(writer);
    tsv.write_record([
        "subject_id",
        "predicate_id",
        "object_id",
        "mapping_justification",
        "mapping_date",
        "mapping_tool",
        "subject_source",
        "object_source",
        "mapping_source",
        "comment",
        "rule_id",
        "rule_version",
        "identity_assertion_ids",
        "identity_decision_ids",
        "identity_gate_manifest_sha256",
    ])?;
    let mut count = 0;
    let mut excluded = 0;
    for d in atlas.diseases() {
        if !may_copy(&atlas_refs(atlas, &d.derived_from)) {
            continue;
        }
        for source_id in &d.source_ids {
            if source_id == &d.id
                || !["MONDO:", "OMIM:", "ORPHA:"].iter().any(|p| source_id.starts_with(p))
                || !["MONDO:", "OMIM:", "ORPHA:"].iter().any(|p| d.id.starts_with(p))
            {
                continue;
            }
            // Old label-merge snapshots are not authoritative cross-reference mappings.
            if !atlas.identity.mapping_basis(source_id).is_some_and(|b| {
                matches!(
                    b,
                    atlas_core::identity::MappingBasis::ExactMondo | atlas_core::identity::MappingBasis::OrphanetE
                )
            }) {
                continue;
            }
            let Some(merge) = graph.identity_merge(&d.id) else {
                excluded += 1;
                continue;
            };
            if !merge.members.iter().any(|member| &member.id == source_id)
                || merge.mappings.is_empty()
                || merge
                    .mappings
                    .iter()
                    .any(|m| m.decision_id.is_empty() || m.gate_manifest_sha256.len() != 64)
            {
                excluded += 1;
                continue;
            }
            let refs = atlas_refs(atlas, &d.derived_from);
            if quarantine.blocks(&refs, files)
                || crate::suppression::blocked(withhold, source_id)
                || !node_ids.contains(&d.id)
            {
                excluded += 1;
                continue;
            }
            let provenance = serde_json::to_string(
                &refs
                    .iter()
                    .map(|r| {
                        serde_json::json!({"source_url": r.source_url, "version": r.version, "sha256": r.sha256,
                    "record_locator": r.record_locator, "source_entity": r.source_entity})
                    })
                    .collect::<Vec<_>>(),
            )?;
            let source = |id: &str| match id.split(':').next().unwrap() {
                "MONDO" => "ra:source-mondo",
                "ORPHA" => "ra:source-orphadata",
                _ => "ra:source-omim",
            };
            tsv.write_record([
                source_id.as_str(),
                "skos:exactMatch",
                &d.id,
                "semapv:MappingChaining",
                policy::CHECKED,
                "atlas-release (authoritative upstream curated cross-reference projection)",
                source(source_id),
                source(&d.id),
                &format!("ra:source-{}", refs[0].source),
                &provenance,
                "R-DIS-08",
                "1.0.0",
                &serde_json::to_string(&merge.mappings.iter().map(|m| &m.assertion_id).collect::<Vec<_>>())?,
                &serde_json::to_string(&merge.mappings.iter().map(|m| &m.decision_id).collect::<Vec<_>>())?,
                &merge.mappings[0].gate_manifest_sha256,
            ])?;
            crate::rdf::mapping(turtle, source_id, &d.id, &refs)?;
            count += 1;
        }
    }
    tsv.flush()?;
    Ok((count, excluded))
}
