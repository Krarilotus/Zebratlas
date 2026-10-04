"""Fail-closed release checks: actual RDF 1.2/SSSOM parsers, policy, counts, checksums.

Run with uv run --with 'pyoxigraph>=0.5.11' --with 'sssom>=0.4' python ...
No requests are made unless the optional same-snapshot integrity URL is supplied.
"""
from __future__ import annotations

import argparse
import collections
import csv
import hashlib
import inspect
import json
import re
import sys
import urllib.request
from datetime import UTC
from pathlib import Path

from release_gate import MAIN, MAPPING_POLICIES, Gate, check_cff, check_secrets

BASE = "https://w3id.org/rare-disease-atlas/"
DCT = "http://purl.org/dc/terms/"
PROV = "http://www.w3.org/ns/prov#"
RA = BASE + "vocab#"
COPY_SOURCES = {"mondo", "orphadata", "hgnc", "go", "reactome", "wikidata",
                "cellosaurus", "impc", "opentargets", "openalex", "ror", "crossref-funders"}
NODE_FIELDS = {"id", "kind", "export_status", "label", "status", "provenance"}
EDGE_FIELDS = {"id", "export_status", "from", "relation", "to", "kind", "activities", "provenance"}
REF_FIELDS = {
    "source", "source_url", "source_url_basis", "record_url", "retrieved_at", "retrieval_basis", "version",
    "sha256", "hash_scope", "source_sha256", "source_hash_scope", "record_locator", "source_entity", "license", "copy_fields",
}
PERSONAL_REFERENCE = re.compile(
    rb"(?:orcid(?::|%3a)|orcid\.org/|person(?::|%3a)|reporter\.pi(?::|%3a)|cache(?:/|%2f)people(?:/|%2f)|"
    rb"\b\d{4}-\d{4}-\d{4}-\d{3}[\dX]\b)", re.IGNORECASE
)
PERSONAL_FIELD = re.compile(
    rb'"(?:pi(?:s|_name)?|principal_investigator(?:s|_name)?|investigator(?:s|_name)?|'
    rb'contact(?:s|_name)?|central_contacts?|officials?|author(?:s|_name)?|staff(?:_name)?|'
    rb'person_name|full_name|first_name|last_name|name_variants|affiliations|orcids?)"\s*:', re.IGNORECASE
)
PERSONAL_RDF_FIELD = re.compile(rb'ra:(?:piName|contactName|authorName|staffName|principalInvestigator|orcid)\b', re.IGNORECASE)
ORCID_SUFFIX = re.compile(rb"-\d{4}-\d{4}-\d{3}[\dX]\b", re.IGNORECASE)
PERSONAL_MARKERS = (b"orcid:", b"orcid%3a", b"orcid.org/", b"person:", b"person%3a", b"reporter.pi:",
                    b"reporter.pi%3a", b"cache/people/", b"cache%2fpeople%2f")
NAME_FIELD_HINTS = (b'"pi', b'"principal', b'"investigator', b'"contact', b'"central', b'"official',
                    b'"author', b'"staff', b'"person_name', b'"full_name', b'"first_name', b'"last_name',
                    b'"name_variants', b'"affiliations', b'"orcid')


def orcid_number(data):
    # Leading literal '-' lets the regex engine skip hashes and other digit runs
    # in C. This has the same boundaries as the original ORCID number pattern.
    for match in ORCID_SUFFIX.finditer(data):
        start = match.start() - 4
        if start >= 0 and data[start:match.start()].isdigit():
            before = data[start - 1] if start else None
            if before is None or not (48 <= before <= 57 or 65 <= before <= 90 or 97 <= before <= 122 or before == 95):
                return True
    return False


