"""Project reviewed mapping caches into a local Rust release and seal its inventory.

Never writes shared data. Run once after export; --seal is only for reviewed metadata edits.
"""
from __future__ import annotations

import argparse
import collections
import csv
import json
import subprocess
from datetime import UTC, datetime
from pathlib import Path

import yaml
from release_gate import HELD_SETS, MAIN, MAPPING_POLICIES, Gate, public_url, require, sha256

REPO = Path(__file__).resolve().parents[3]
COLUMNS = ["subject_id", "predicate_id", "object_id", "mapping_justification", "mapping_date", "mapping_tool",
           "confidence", "mapping_cardinality", "asserted_predicate_id", "conflict", "comment"]


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def metadata(path):
    lines = []
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            if not line.startswith("#"):
                break
            lines.append(line[2:] if line.startswith("# ") else line[1:])
    value = yaml.safe_load("".join(lines))
    require(isinstance(value, dict), "missing SSSOM metadata")
    return value


def manifest_for(path):
    for candidate in [path.with_name(path.name.replace(".sssom.tsv", ".manifest.json")),
                      path.with_name(path.name.replace(".tsv", ".manifest.json"))]:
        if candidate.exists():
            return candidate, json.loads(candidate.read_text(encoding="utf-8"))
    return None, {}


