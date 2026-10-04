//! Bundle documentation, file inventory and byte checksums.
use crate::{policy, quarantine::QuarantineSummary};
use anyhow::Result;
use atlas_core::graph::hex;
use atlas_core::{Atlas, Graph};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

pub(crate) fn write_json(path: impl AsRef<Path>, v: &impl Serialize) -> Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut f, v)?;
    f.write_all(b"\n")?;
    f.flush()?;
    Ok(())
}

pub(crate) fn write_support(
    target: &Path,
    version: &str,
    atlas: &Atlas,
    graph: &Graph,
    used: &BTreeSet<String>,
    quarantine: &QuarantineSummary,
) -> Result<()> {
    let mut notices = BTreeMap::new();
    for (ns, prov) in [("atlas", &atlas.provenance), ("graph", graph.provenance())] {
        for e in &prov.entities {
            if !used.contains(&format!("{ns}:{}", e.id)) {
                continue;
            }
            let p = policy::for_entity(e);
            notices.insert(p.source, p);
        }
    }
    write_json(target.join("source-policies.json"), &notices)?;
    fs::write(
        target.join("LICENCES.md"),
        include_str!("../../../docs/release/LICENCES.md"),
    )?;
    let mut license = format!(
        "Rare Disease Atlas derived contribution — CC BY 4.0\n{}\n\nCopyright 2026 Rare Disease Atlas contributors.\nYou may share and adapt our contribution, including commercially, with attribution,\na licence link and an indication of changes. No endorsement. No warranties.\nThird-party rights remain with their holders. This licence does not relicense\nthird-party inputs or grant rights to content reached through source links.\nThe CC BY 4.0 legal code is incorporated by reference:\nhttps://creativecommons.org/licenses/by/4.0/legalcode.en\n\nTHIRD-PARTY NOTICES\n",
        policy::CC_BY
    );
    for p in notices.values() {
        license.push_str(&format!(
            "\n{}\n{}\nTerms: {} (checked {})\n",
            p.source, p.attribution, p.terms_url, p.checked_on
        ));
    }
    fs::write(target.join("LICENSE"), license)?;
    fs::write(
        target.join("CITATION.cff"),
        format!(
            "cff-version: 1.2.0\nmessage: 'Please cite this dataset and its attributed upstream sources.'\ntype: dataset\ntitle: 'Rare Disease Atlas licence-aware knowledge graph'\nversion: '{}'\ndate-released: '{}'\nlicense: CC-BY-4.0\nauthors:\n  - name: 'Rare Disease Atlas contributors'\n",
            version.replace('\'', "''"),
            policy::CHECKED
        ),
    )?;
    fs::write(
        target.join("README.md"),
        format!(
            "# Rare Disease Atlas — {version}\n\nLocal release candidate; not published. Cite CITATION.cff plus upstream attribution in LICENSE.\nNo DOI has been assigned. Our contribution is CC BY 4.0; source terms are retained.\n\n- graph.ttl: RDF 1.2 Turtle, explicit reifiers for licensed assertions; PROV-O lineage.\n- nodes.jsonl / edges.jsonl: whitelisted projections; link_only records contain identifiers and provenance only.\n- mappings/: SSSOM TSV of authoritative disease-identity equivalences.\n- integrity.json: same core check and counts as /api/integrity, without personal conflict details.\n- release-report.json: counts, withheld records, per-source memberships and byte sizes.\n- source-policies.json / LICENCES.md: machine and human policy notices.\n- datasheet.md / LICENSE / CITATION.cff / SHA256SUMS: documentation, attribution and integrity.\n\nRestricted link_only edge ids are opaque identifiers: do not interpret them as asserted RDF statements.\nNodes and edges retain exact record locators; raw-file hashes cover file bytes, while connected-layer\nrecord hashes use the upstream canonical JSON/JSON-line/TSV algorithm stated in hash_scope.\nRaw retrieval dates may be filesystem mtime proxies: retrieval_basis preserves that limitation.\nAbsent upstream versions are represented by snapshot-sha256, not an invented upstream release.\n\nVerify from the repository in Git Bash:\n```bash\nuv run --with 'pyoxigraph>=0.5.11' --with 'sssom>=0.4' python crates/atlas-release/scripts/validate_release.py release/{version}\n(cd release/{version} && sha256sum -c SHA256SUMS)\n```\nFor a same-snapshot server, add --integrity-url http://127.0.0.1:8010/api/integrity.\nSee crates/atlas-release/README.md for reproduction.\n"
        ),
    )?;
    fs::write(
        target.join("datasheet.md"),
        format!(
            "{}\n## This bundle's quarantine snapshot\n\nManifest present: {}. Byte SHA-256: `{}`. Input entries: {}; unique\ncache-file/containing-record pairs: {}. Quarantine exclusions: {} nodes,\n{} edges, {} mapping rows. Full per-cache counts are in `release-report.json`.\n",
            include_str!("../../../docs/release/DATASHEET.md"),
            quarantine.present,
            quarantine
                .manifest_sha256
                .as_deref()
                .unwrap_or("missing; empty quarantine"),
            quarantine.input_entries,
            quarantine.listed_records,
            quarantine.excluded_nodes,
            quarantine.excluded_edges,
            quarantine.excluded_mapping_rows
        ),
    )?;
    Ok(())
}

pub(crate) fn files(root: &Path) -> Result<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let p = entry?.path();
            if p.is_dir() {
                walk(root, &p, out)?;
            } else {
                out.push(p.strip_prefix(root)?.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    walk(root, root, &mut result)?;
    result.sort();
    Ok(result)
}

pub(crate) fn checksums(root: &Path) -> Result<()> {
    let mut out = BufWriter::new(File::create(root.join("SHA256SUMS"))?);
    for file in files(root)?.into_iter().filter(|p| p != "SHA256SUMS") {
        let mut input = File::open(root.join(&file))?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut input, &mut HashWriter(&mut hasher))?;
        writeln!(out, "{}  {file}", hex(&hasher.finalize().into()))?;
    }
    out.flush()?;
    Ok(())
}

struct HashWriter<'a>(&'a mut Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