def check_privacy(path: Path) -> None:
    if path.name == "CITATION.cff":
        # CFF authors are dataset credit explicitly requested by the founder, not
        # graph profiles. The CFF schema and placeholder gate validate them separately.
        import yaml
        value = yaml.safe_load(path.read_text(encoding="utf-8"))
        require(set(value) <= {"cff-version", "message", "type", "title", "version", "date-released",
                              "license", "repository-code", "authors", "abstract"}, "unexpected CFF content")
        value.pop("authors", None)
        data = json.dumps(value, default=str).encode()
        require(not PERSONAL_REFERENCE.search(data), "PERSON PROFILE IN CFF BODY")
        return
    # Every file, including RDF/SSSOM/provenance/support files. Overlap catches chunk boundaries.
    with path.open("rb") as f:
        tail = b""
        for chunk in iter(lambda: f.read(1 << 20), b""):
            data = tail + chunk
            lower = data.lower()
            require(not any(marker in lower for marker in PERSONAL_MARKERS)
                    and not orcid_number(data), f"PERSON ID/PROFILE LEAK: {path.name}")
            if any(hint in lower for hint in NAME_FIELD_HINTS):
                require(not PERSONAL_FIELD.search(data), f"PERSON NAME FIELD LEAK: {path.name}")
            if any(hint in lower for hint in (b"piname", b"contactname", b"authorname", b"staffname", b"principalinvestigator", b"orcid")):
                require(not PERSONAL_RDF_FIELD.search(data), f"PERSON RDF FIELD LEAK: {path.name}")
            if b"person" in lower:
                require(not re.search(rb'ra:nodeKind\s+"person"|"kind"\s*:\s*"person"', data), f"PERSON NODE LEAK: {path.name}")
            tail = data[-1024:]


def require(ok: bool, message: str) -> None:
    if not ok:
        raise ValueError(message)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_refs(record: dict) -> bool:
    refs = record["provenance"]
    require(bool(refs), f"missing provenance: {record['id']}")
    for ref in refs:
        require(set(ref) <= REF_FIELDS, "unexpected provenance field")
        for name in ["source_url", "retrieved_at", "version", "record_locator", "source_entity", "hash_scope", "source_hash_scope"]:
            require(bool(ref.get(name)), f"missing {name}: {record['id']}")
        for name in ["sha256", "source_sha256"]:
            require(len(ref[name]) == 64 and all(c in "0123456789abcdef" for c in ref[name].lower()), "invalid checksum")
        for name in ["source_url", "record_url"]:
            url = ref.get(name)
            if url:
                require(url.startswith(("https://", "http://")), "non-public source URL")
                require("@" not in url.split("/")[2], "credential-bearing source URL")
                require("?" not in url and "#" not in url, "unfiltered URL query/fragment")
        if ref["copy_fields"]:
            require(ref["source"] in COPY_SOURCES, "unverified source copying")
            require(ref["license"] in {
                "https://creativecommons.org/licenses/by/4.0/",
                "https://creativecommons.org/publicdomain/zero/1.0/",
            }, "non-compatible licence")
    return all(r["copy_fields"] for r in refs)


