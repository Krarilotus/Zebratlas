#!/usr/bin/env python3
"""Start the isolated Zebra API against a checksum-pinned repo-graph RDF adapter.

No ingestion, upload, or existing-store mutation is performed by this launcher.
The separate one-time import must use RAYON_NUM_THREADS=1 (Turtle chunk state).
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import tomllib
import urllib.request


def start_child(command, *, env, flags):
    """Retain redirected parent output under Windows CREATE_NO_WINDOW."""
    return subprocess.Popen(
        command, env=env, creationflags=flags,
        stdin=subprocess.DEVNULL, stdout=sys.stdout, stderr=sys.stderr,
    )


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        while block := f.read(1024 * 1024):
            h.update(block)
    return h.hexdigest()


def ready(url):
    try:
        with urllib.request.urlopen(url, timeout=2) as r:
            return r.status == 200
    except (OSError, urllib.error.URLError):
        return False


def require_free_port(port):
    with socket.socket() as probe:
        try:
            probe.bind(("127.0.0.1", port))
        except OSError:
            raise SystemExit(f"Port {port} is occupied; choose an owned free port") from None


def verified_reasoning(path, args, manifest, config):
    profile = json.loads(path.read_text(encoding="utf-8"))
    if (profile.get("format") != "zebratlas-verified-reasoning-profile-v1"
            or profile.get("runtime_reasoning") is not True
            or profile.get("public_release") is not False
            or profile.get("engine") != "nrese"
            or profile.get("mode") != "custom"
            or profile.get("rule") != "rdfs-subclass-transitivity"
            or config.get("reasoner", {}).get("mode") != "custom"):
        raise SystemExit("Unsupported reasoning profile")
    if (profile.get("source_rdf_sha256") != manifest["rdf_sha256"]
            or Path(profile["source_manifest"]).resolve() != args.manifest.resolve()
            or Path(profile["config"]).resolve() != args.config.resolve()
            or Path(profile["binary"]).resolve() != args.nrese.resolve()
            or Path(profile["store"]).resolve() != Path(config["store"]["data_dir"]).resolve()
            or Path(profile["rules"]).resolve() != Path(config["reasoner"]["rules"]).resolve()):
        raise SystemExit("Reasoning profile does not match the selected artifacts")
    for key in ["source_manifest", "config", "rules", "binary", "checkpoint", "reasoning_state", "preflight"]:
        if digest(Path(profile[key])) != profile[key + "_sha256"]:
            raise SystemExit(f"Reasoning checksum mismatch for {key}")
    receipts = profile.get("verification_receipts", [])
    if not receipts:
        raise SystemExit("Reasoning requires actual source/proof verification receipts")
    for receipt in receipts:
        if digest(Path(receipt["path"])) != receipt["sha256"]:
            raise SystemExit("Reasoning verification receipt has changed")
    endpoint = f"http://127.0.0.1:{args.nrese_port}/api/v1/repositories/nrese/sparql"
    if profile.get("endpoint") != endpoint or profile.get("proof_endpoint") != endpoint.removesuffix("sparql") + "explain":
        raise SystemExit("Reasoning endpoint does not match the selected loopback port")
    return endpoint


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--nrese", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--schema", type=Path)
    parser.add_argument("--reasoning-manifest", type=Path, help="Optional verified, bounded hierarchy reasoning profile")
    parser.add_argument("--env-file", type=Path, help="Optional local provider settings; values are never printed")
    parser.add_argument("--data", type=Path, default=Path("data"))
    parser.add_argument("--accounts-db", type=Path, help="Explicit account database; otherwise keep configured/shared accounts")
    parser.add_argument("--contributions-db", type=Path, help="Explicit submissions database; otherwise keep configured/shared submissions")
    parser.add_argument("--free-spend-file", type=Path, help="Owned free-assistant quota ledger for a parallel API; avoids sharing a writer lock")
    parser.add_argument("--api-port", type=int, default=8001)
    parser.add_argument("--nrese-port", type=int, default=3161)
    args = parser.parse_args()
    env = os.environ.copy()
    if args.env_file:
        for raw in args.env_file.read_text(encoding="utf-8-sig").splitlines():
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            line = line.removeprefix("export ")
            key, sep, value = line.partition("=")
            if not sep or not key.replace("_", "").isalnum():
                raise SystemExit("Unsupported local environment line; use KEY=value")
            value = value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            env.setdefault(key, value)
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    if manifest.get("public_release") is not False or manifest.get("runtime_reasoning") is not False:
        raise SystemExit("Expected the private, non-reasoning operational adapter manifest")
    for file_key, hash_key in [("rdf", "rdf_sha256"), ("atlas_snapshot", "atlas_sha256"), ("graph_snapshot", "graph_sha256")]:
        path = Path(manifest[file_key]).resolve()
        if digest(path) != manifest[hash_key]:
            raise SystemExit(f"Checksum mismatch for {file_key}; refusing activation")
    config = tomllib.loads(args.config.read_text(encoding="utf-8"))
    if (config.get("server", {}).get("bind_address") != f"127.0.0.1:{args.nrese_port}"
            or config.get("server", {}).get("deployment_posture") != "read-only-demo"):
        raise SystemExit("Configuration must match the read-only loopback store")
    endpoint = f"http://127.0.0.1:{args.nrese_port}/dataset/sparql"
    if args.reasoning_manifest:
        endpoint = verified_reasoning(args.reasoning_manifest, args, manifest, config)
        env["ATLAS_REASONING_MANIFEST"] = str(args.reasoning_manifest.resolve())
    else:
        if config.get("reasoner", {}).get("mode") != "disabled":
            raise SystemExit("Reasoning requires an explicit verified profile")
        env.pop("ATLAS_REASONING_MANIFEST", None)
        receipt = json.loads(args.config.with_suffix(".load.json").read_text(encoding="utf-8"))
        if (receipt.get("rdf_sha256") != manifest["rdf_sha256"]
                or receipt.get("config_sha256") != digest(args.config)
                or receipt.get("nrese_sha256") != digest(args.nrese)
                or receipt.get("skipped") != 0
                or Path(receipt["store"]).resolve() != Path(config["store"]["data_dir"]).resolve()):
            raise SystemExit("Import receipt does not match the configured RDF, store and executable")
    # File config owns the loaded store. Reject stale global engine overrides.
    for key in list(env):
        if key.startswith("NRESE_"):
            del env[key]
    env.update(RAYON_NUM_THREADS="2", TOKIO_WORKER_THREADS="2")
    env.update(ATLAS_SPARQL_URL=endpoint, ATLAS_QUERY_ENDPOINT=endpoint,
               ATLAS_OPERATIONAL_MANIFEST=str(args.manifest.resolve()),
               RARE_ATLAS_DATA=str(args.data.resolve()),
               RARE_ATLAS_ATLAS_SNAPSHOT=str(Path(manifest["atlas_snapshot"]).resolve()),
               RARE_ATLAS_GRAPH_SNAPSHOT=str(Path(manifest["graph_snapshot"]).resolve()))
    if manifest.get("mapping_directory") or manifest.get("identity_gate_sha256"):
        if not manifest.get("mapping_directory") or not manifest.get("identity_gate_sha256"):
            raise SystemExit("Identity projection requires both its directory and manifest hash")
        mappings = Path(manifest["mapping_directory"]).resolve()
        if digest(mappings / "identity-gate.manifest.json") != manifest["identity_gate_sha256"]:
            raise SystemExit("Identity gate manifest has changed; refusing activation")
        env["RARE_ATLAS_MAPPING_DIR"] = str(mappings)
        env["RARE_ATLAS_IDENTITY_MANIFEST_SHA256"] = manifest["identity_gate_sha256"]
    for option, key in [(args.accounts_db, "RARE_ATLAS_ACCOUNTS_DB"),
                        (args.contributions_db, "RARE_ATLAS_CONTRIB_DB"),
                        (args.free_spend_file, "ATLAS_FREE_SPEND_FILE")]:
        if option:
            env[key] = str(option.resolve())
    if args.schema:
        card = json.loads(args.schema.read_text(encoding="utf-8"))
        if card.get("graph", {}).get("sha256") != manifest["rdf_sha256"]:
            raise SystemExit("Schema describes another RDF artifact; regenerate it")
        env["ATLAS_QUERY_SCHEMA"] = str(args.schema.resolve())
    children = []
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    try:
        # Do not connect silently to an existing process with unknown store identity.
        if args.api_port == args.nrese_port:
            raise SystemExit("API and nrese must use separate ports")
        require_free_port(args.api_port)
        require_free_port(args.nrese_port)
        children.append(start_child([str(args.nrese.resolve()), "--config", str(args.config.resolve())], env=env, flags=flags))
        deadline = time.monotonic() + 90
        while not ready(f"http://127.0.0.1:{args.nrese_port}/readyz"):
            if children[0].poll() is not None or time.monotonic() >= deadline:
                raise SystemExit("The owned nrese process did not become ready")
            time.sleep(0.25)
        print(f"Starting Zebra API :{args.api_port}; nrese :{args.nrese_port}; graph {manifest['graph_sha256'][:12]}", flush=True)
        children.append(start_child([str(args.server.resolve()), "--data", str(args.data.resolve()), "serve", "--addr", f"127.0.0.1:{args.api_port}"], env=env, flags=flags))
        while all(child.poll() is None for child in children):
            time.sleep(0.5)
        raise SystemExit("One owned process exited; stopped the matching preview")
    except KeyboardInterrupt:
        pass
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()


if __name__ == "__main__":
    main()
