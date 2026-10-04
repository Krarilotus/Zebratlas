#!/usr/bin/env python3
"""Data manifest for a zebratlas data release (D33).

A data release is the set of files the server's code reads, selected by the rules below,
listed as `sha256  size  path` (sorted by path). Its id is the first 16 hex characters of
the manifest's own SHA-256. The same manifest is verified on the server before use.

  datamanifest.py build ROOT            print the manifest for data root ROOT
  datamanifest.py id MANIFEST           print the data id of a manifest file
  datamanifest.py audit CRATES_DIR      list cache paths the code reads that the selection misses

Selection (derived from the code, 2026-10-04; see docs/ops/RUNBOOK.md "Data"):
  raw/*                                  pinned raw inputs (files directly in raw/)
  cache/<dir>/<pattern> (CACHE_RULES)    files read by atlas-ingest (graph), atlas-server, atlas-contrib
Excluded everywhere: raw page snapshots (pages/, serp/, *.html), logs, pickles, temp files.
Never included: cache/llm (server state), app/ (accounts, contributions), snapshots
(built by the server), scratch dirs (local-model, e2e, quality, deploy, bundle, ...).
"""
from __future__ import annotations

import fnmatch
import hashlib
import os
import sys
from pathlib import Path, PurePosixPath

# Per cache directory: the files the code reads (fnmatch patterns relative to cache/<dir>/),
# from the readers in atlas-ingest/atlas-server/atlas-contrib (re-derive with `audit`).
CACHE_RULES: dict[str, tuple[str, ...]] = {
    # whole directories (all non-excluded files)
    "claims": ("*",), "clinvar": ("*",), "contacts": ("*",), "evidence": ("*",),
    "labels": ("*",), "models": ("*",), "orgs": ("*",), "people": ("*",), "pubmed": ("*",),
    "registries": ("*",), "reporter": ("*",), "repurposing": ("*",), "trials": ("*",),
    "discovery": ("*",),
    # named files only (these dirs also hold scraped pages and scratch)
    "directories": ("erdri.json",),
    "funders": ("calls.json",),
    "groups-wide": ("organisations.json",),
    "kg-scan": ("resources.json",),
    "org-assets": ("assets.json", "duplicates.json"),
    "outcomes": ("resources.json",),
    "ecosystem": ("*.json", "requests.jsonl", "responses/*.bin"),
    "bench-fixes": ("organisations.json", "reporter/*.json"),
    "openaccess": ("live-papers.json",),
    "pipelines": ("evidence.json",),
    "regulatory": ("signals.json",),
    "research_intl": ("cordis.json", "gtr.json", "kaken.json", "europepmc_grants.json"),
    "mappings": ("*.sssom.tsv", "identity-gate.manifest.json", "identity-decisions.jsonl",
                 "identity-invalidations.jsonl", "rules.json", "review-diff.tsv"),
    # Private checksum-pinned serving inputs; never part of the public Git repository.
    "runtime": ("atlas.snapshot", "graph.snapshot", "mechanism.snapshot", "operational.ttl", "schema.json",
                "operational.manifest.json", "operational.source.manifest.json", "runtime.manifest.json",
                "native-build.receipt.json"),
    "kgx": ("*/nodes.jsonl", "*/nodes.jsonl.gz", "*/edges.jsonl", "*/edges.jsonl.gz", "*/manifest.json"),
}
CACHE_FILES = ("quarantine.json",)  # files directly in cache/
CACHE_DIRS = tuple(CACHE_RULES)
EXCLUDE_DIRS = {"pages", "serp", "__pycache__", ".git"}
EXCLUDE_SUFFIXES = (".html", ".htm", ".log", ".pkl", ".part", ".tmp", ".bak", ".pyc", ".py")
EXCLUDE_NAMES = {".DS_Store", "Thumbs.db"}
DATA_FILES = ("suppression.json",)


def selected(rel: PurePosixPath) -> bool:
    parts = rel.parts
    if len(parts) == 1:
        return parts[0] in DATA_FILES
    if any(p in EXCLUDE_DIRS for p in parts) or rel.name in EXCLUDE_NAMES:
        return False
    if rel.name.endswith(EXCLUDE_SUFFIXES):
        return False
    if parts[0] == "raw":
        return len(parts) == 2
    if parts[0] != "cache":
        return False
    if len(parts) == 2:
        return parts[1] in CACHE_FILES
    rules = CACHE_RULES.get(parts[1])
    if not rules:
        return False
    sub = "/".join(parts[2:])
    return any(fnmatch.fnmatchcase(sub, r) for r in rules)


def audit(crates: Path) -> int:
    """Warn about cache paths the Rust code mentions that the selection does not cover."""
    import re
    found: set[str] = set()
    for f in crates.rglob("*.rs"):
        if "/tests/" in f.as_posix() or f.name.endswith("_tests.rs"):
            continue
        t = f.read_text(encoding="utf-8", errors="replace")
        found.update(re.findall(r'"cache/([A-Za-z0-9_.-]+)', t))
        found.update(re.findall(r'join\("cache"\)\s*\.join\("([A-Za-z0-9_.-]+)"\)', t))
    # server state / built by the server / test fixtures inside #[cfg(test)] modules
    ignore = {"llm", "ask", "contrib-datasource", "atlas.snapshot", "graph.snapshot", "mechanism.snapshot",
              "bundle", "labels-evil", "unknown"}
    missing = sorted(d for d in found if d not in CACHE_RULES and d not in CACHE_FILES and d not in ignore)
    for d in missing:
        print(f"not in the data release: cache/{d}")
    return 1 if missing else 0


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def build(root: Path) -> str:
    lines = []
    for name in DATA_FILES:
        path = root / name
        if path.is_file() and not path.is_symlink():
            lines.append(f"{sha256(path)}  {path.stat().st_size}  {name}")
    for base in ("raw", "cache"):
        top = root / base
        if not top.is_dir():
            continue
        for dirpath, dirnames, filenames in os.walk(top, followlinks=False):
            dirnames[:] = sorted(d for d in dirnames if d not in EXCLUDE_DIRS)
            for name in sorted(filenames):
                p = Path(dirpath) / name
                rel = PurePosixPath(p.relative_to(root).as_posix())
                if p.is_symlink() or not p.is_file() or not selected(rel):
                    continue
                if any(c in str(rel) for c in ("\n", "\t", "  ")):
                    raise SystemExit(f"unsupported file name: {rel!r}")
                lines.append(f"{sha256(p)}  {p.stat().st_size}  {rel}")
    lines.sort(key=lambda l: l.split("  ", 2)[2])
    return "".join(l + "\n" for l in lines)


def data_id(manifest: bytes) -> str:
    return hashlib.sha256(manifest).hexdigest()[:16]


def main(argv: list[str]) -> None:
    if len(argv) == 2 and argv[0] == "build":
        sys.stdout.buffer.write(build(Path(argv[1])).encode())
    elif len(argv) == 2 and argv[0] == "audit":
        raise SystemExit(audit(Path(argv[1])))
    elif len(argv) == 2 and argv[0] == "id":
        print(data_id(Path(argv[1]).read_bytes()))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