def check_jsonl(root: Path, report: dict, gate=None) -> tuple[set[str], int, dict]:
    node_ids = set()
    nodes = collections.Counter()
    disease_status = collections.Counter()
    node_link_only = 0
    memberships = collections.defaultdict(lambda: collections.Counter())
    with (root / "nodes.jsonl").open(encoding="utf-8") as f:
        for line in f:
            n = json.loads(line)
            require(set(n) <= NODE_FIELDS, f"unexpected node field: {n['id']}")
            require(n["id"] not in node_ids, "duplicate node ID")
            node_ids.add(n["id"])
            copy = check_refs(n)
            if gate:
                require(not gate.blocked([n["id"]], gate_refs(n["provenance"])), "WITHHELD NODE LINEAGE LEAK")
            require("label" not in n or copy, "RESTRICTED NODE LABEL LEAK")
            require(n["kind"] != "person", "PERSON NODE LEAK")
            require(n["export_status"] == ("projected" if copy else "link_only"), "incorrect node policy")
            nodes[n["kind"]] += 1
            node_link_only += n["export_status"] == "link_only"
            if n["kind"] == "disease":
                disease_status[n["status"]] += 1
            for source in {r["source"] for r in n["provenance"]}:
                memberships[source]["node_memberships"] += 1
                memberships[source]["jsonl_bytes"] += len(line.encode("utf-8"))
    edge_ids = set()
    asserted = 0
    with (root / "edges.jsonl").open(encoding="utf-8") as f:
        for line in f:
            e = json.loads(line)
            require(set(e) <= EDGE_FIELDS, f"unexpected edge field: {e['id']}")
            require(e["id"] not in edge_ids, "duplicate edge ID")
            edge_ids.add(e["id"])
            copy = check_refs(e)
            if gate:
                require(not gate.blocked([e["id"]], gate_refs(e["provenance"])), "WITHHELD EDGE LINEAGE LEAK")
            require(bool(e["activities"]), "missing generating activity")
            if e["export_status"] == "link_only":
                require(not ({"from", "to", "relation", "kind"} & set(e)), "RESTRICTED EDGE ASSERTION LEAK")
            else:
                require(e["export_status"] == "asserted" and copy, "unlicensed asserted edge")
                require(e["from"] in node_ids and e["to"] in node_ids, "dangling asserted edge")
                asserted += 1
            for source in {r["source"] for r in e["provenance"]}:
                memberships[source]["edge_memberships"] += 1
                memberships[source]["asserted_edge_memberships" if e["export_status"] == "asserted" else "link_only_edge_memberships"] += 1
                memberships[source]["jsonl_bytes"] += len(line.encode("utf-8"))
    require(dict(nodes) == report["nodes"], "node counts mismatch")
    require(node_link_only == report["node_link_only"], "withheld node counts mismatch")
    require(len(edge_ids) == sum(report["edges"].values()), "edge count mismatch")
    require(asserted == report["asserted_edges"], "asserted edge count mismatch")
    require(len(edge_ids) - asserted == report["link_only_edges"], "withheld edge count mismatch")
    for source, expected in report["sources"].items():
        for key, value in expected.items():
            require(memberships[source][key] == value, f"source count/size mismatch: {source}.{key}")
    api = report["reconciliation"]["api_nodes"]
    excluded_status = report["excluded_disease_status"]
    for status, key in [("active", "disease"), ("retired", "disease_retired"), ("newly_described", "disease_newly_described")]:
        require(disease_status[status] + excluded_status.get(status, 0) == api[key], f"disease count mismatch: {status}")
    for kind in ["phenotype", "study", "grant", "paper", "person", "organisation", "asset"]:
        require(nodes[kind] + report["excluded_nodes"].get(kind, 0) == api.get(kind, 0), f"node exclusion count mismatch: {kind}")
    return edge_ids, len(node_ids), {"nodes": sum(nodes.values()), "edges": len(edge_ids), "asserted_edges": asserted}


def check_rdf(root: Path, report: dict, node_count: int) -> dict:
    from pyoxigraph import RdfFormat, parse
    # Streaming parser validates the entire document without loading millions of triples into a store.
    flags = {}
    rdf_nodes = 0
    reifiers = 0
    triples = 0
    for triple in parse(path=root / "graph.ttl", format=RdfFormat.TURTLE):
        triples += 1
        s, p = getattr(triple.subject, "value", ""), triple.predicate.value
        if s.startswith(BASE + "id/") and p == DCT + "identifier":
            rdf_nodes += 1
        if s.startswith(BASE + "edge/"):
            bit = {DCT + "source": 1, DCT + "license": 2, PROV + "wasDerivedFrom": 4,
                   PROV + "wasGeneratedBy": 8, RA + "exportStatus": 16}.get(p, 0)
            flags[s] = flags.get(s, 0) | bit
            if p == "http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies":
                reifiers += 1
    require(rdf_nodes == node_count, "RDF/JSON node count mismatch")
    require(len(flags) == report["asserted_edges"] + report["link_only_edges"], "RDF/JSON edge count mismatch")
    require(all(f == 31 for f in flags.values()), "RDF edge missing required licence/source/PROV-O")
    require(reifiers == report["asserted_edges"], "RDF asserted reifier count mismatch")
    return {"rdf_triples": triples, "rdf_reifiers": reifiers, "rdf_parsed": True}


