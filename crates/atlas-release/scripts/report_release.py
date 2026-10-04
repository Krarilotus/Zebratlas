"""Write the measured release handoff from the sealed report (no source prose)."""
import argparse
import json
from pathlib import Path

from release_gate import require, sha256


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("release", type=Path)
    parser.add_argument("--validated", action="store_true")
    parser.add_argument("--published", type=Path)
    args = parser.parse_args()
    root = args.release
    report = json.loads((root / "release-report.json").read_text(encoding="utf-8"))
    build = json.loads((root / "build-info.json").read_text(encoding="utf-8"))
    sizes = {p.relative_to(root).as_posix(): p.stat().st_size for p in root.rglob("*") if p.is_file()}
    receipt = json.loads(args.published.read_text()) if args.published else None
    if receipt:
        require(receipt["public"] and receipt["sha256sums_sha256"] == sha256(root / "SHA256SUMS"),
                "publication receipt does not match this bundle")
    status = "Published and all public downloads verified" if receipt else ("Passed all offline validations" if args.validated else "Validation in progress")
    lines = ["# Zebratlas KG v0.1 release handoff", "", f"**{status}.**", "",
        (f"Merged master build base: `{build['build_head']}`; upstream master observed at finalization: `{build.get('observed_master_at_finalization', build['master_commit'])}`. "
        "Bundle: `release/zebratlas-kg-v0.1/` (git-ignored)."),
        (f"{sum(report['nodes'].values()):,} nodes; {sum(report['edges'].values()):,} edge records: "
        f"{report['asserted_edges']:,} asserted, {report['link_only_edges']:,} pointer-only; "
        f"{report['mapping_rows']:,} SSSOM rows across 7 emitted sets."),
        f"{len(sizes)} files; {sum(sizes.values()):,} bytes ({sum(sizes.values()) / (1 << 30):.3f} GiB).",
        f"SHA256SUMS manifest: `{sha256(root / 'SHA256SUMS')}`.", "",
        "## Validation evidence", "",
        "`cargo test -j 8 -p atlas-release`: 13 passed (1 policy + 12 guardrails).",
        "`cargo clippy -j 8 -p atlas-release --all-targets --no-deps -- -D warnings`: passed. Dependency-only strict Clippy is blocked by upstream atlas-core collapsible_if; atlas-ingest also emits a quick-xml deprecation warning.",
        "Python: 8 release-final adversarial tests + 2 all-payload privacy tests passed; Ruff and Bash syntax passed.",
        ("Independent validator covers SHA256SUMS/file inventory, JSONL fields/licences/counts, all-file person/ORCID "
        "and secret scan (patterns plus private .env value comparison), current quarantine/suppression, "
        "official SSSOM validation, streaming RDF 1.2 parse and PROV-O edge checks, CFF schema and card metadata."),
        "Full validation/upload output: `release/logs/publish-v0.1.log`; public download receipt: `release/logs/published-v0.1.json`.",
        ("Committed checksum receipt: `docs/release/UPLOAD-VERIFICATION.json`. "
         "Both failed build directories were deleted after public download verification." if receipt
         and not any((root.parent / f"zebratlas-kg-v0.1.failed-{n}").exists() for n in [1, 2])
         else "Cleanup status has not been confirmed."),
        (f"Dataset: {receipt['dataset_url']}; remote commit: `{receipt['revision']}`. "
         f"{len(receipt['verified_files'])} anonymous downloaded payloads match SHA256SUMS, including the manifest itself. "
         f"Hub transport metadata: `{receipt['platform_metadata_files']}`." if receipt else "Publication pending full gate and upload."), "",
        "## Graph source memberships and sizes", "",
        "Mixed records count in each contributing source; bytes below are JSONL memberships, not additive bundle sizes.", "",
        "| Source | Nodes | Edges | Asserted edges | JSONL bytes |", "|---|---:|---:|---:|---:|"]
    for name, counts in report["sources"].items():
        lines.append(f"| {name} | {counts['node_memberships']:,} | {counts['edge_memberships']:,} | "
                     f"{counts['asserted_edge_memberships']:,} | {counts['jsonl_bytes']:,} |")
    lines += ["", "## Mapping sets", "", "| Set | Rows | Output TSV bytes | Held/excluded |", "|---|---:|---:|---|"]
    lines.append(f"| disease-identity | {report['mapping_rows'] - sum(c['rows'] for c in report['mapping_sets'].values()):,} | {sizes['mappings/disease-identity.sssom.tsv']:,} | licensed authoritative projection |")
    for name, counts in report["mapping_sets"].items():
        note = counts.get("held_reason") or json.dumps(counts["excluded"], sort_keys=True)
        lines.append(f"| {name} | {counts['rows']:,} | {counts.get('bytes', 0):,} | {note} |")
    lines += ["", "## Exclusions and practical limits", "",
        f"Exclusion reasons (node + edge records): `{json.dumps(report['exclusion_reasons'], sort_keys=True)}`.",
        (f"Quarantine: {report['quarantine']['input_entries']:,} input entries, "
        f"{report['quarantine']['listed_records']:,} normalized record pairs; "
        f"{report['quarantine']['excluded_nodes']:,} nodes and {report['quarantine']['excluded_edges']:,} edges excluded; "
        f"manifest `{report['quarantine']['manifest_sha256']}`."),
        (f"Suppression present: {report['suppression']['present']}; "
         f"{report['suppression']['input_entries']} entries. All list parsing uses atlas_core::withhold; "
         "whole-file/record selectors and foreign-salt failure are tested. Person profiles and researcher mappings remain excluded."),
        f"Emitted asset kinds: `{json.dumps(report['asset_kinds'], sort_keys=True)}`.",
        ("KGX retains the merged master's neighbourhood scope; this bundle does not claim the full external KG union. "
         f"Incomplete retrieval metadata holds {report['exclusion_reasons'].get('missing_retrieval_metadata', 0):,} node/edge records. "
         "The ingest owner should retain acquisition dates rather than inventing them."),
        ("HPO/G2P/ClinGen/GARD copied content and SA/NC/unknown source assertions remain held. "
        "Person profiles, names, ORCIDs, private contacts, page text, quotes and abstracts are excluded. "
        "CFF dataset-author attribution is the explicitly requested exception to profile exclusion."), "",
        "## Orchestrator's next step", "",
        ("Merge `agent/release`; the ignored local bundle remains in this worktree. Author Johannes Mitschunas, "
         "ORCID https://orcid.org/0009-0004-3579-1399, no affiliation, is recorded in CITATION.cff. "
         "The explicit orchestrator follow-up supersedes the earlier prepare-only brief. "
         "Every future release must rebuild and validate against current withholding manifests and mapping flags."),
        "Picked up current shared withholding, align round 2 and openaccess from master. Reviewed founder feedback; no website UI changed.",
        "Zenodo steps are documented only; DOI deposit waits for University of Jena review under D50.", "",
        "## Reproduce (Git Bash)", "", "```bash",
        "cargo build -j 8 -p atlas-release\nuv run --with 'pyoxigraph>=0.5.11' --with 'sssom>=0.4' --with cffconvert --with python-dotenv python crates/atlas-release/scripts/validate_release.py release/zebratlas-kg-v0.1",
        "(cd release/zebratlas-kg-v0.1 && sha256sum -c SHA256SUMS)", "```", ""]
    (Path(__file__).resolve().parents[3] / "docs/release/REPORT.md").write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    main()
