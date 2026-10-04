"""Stage matched PRIVATE serving data without modifying original evidence artifacts."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import tomllib
from pathlib import Path, PurePosixPath

from datamanifest import DATA_FILES, build, data_id, selected, sha256

GATE_FILES = {"identity-gate.manifest.json", "identity-decisions.jsonl",
              "identity-invalidations.jsonl", "rules.json", "review-diff.tsv"}


def regular(path: Path) -> Path:
    if path.is_symlink() or not path.is_file():
        raise ValueError("serving input must be a regular non-symlink file")
    return path


def pinned(path: Path, expected: str):
    if not re.fullmatch(r"[0-9a-f]{64}", expected or "") or sha256(regular(path)) != expected:
        raise ValueError("serving input hash mismatch")


def snapshot_signature(path: Path, magic: bytes, version: int):
    with path.open("rb") as stream:
        if stream.read(8) != magic or struct.unpack("<I", stream.read(4))[0] != version:
            raise ValueError("snapshot format does not match the selected consumer")
        length = struct.unpack("<Q", stream.read(8))[0]
        if length > 1024 * 1024:
            raise ValueError("snapshot signature exceeds the allowed size")
        return stream.read(length).decode("utf-8")


def validate_gate(directory: Path, consumer: Path):
    path = regular(directory / "identity-gate.manifest.json")
    gate_bytes = path.read_bytes()
    gate_hash = hashlib.sha256(gate_bytes).hexdigest()
    gate = json.loads(gate_bytes)
    policy = (consumer / "crates/atlas-core/src/identity_policy.rs").read_text(encoding="utf-8").replace("\r\n", "\n")
    policy_hash = hashlib.sha256(policy.encode()).hexdigest()
    if (gate.get("schema") != "atlas.identity.gate" or gate.get("version") != 1
            or gate.get("identity_policy_sha256") != policy_hash
            or gate.get("rule_id") != "R-ID-03" or gate.get("rule_version") != "1.0.0"):
        raise ValueError("identity gate does not match the selected consumer policy")
    registry = consumer / "data/cache/mappings/rules.json"
    if regular(directory / "rules.json").read_bytes() != registry.read_bytes():
        raise ValueError("identity gate registry differs from the selected consumer")
    inventory = set()
    for row in gate.get("outputs", []):
        name = row.get("file", "")
        if (not name or Path(name).name != name or any(c in name for c in "/\\:")
                or name in inventory or (name not in GATE_FILES and not name.endswith(".sssom.tsv"))):
            raise ValueError("unsafe or unsupported identity gate output")
        inventory.add(name)
        pinned(directory / name, row.get("sha256"))
    if not (GATE_FILES - {"identity-gate.manifest.json"}).issubset(inventory):
        raise ValueError("identity gate is incomplete")
    if {p.name for p in directory.glob("*.sssom.tsv")} - inventory:
        raise ValueError("identity directory contains unlisted mapping inputs")
    for source in gate["prov:wasGeneratedBy"]["prov:used"]:
        pinned(Path(source["path"]), source["sha256"])
    if sha256(path) != gate_hash:
        raise ValueError("identity completion manifest changed during validation")
    hashes = {row["file"]: row["sha256"] for row in gate["outputs"]}
    hashes["identity-gate.manifest.json"] = gate_hash
    return gate_hash, hashes, policy_hash


def inventory(data: Path, mappings: Path, operational: Path, schema: Path, consumer: Path, build_receipt: Path):
    gate_hash, gate_names, policy_hash = validate_gate(mappings, consumer)
    manifest_bytes = regular(operational).read_bytes()
    manifest_hash = hashlib.sha256(manifest_bytes).hexdigest()
    manifest = json.loads(manifest_bytes)
    if (manifest.get("format") != "zebratlas-operational-rdf-v1"
            or manifest.get("public_release") is not False
            or manifest.get("runtime_reasoning") is not False
            or manifest.get("identity_gate_sha256") != gate_hash):
        raise ValueError("private operational manifest does not match the accepted identity gate")
    schema_bytes = regular(schema).read_bytes()
    schema_hash = hashlib.sha256(schema_bytes).hexdigest()
    card = json.loads(schema_bytes)
    if card.get("graph", {}).get("sha256") != manifest["rdf_sha256"]:
        raise ValueError("query schema describes another RDF projection")
    files = {}
    frozen_hashes = {}
    for name in DATA_FILES:
        if (data / name).exists():
            files[name] = regular(data / name)
    for directory in (data / "raw", data / "cache"):
        for top, dirs, names in os.walk(directory, followlinks=False):
            dirs[:] = [n for n in dirs if not (Path(top) / n).is_symlink()]
            for name in names:
                source = Path(top) / name
                rel = PurePosixPath(source.relative_to(data).as_posix())
                if selected(rel) and rel.parts[:2] not in (("cache", "mappings"), ("cache", "runtime")):
                    files[str(rel)] = regular(source)
    for name in gate_names:
        files[f"cache/mappings/{name}"] = mappings / name
        frozen_hashes[f"cache/mappings/{name}"] = gate_names[name]
    for key, filename, hash_key in (("atlas_snapshot", "atlas.snapshot", "atlas_sha256"),
                                    ("graph_snapshot", "graph.snapshot", "graph_sha256"),
                                    ("rdf", "operational.ttl", "rdf_sha256")):
        source = Path(manifest[key])
        pinned(source, manifest[hash_key])
        files[f"cache/runtime/{filename}"] = source
        frozen_hashes[f"cache/runtime/{filename}"] = manifest[hash_key]
    snapshot_code = (consumer / "crates/atlas-core/src/snapshot.rs").read_text(encoding="utf-8")
    for key, magic, version_key in (("atlas_snapshot", b"ATLASNAP", "FORMAT"), ("graph_snapshot", b"ATLGRAPH", "GRAPH_FORMAT")):
        expected_version = int(re.search(r"pub const " + version_key + r": u32 = (\d+);", snapshot_code)[1])
        signature = snapshot_signature(Path(manifest[key]), magic, expected_version)
        if gate_hash not in signature:
            raise ValueError("snapshot belongs to another accepted identity projection")
        if key == "atlas_snapshot" and policy_hash not in signature:
            raise ValueError("native snapshot belongs to another identity policy")
        if key == "graph_snapshot":
            graph_source = (consumer / "crates/atlas-ingest/src/graph/mod.rs").read_text(encoding="utf-8")
            rules = re.search(r"pub const GRAPH_RULES: u32 = (\d+);", graph_source)[1]
            if f"graph-rules:{rules};" not in signature:
                raise ValueError("graph snapshot belongs to another graph-rule version")
            for withholding in (data / "suppression.json", data / "cache/quarantine.json"):
                if withholding.exists() and sha256(withholding) not in signature:
                    raise ValueError("graph snapshot differs from current withholding inputs")
    files["cache/runtime/operational.source.manifest.json"] = operational
    files["cache/runtime/schema.json"] = schema
    frozen_hashes["cache/runtime/operational.source.manifest.json"] = manifest_hash
    frozen_hashes["cache/runtime/schema.json"] = schema_hash
    receipt_bytes = regular(build_receipt).read_bytes()
    receipt = json.loads(receipt_bytes)
    if (receipt.get("format") != "zebratlas-runtime-build-v1" or receipt.get("public_release") is not False
            or receipt.get("identity_gate_sha256") != gate_hash
            or receipt.get("consumer_registry_sha256") != sha256(consumer / "data/cache/mappings/rules.json")):
        raise ValueError("missing owner-matched native rebuild receipt")
    for key, expected in (("atlas_snapshot", manifest["atlas_sha256"]), ("graph_snapshot", manifest["graph_sha256"]),
                          ("rdf", manifest["rdf_sha256"]), ("schema", schema_hash)):
        if receipt.get("artifacts", {}).get(key, {}).get("sha256") != expected:
            raise ValueError("native rebuild receipt describes another serving artifact")
    mechanism = receipt.get("artifacts", {}).get("mechanism_snapshot", {})
    mechanism_path = Path(mechanism["path"])
    pinned(mechanism_path, mechanism.get("sha256"))
    mechanism_source = (consumer / "crates/atlas-ingest/src/mechanism/mod.rs").read_text(encoding="utf-8")
    mechanism_version = int(re.search(r"pub const FORMAT: u32 = (\d+);", mechanism_source)[1])
    mechanism_signature = snapshot_signature(mechanism_path, b"MECHSNAP", mechanism_version)
    if gate_hash not in mechanism_signature or policy_hash not in mechanism_signature:
        raise ValueError("mechanism snapshot does not match the accepted identity policy")
    files["cache/runtime/mechanism.snapshot"] = mechanism_path
    frozen_hashes["cache/runtime/mechanism.snapshot"] = mechanism["sha256"]
    required_sources = {p.relative_to(consumer).as_posix() for crate in ("atlas-core", "atlas-ingest")
                        for p in (consumer / f"crates/{crate}/src").rglob("*.rs")}
    required_sources.update(p.relative_to(consumer).as_posix() for crate in ("atlas-core", "atlas-ingest")
                            for p in (consumer / f"crates/{crate}").glob("build.rs"))
    required_sources.add("data/cache/mappings/rules.json")
    required_sources.update(("Cargo.toml", "Cargo.lock", "crates/atlas-core/Cargo.toml", "crates/atlas-ingest/Cargo.toml"))
    source_records = {r["file"]: r["sha256"] for r in receipt.get("consumer_sources", [])}
    workspace_record = receipt["workspace_manifest"]
    original_workspace = Path(workspace_record["path"])
    pinned(original_workspace, workspace_record["sha256"])
    if source_records.get("Cargo.toml") != workspace_record["sha256"]:
        raise ValueError("original workspace configuration is not bound to the native build")
    original_tables = tomllib.loads(original_workspace.read_text(encoding="utf-8"))
    package_tables = tomllib.loads((consumer / "Cargo.toml").read_text(encoding="utf-8"))
    original_tables["workspace"].pop("members", None)
    package_tables["workspace"].pop("members", None)
    if package_tables != original_tables:
        raise ValueError("submission changed native workspace semantics beyond member pruning")
    serde_json = package_tables["workspace"]["dependencies"].get("serde_json", {})
    if not isinstance(serde_json, dict) or "float_roundtrip" not in serde_json.get("features", []):
        raise ValueError("compiled consumer must retain canonical source numeric parsing")
    if not required_sources.issubset(source_records):
        raise ValueError("native receipt omits consumer build source hashes")
    for name in required_sources:
        pinned(consumer / name, source_records[name])
    inputs = {r["file"]: r["sha256"] for r in receipt.get("inputs", [])}
    for rel, source in files.items():
        if not rel.startswith("cache/runtime/"):
            if rel not in inputs:
                raise ValueError("native receipt omits a private serving input")
            if rel in frozen_hashes and frozen_hashes[rel] != inputs[rel]:
                raise ValueError("native receipt describes a different identity gate")
            frozen_hashes[rel] = inputs[rel]
        pinned(source, frozen_hashes[rel])
    files["cache/runtime/native-build.receipt.json"] = build_receipt
    frozen_hashes["cache/runtime/native-build.receipt.json"] = hashlib.sha256(receipt_bytes).hexdigest()
    return files, manifest, gate_hash, frozen_hashes


def stage(data: Path, mappings: Path, operational: Path, schema: Path, consumer: Path, build_receipt: Path, destination: Path):
    if destination.exists():
        raise ValueError("staging destination must be new; existing data will never be overwritten")
    if destination.is_relative_to(data) or data.is_relative_to(destination):
        raise ValueError("staging data must be separate from original source data")
    if destination.is_relative_to(consumer):
        raise ValueError("private runtime bundle must stay outside the public source checkout")
    files, manifest, gate_hash, frozen_hashes = inventory(data, mappings, operational, schema, consumer, build_receipt)
    destination.mkdir(parents=True, exist_ok=False)
    for rel, source in sorted(files.items()):
        target = destination / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        before = frozen_hashes[rel]
        pinned(source, before)
        shutil.copy2(source, target)
        if sha256(target) != before or sha256(source) != before:
            raise ValueError("serving input changed during immutable staging")
    rebased = dict(manifest)
    rebased.update(atlas_snapshot="/data/cache/runtime/atlas.snapshot", graph_snapshot="/data/cache/runtime/graph.snapshot",
                   mechanism_snapshot="/data/cache/runtime/mechanism.snapshot",
                   mechanism_sha256=frozen_hashes["cache/runtime/mechanism.snapshot"],
                   rdf="/data/cache/runtime/operational.ttl", mapping_directory="/data/cache/mappings")
    rebased["prov:wasDerivedFrom"] = {"sha256": frozen_hashes["cache/runtime/operational.source.manifest.json"], "artifact": "operational.source.manifest.json"}
    runtime = destination / "cache/runtime"
    (runtime / "operational.manifest.json").write_text(json.dumps(rebased, indent=2) + "\n", encoding="utf-8")
    receipt = {"format": "zebratlas-private-runtime-bundle-v1", "public_release": False,
               "identity_gate_sha256": gate_hash, "rdf_sha256": manifest["rdf_sha256"],
               "operational_manifest_sha256": sha256(runtime / "operational.manifest.json"),
               "source_operational_manifest_sha256": frozen_hashes["cache/runtime/operational.source.manifest.json"],
               "query_schema_sha256": sha256(runtime / "schema.json"),
               "consumer_registry_sha256": sha256(consumer / "data/cache/mappings/rules.json"),
               "workspace_derivation": {"source_sha256": json.loads(build_receipt.read_bytes())["workspace_manifest"]["sha256"],
                                        "package_sha256": sha256(consumer / "Cargo.toml"), "allowed_difference": "none; current package retains exact workspace bytes"},
               "files": [{"file": rel, "sha256": sha256(destination / rel)} for rel in sorted(files)],
               "runtime_reasoning": False, "reasoning_note": "Requires a fresh certified Linux store and proof receipt before activation"}
    (runtime / "runtime.manifest.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    manifest_bytes = build(destination).encode()
    (destination / "MANIFEST.tsv").write_bytes(manifest_bytes)
    return data_id(manifest_bytes), receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("data", "mapping-dir", "operational-manifest", "schema", "consumer", "build-receipt"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--stage", type=Path, help="New private staging directory; omit for read-only validation")
    args = parser.parse_args()
    parameters = [p.resolve() for p in (args.data, args.mapping_dir, args.operational_manifest, args.schema, args.consumer, args.build_receipt)]
    if args.stage:
        identifier, receipt = stage(*parameters, args.stage.resolve())
        print(json.dumps({"data_id": identifier, "private": True, "files": len(receipt["files"])}))
    else:
        files, _, pin, _ = inventory(*parameters)
        print(json.dumps({"validated": True, "private": True, "files": len(files), "bytes": sum(p.stat().st_size for p in files.values()), "identity_gate_sha256": pin}))


if __name__ == "__main__":
    main()