def gate_refs(refs):
    out = []
    for ref in refs:
        ref = dict(ref)
        entity = ref.get("source_entity", "")
        if "cache/" in entity and not ref.get("cache_file"):
            ref["cache_file"] = "cache/" + entity.split("cache/", 1)[1].split("#licence=", 1)[0]
        out.append(ref)
    return out


def check_sssom(root: Path, report: dict, gate=None) -> int:
    from sssom.constants import SchemaValidationType
    from sssom.io import parse_sssom_table
    from sssom.validators import validate
    rows = 0
    for path in sorted((root / "mappings").glob("*.sssom.tsv")):
        set_rows = 0
        # sssom 0.4.17's strict parser inverts its metadata-validity boolean and rejects
        # valid standard metadata. Use the normal parser, then all three official validators
        # (JSON schema, prefix map completeness, strict CURIE format) below.
        msdf = parse_sssom_table(path)
        if "fail_on_error" in inspect.signature(validate).parameters:
            results = validate(msdf, fail_on_error=True)
            require(bool(results), "no SSSOM validation ran")
        else:
            # cffconvert's dependency bounds select sssom 0.4.11. That API
            # discards the schema report, so inspect its errors explicitly.
            from sssom.validators import (
                SCHEMA_YAML,
                MappingSet,
                ReferenceValidator,
                SchemaView,
                to_mapping_set_document,
            )
            schema_report = ReferenceValidator(SchemaView(SCHEMA_YAML)).validate(
                to_mapping_set_document(msdf).mapping_set, MappingSet
            )
            require(not schema_report.errors(), "SSSOM schema validation failed")
            validate(msdf, [SchemaValidationType.PrefixMapCompleteness])
        with path.open(encoding="utf-8") as f:
            reader = csv.DictReader((line for line in f if not line.startswith("#")), delimiter="\t")
            for row in reader:
                for key in ["subject_id", "predicate_id", "object_id", "mapping_justification"]:
                    require(bool(re.fullmatch(r"[A-Za-z][A-Za-z0-9_.-]*:\S+", row[key])), "invalid mapping CURIE format")
                name = path.name.removesuffix(".sssom.tsv")
                if name == "disease-identity":
                    require(row["predicate_id"] == "skos:exactMatch", "unexpected alignment predicate")
                    require(gate is not None, "identity mapping requires the shared accepted-decision gate")
                    gate.check_identity_mapping(row)
                else:
                    require(name in MAPPING_POLICIES, "unreviewed mapping set")
                    require(msdf.metadata.get("license") == MAPPING_POLICIES[name][0], "mapping licence mismatch")
                    require(not ({"subject_label", "object_label", "author_id", "evidence_text"} & set(row)), "MAPPING FIELD LEAK")
                    require(row["predicate_id"] in {"skos:exactMatch", "skos:closeMatch", "skos:broadMatch",
                            "skos:narrowMatch", "skos:relatedMatch", "RO:0002205"}, "unreviewed mapping predicate")
                    require(row["mapping_justification"] in {"semapv:DatabaseCrossReference", "semapv:ManualMappingCuration",
                            "semapv:CompositeMatching"}, "unreviewed mapping justification")
                    if row["predicate_id"] == "skos:exactMatch":
                        require(not row.get("conflict"), "conflicted exact mapping")
                refs = json.loads(row["comment"])
                require(isinstance(refs, list) and refs, "mapping without exact provenance")
                if name != "disease-identity":
                    for ref in refs:
                        for key in ["source_url", "retrieved_at", "version", "sha256", "record_locator", "mapping_sha256", "mapping_record_locator"]:
                            require(ref.get(key), "incomplete mapping provenance")
                        require(re.fullmatch(r"[a-f0-9]{64}", ref["sha256"]), "invalid mapping hash")
                    require(not any(i.upper().startswith(("HP:", "GARD:", "CLINGEN:", "G2P:"))
                                    for i in [row["subject_id"], row["object_id"]]), "held namespace mapping")
                if gate:
                    require(not gate.blocked([row["subject_id"], row["object_id"]], gate_refs(refs)), "WITHHELD MAPPING LEAK")
                rows += 1
                set_rows += 1
        name = path.name.removesuffix(".sssom.tsv")
        if name != "disease-identity":
            expected = report["mapping_sets"][name]
            require(set_rows == expected["rows"] and path.stat().st_size == expected["bytes"], "mapping set count/size mismatch")
    require(rows == report["mapping_rows"], "SSSOM row count mismatch")
    return rows