def project_mappings(root, data, gate):
    results = {}
    total = 0
    for path in sorted((data / "cache/mappings").glob("*.sssom.tsv")):
        name = path.name.removesuffix(".sssom.tsv")
        source_hash = sha256(path)
        result = {"source_file": "cache/mappings/" + path.name, "source_sha256": source_hash,
                  "source_bytes": path.stat().st_size, "rows": 0, "input_rows": 0, "excluded": {},
                  "license": "unverified; held artifact"}
        results[name] = result
        if name not in MAPPING_POLICIES:
            result["held_reason"] = HELD_SETS.get(name, "No reviewed artifact-level release policy, or release false")
            continue
        meta = metadata(path)
        _manifest_path, manifest = manifest_for(path)
        result["license"] = meta.get("license", "unverified")
        if any(str(value.get("release", "")).lower() == "false" for value in [meta, manifest]):
            result["held_reason"] = "release false"
            continue
        licence, basis = MAPPING_POLICIES[name]
        require(meta.get("license") == licence, f"mapping licence changed: {name}")
        result["policy_basis"] = basis
        generated = manifest.get("prov:wasGeneratedBy", {})
        inputs = generated.get("prov:used", [])
        by_hash = {i.get("sha256"): i for i in inputs if isinstance(i, dict)}
        kept_inputs = {}
        cache_files = {}
        output_meta = {"mapping_set_id": meta["mapping_set_id"], "mapping_set_version": "zebratlas-kg-v0.1",
                       "mapping_set_description": "Identifier-only release projection; source predicates and conflict flags retained; no labels or quoted evidence.",
                       "license": licence, "mapping_date": "2026-10-04", "curie_map": meta["curie_map"]}
        output = root / "mappings" / path.name
        with path.open(encoding="utf-8") as source:
            metadata_lines = 0
            for line in source:
                if not line.startswith("#"):
                    break
                metadata_lines += 1
        exclusions = collections.Counter()
        with output.open("w", encoding="utf-8", newline="") as stream:
            for line in yaml.safe_dump(output_meta, sort_keys=False, allow_unicode=True).splitlines():
                stream.write("# " + line + "\n")
            writer = csv.DictWriter(stream, fieldnames=COLUMNS, delimiter="\t", lineterminator="\n")
            writer.writeheader()
            with path.open(encoding="utf-8", newline="") as source:
                reader = csv.DictReader((l for l in source if not l.startswith("#")), delimiter="\t")
                for index, row in enumerate(reader):
                    result["input_rows"] += 1
                    require(None not in row, f"malformed mapping row: {name}")
                    if str(row.get("release", "")).lower() == "false":
                        exclusions["release_false"] += 1
                        continue
                    if row.get("license_class") and row["license_class"].lower() != "open":
                        exclusions["row_licence_class_held"] += 1
                        continue
                    ids = (row["subject_id"], row["object_id"])
                    if any(i.upper().startswith(("ORCID:", "PERSON:", "REPORTER.PI:", "HP:", "GARD:", "CLINGEN:", "G2P:")) for i in ids):
                        exclusions["person_or_held_namespace"] += 1
                        continue
                    evidence_hash = row.get("evidence_sha256", row.get("prov_sha256", ""))
                    evidence_input = by_hash.get(evidence_hash, {})
                    if name == "funder-ror" and not (ids[0].startswith("ROR:") and ids[1].startswith("crossref.funder:")
                                                      and "ROR data dump" in evidence_input.get("role", "")):
                        exclusions["non_ror_input_permission_unverified"] += 1
                        continue
                    require(len(evidence_hash) == 64, f"mapping missing evidence hash: {name}")
                    evidence_url = public_url(row.get("evidence_url", row.get("prov_source_url", "")))
                    evidence_locator = row.get("evidence_locator", row.get("prov_locator", ""))
                    require(evidence_locator, f"mapping missing evidence locator: {name}")
                    cache_file = cache_files.get(evidence_hash, "")
                    if evidence_input.get("path") and evidence_hash not in kept_inputs:
                        input_path = Path(evidence_input["path"]).resolve()
                        require(input_path.is_relative_to(data), "mapping input outside shared data")
                        relative = input_path.relative_to(data).as_posix()
                        if relative.startswith("cache/"):
                            cache_file = relative
                        cache_files[evidence_hash] = cache_file
                        require(input_path.exists() and sha256(input_path) == evidence_hash, f"mapping evidence bytes changed: {name}")
                        kept_inputs[evidence_hash] = {"source_url": public_url(evidence_input["url"]),
                            "version": evidence_input.get("version") or "snapshot-sha256:" + evidence_hash,
                            "sha256": evidence_hash, "record_locator": "file", "hash_scope": "source-file bytes",
                            "retrieved_at": datetime.fromtimestamp(input_path.stat().st_mtime, UTC).isoformat(),
                            "retrieval_basis": "source-file mtime proxy, not authenticated acquisition"}
                    retrieved = row.get("prov_retrieved_at") or (kept_inputs.get(evidence_hash, {}).get("retrieved_at"))
                    require(retrieved, f"mapping missing retrieval metadata: {name}")
                    ref = {"source_url": evidence_url, "version": row.get("prov_version") or evidence_input.get("version") or "snapshot-sha256:" + evidence_hash,
                           "retrieved_at": retrieved, "retrieval_basis": "record acquisition" if row.get("prov_retrieved_at") else "source-file mtime proxy",
                           "sha256": evidence_hash, "record_locator": evidence_locator, "cache_file": cache_file,
                           "mapping_file": result["source_file"], "mapping_sha256": source_hash,
                           "mapping_record_locator": f"data row {index + 1}", "license": licence}
                    mapping_ref = {"cache_file": result["source_file"], "record_locator": f"L{metadata_lines + reader.line_num}", "source_url": evidence_url}
                    if gate.blocked(ids, (ref, mapping_ref)):
                        exclusions["quarantine_or_suppression"] += 1
                        continue
                    projected = {k: row.get(k, "") for k in COLUMNS}
                    projected["comment"] = json.dumps([ref], separators=(",", ":"), ensure_ascii=False)
                    projected["mapping_date"] = row.get("mapping_date") or "2026-10-04"
                    projected["mapping_tool"] = row.get("mapping_tool") or "atlas-release identifier projection"
                    writer.writerow(projected)
                    result["rows"] += 1
        result["excluded"] = dict(exclusions)
        result["bytes"] = output.stat().st_size
        # Public manifests omit raw labels, comments, absolute paths and unrelated restricted inputs.
        write_json(output.with_suffix(".manifest.json"), {
            "@context": {"prov": "http://www.w3.org/ns/prov#"}, "mapping_set_id": meta["mapping_set_id"],
            "license": licence, "rows": result["rows"], "excluded": result["excluded"],
            "prov:wasGeneratedBy": {"@type": "prov:Activity", "prov:endedAtTime": datetime.now(UTC).isoformat(),
                "prov:wasAssociatedWith": {"@type": "prov:SoftwareAgent", "name": "atlas-release-final", "version": "0.1"},
                "prov:used": list(kept_inputs.values()), "source_mapping_sha256": source_hash,
                "source_mapping_version": str(meta.get("mapping_set_version", "snapshot-sha256:" + source_hash)),
                "parameters": {"labels": "withheld", "comments": "replaced by exact provenance", "conflicts": "retained", "policy": basis}}
        })
        total += result["rows"]
    return results, total


