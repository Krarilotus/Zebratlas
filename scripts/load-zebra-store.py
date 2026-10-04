#!/usr/bin/env python3
"""Checksum-verify and serially import the private adapter into a NEW owned store."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import tomllib


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        while block := f.read(1024 * 1024):
            h.update(block)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nrese", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf8"))
    config = tomllib.loads(args.config.read_text(encoding="utf8"))
    owned = (Path(__file__).resolve().parents[1] / "data/cache/zebra-preview").resolve()
    store = Path(config["store"]["data_dir"]).resolve()
    if not store.is_relative_to(owned) or store == owned:
        raise SystemExit("Only a new store inside data/cache/zebra-preview is permitted")
    if store.exists() and any(store.iterdir()):
        raise SystemExit("Store is not empty; create a new owned directory instead")
    if manifest.get("public_release") is not False:
        raise SystemExit("Expected a private operational adapter")
    rdf = Path(manifest["rdf"]).resolve()
    if digest(rdf) != manifest["rdf_sha256"]:
        raise SystemExit("RDF checksum mismatch")
    if config["server"].get("deployment_posture") != "read-only-demo" or not config["server"]["bind_address"].startswith("127.0.0.1:"):
        raise SystemExit("Store must use a read-only loopback configuration")
    env = {k: v for k, v in os.environ.items() if not k.startswith("NRESE_")}
    # nrese's parallel Turtle parser loses prefix context across chunk boundaries.
    # Serial parsing is deliberate; the serving process uses two workers.
    env.update(RAYON_NUM_THREADS="1", TOKIO_WORKER_THREADS="2")
    started = time.monotonic()
    result = subprocess.run([str(args.nrese.resolve()), "load", "--config", str(args.config.resolve()), str(rdf)],
                            env=env, capture_output=True, text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    output = result.stdout + result.stderr
    print(output, end="")
    if result.returncode:
        raise SystemExit(result.returncode)
    match = re.search(r"bulk load complete.*?parsed=(\d+).*?inserted=(\d+).*?skipped=(\d+)", output)
    if not match or int(match[3]) != 0:
        raise SystemExit("No zero-skipped import proof; do not activate this store")
    receipt = {"rdf_sha256": manifest["rdf_sha256"], "graph_sha256": manifest["graph_sha256"],
               "config_sha256": digest(args.config), "nrese_sha256": digest(args.nrese), "store": str(store),
               "parsed": int(match[1]), "quads": int(match[2]), "skipped": 0,
               "elapsed_ms": round((time.monotonic() - started) * 1000), "parser_workers": 1}
    path = args.config.with_suffix(".load.json")
    with path.open("x", encoding="utf8") as f:
        json.dump(receipt, f, indent=2)


if __name__ == "__main__":
    main()