def check_mapping_inputs(root: Path, report: dict, data: Path) -> None:
    from finalize_release import manifest_for, metadata
    for name, counts in report.get("mapping_sets", {}).items():
        if counts.get("held_reason"):
            require(not (root / "mappings" / f"{name}.sssom.tsv").exists(), "held mapping set leaked")
            continue
        source = data / counts["source_file"]
        require(sha256(source) == counts["source_sha256"], "mapping input changed; rebuild release")
        _path, manifest = manifest_for(source)
        require(not any(str(value.get("release", "")).lower() == "false"
                        for value in [metadata(source), manifest]), "release:false mapping set leaked")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("release", type=Path)
    parser.add_argument("--integrity-url")
    parser.add_argument("--receipt", type=Path, help="Write deployment evidence outside the immutable release after all checks pass")
    parser.add_argument("--data", type=Path, default=MAIN / "data")
    parser.add_argument("--env-file", type=Path, default=MAIN / ".env")
    parser.add_argument("--publishing", action="store_true")
    parser.add_argument("--formats-only", action="store_true",
                        help="Recheck checksums, counts and formats after a completed all-file scan; not allowed for publishing")
    args = parser.parse_args()
    root = args.release.resolve()
    require(not (args.publishing and args.formats_only), "publication must run every gate")
    require(not (args.receipt and args.formats_only), "deployment receipt requires every gate")
    if args.receipt:
        require(not args.receipt.resolve().is_relative_to(root), "validation receipt must be outside the immutable release")
    require(not any(p.is_symlink() for p in root.rglob("*")), "symlink in release")
    report = json.loads((root / "release-report.json").read_text(encoding="utf-8"))
    gate = Gate(args.data)
    gate.check_snapshot(report)
    check_mapping_inputs(root, report, args.data)
    if not args.formats_only:
        check_privacy(root / "SHA256SUMS")
        check_secrets(root / "SHA256SUMS", args.env_file)
    required = {"graph.ttl", "nodes.jsonl", "edges.jsonl", "datasheet.md", "README.md", "CITATION.cff", "LICENSE",
                "LICENCES.md", "source-policies.json", "release-report.json", "integrity.json", "mappings/disease-identity.sssom.tsv"}
    manifest = {}
    for line in (root / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        expected, name = line.split("  ", 1)
        path = (root / name).resolve()
        print(f"Checking checksum and privacy: {name}", flush=True)
        require(path.is_relative_to(root), "checksum path escapes release")
        require(name not in manifest, "duplicate checksum entry")
        require(sha256(path) == expected, f"checksum mismatch: {name}")
        if not args.formats_only:
            check_privacy(path)
            check_secrets(path, args.env_file)
        manifest[name] = expected
    actual = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file() and p.name != "SHA256SUMS"}
    require(set(manifest) == actual and required <= actual, "manifest omitted/unexpected files")
    for name, size in report["files"].items():
        require((root / name).stat().st_size == size, f"reported size mismatch: {name}")
    integrity = json.loads((root / "integrity.json").read_text(encoding="utf-8"))
    require(integrity["passed"] and report["reconciliation"]["passed"], "upstream integrity/reconciliation failed")
    require(integrity["nodes"] == report["reconciliation"]["api_nodes"], "integrity node counts mismatch")
    require(integrity["edges_by_relation"] == report["reconciliation"]["api_edges_by_relation"], "integrity edge counts mismatch")
    for relation, n in integrity["edges_by_relation"].items():
        require(report["edges"].get(relation, 0) + report["excluded_edges"].get(relation, 0) == n, f"API edge count mismatch: {relation}")
    print("Checksums, file inventory and integrity reconciled", flush=True)
    edge_ids, node_count, counts = check_jsonl(root, report, gate)
    del edge_ids  # RDF validates counts; retain no duplicate large ID set during parsing.
    print("JSONL whitelist, exclusion reconciliation and source counts passed", flush=True)
    if not args.formats_only:
        print("All-file privacy and secret scans passed", flush=True)
    sssom_rows = check_sssom(root, report, gate)
    print("SSSOM validation passed", flush=True)
    rdf = check_rdf(root, report, node_count)
    cff = check_cff(root, args.publishing)
    import yaml
    require(yaml.safe_load((root / "CITATION.cff").read_text(encoding="utf-8"))["version"] == report["version"],
            "CFF version does not match release")
    live = False
    if args.integrity_url:
        require(args.integrity_url.startswith(("http://", "https://")), "invalid integrity URL")
        with urllib.request.urlopen(args.integrity_url, timeout=60) as response:
            api = json.load(response)
        for key in ["nodes", "edges_by_relation"]:
            require(api[key] == integrity[key], f"live /api/integrity mismatch: {key}; use the same data snapshot")
        require(api["passed"], "live graph integrity failed")
        live = True
    final_gate = Gate(args.data)
    final_gate.check_snapshot(report)
    require(final_gate.identity["manifest_sha256"] == gate.identity["manifest_sha256"],
            "identity gate changed during validation; rebuild release")
    require(final_gate.executable_sha256 == gate.executable_sha256, "shared gate executable changed during validation")
    check_mapping_inputs(root, report, args.data)
    result = {"passed": True, **counts, **rdf, **cff, "secret_scan_passed": not args.formats_only,
                      "current_inputs_checked": True, "identity_gate_manifest_sha256": gate.identity["manifest_sha256"],
                      "sssom_rows": sssom_rows,
                      "excluded_nodes": sum(report["excluded_nodes"].values()),
                      "excluded_edges": sum(report["excluded_edges"].values()),
                      "quarantine": report["quarantine"],
                      "checksum_files": len(manifest), "live_integrity_compared": live}
    if args.receipt:
        from datetime import datetime
        receipt = {"type": "prov:Activity", "version": "release-validation-v1",
                   "source_url": "https://github.com/Krarilotus/rare-disease-atlas",
                   "record_locator": "crates/atlas-release/scripts/validate_release.py",
                   "retrieved_at": datetime.now(UTC).isoformat(),
                   "sha256": sha256(Path(__file__)),
                   "validator_inputs": {name: sha256(Path(__file__).with_name(name))
                                        for name in ("release_gate.py", "finalize_release.py")},
                   "shared_gate_executable_sha256": gate.executable_sha256,
                   "manifest_sha256": sha256(root / "SHA256SUMS"),
                   "rdf_sha256": sha256(root / "graph.ttl"),
                   "release_version": report["version"], "result": result}
        # Failed validations never leave a success receipt for their manifest.
        args.receipt.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"RELEASE VALIDATION FAILED: {error}", file=sys.stderr)
        raise