def seal(root):
    require(root.is_dir(), "release directory missing")
    files = sorted(p for p in root.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
    require(not any(p.is_symlink() for p in root.rglob("*")), "symlink in release")
    report_path = root / "release-report.json"
    report = json.loads(report_path.read_text(encoding="utf-8"))
    report["files"] = {p.relative_to(root).as_posix(): p.stat().st_size for p in files if p != report_path}
    write_json(report_path, report)
    (root / "SHA256SUMS").write_text("".join(f"{sha256(p)}  {p.relative_to(root).as_posix()}\n" for p in files), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("release", type=Path)
    parser.add_argument("--data", type=Path, default=MAIN / "data")
    parser.add_argument("--seal", action="store_true")
    args = parser.parse_args()
    root, data = args.release.resolve(), args.data.resolve()
    gate = Gate(data)
    report_path = root / "release-report.json"
    report = json.loads(report_path.read_text(encoding="utf-8"))
    gate.check_snapshot(report)
    if args.seal:
        seal(root)
        return
    require("mapping_sets" not in report, "already finalized; only --seal for metadata edits")
    sets, rows = project_mappings(root, data, gate)
    report["mapping_sets"] = sets
    report["mapping_rows"] += rows
    write_json(report_path, report)
    write_json(root / "build-info.json", {
        "master_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "observed_master_at_finalization": subprocess.check_output(["git", "rev-parse", "master"], cwd=REPO, text=True).strip(),
        "build_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "release_code": {p.relative_to(REPO).as_posix(): sha256(p) for p in sorted((REPO / "crates/atlas-release").rglob("*"))
                         if p.is_file() and p.suffix in {".rs", ".py", ".toml"}},
        "source_scope": "Current master atlas-ingest graph readers; KGX two-hop scope, not full external unions",
        "publication": "Public Hugging Face distribution approved; exact remote commit and download verification are recorded in the release handoff"
    })
    card = (REPO / "docs/release/DATASET-CARD.md").read_text(encoding="utf-8")
    card = card.replace("{NODE_COUNT}", str(sum(report["nodes"].values()))).replace("{EDGE_COUNT}", str(sum(report["edges"].values())))
    (root / "README.md").write_text(card, encoding="utf-8")
    citation = yaml.safe_load((REPO / "docs/release/CITATION.cff").read_text(encoding="utf-8"))
    citation["version"] = report["version"]
    (root / "CITATION.cff").write_text(yaml.safe_dump(citation, sort_keys=False, allow_unicode=True), encoding="utf-8")
    # Rust embeds the same datasheet; refresh after adding the reviewed mapping projection.
    (root / "datasheet.md").write_text((REPO / "docs/release/DATASHEET.md").read_text(encoding="utf-8"), encoding="utf-8")
    (root / "LICENCES.md").write_text((REPO / "docs/release/LICENCES.md").read_text(encoding="utf-8"), encoding="utf-8")
    seal(root)
    print(json.dumps({"mapping_rows": report["mapping_rows"], "mapping_sets": sets}, indent=2))


if __name__ == "__main__":
    main()
