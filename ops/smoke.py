#!/usr/bin/env python3
"""D33 smoke test of a live zebratlas release (also used for the local reference run).

  smoke.py --base https://zebratlas.org [--resolve IP] [--expect-commit C --expect-data D]
           [--reference ref.json] [--observed-out obs.json] [--llm] [--no-web]

Checks: health; /api/stats release (commit, data); STXBP1 page; connections counts for
MONDO:0012812; questions; STX1A gap (G2P:G2P03465); verify; RDF export; one free-assistant
call (--llm). With --reference, the observed counts (integrity totals + connections + gap)
must equal the reference exactly. Exit 0 only if every check passes. Uses curl (no deps).
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from urllib.parse import urlencode, urlparse

RICH = "MONDO:0012812"   # STXBP1 encephalopathy (demo.rich)
GAP = "G2P:G2P03465"     # STX1A (demo.gap)


class Smoke:
    def __init__(self, base: str, resolve: str | None):
        self.base = base.rstrip("/")
        u = urlparse(self.base)
        port = u.port or (443 if u.scheme == "https" else 80)
        self.extra = ["--resolve", f"{u.hostname}:{port}:{resolve}"] if resolve else []
        self.results: list[tuple[str, bool, str]] = []

    def fetch(self, path: str, data: str | None = None, timeout: int = 90) -> tuple[int, bytes]:
        cmd = ["curl", "-sS", "--max-time", str(timeout), "-o", "-", "-w", "\n%{http_code}", *self.extra]
        if data is not None:
            cmd += ["-H", "content-type: application/json", "--data-binary", "@-"]
        cmd.append(self.base + path)
        p = subprocess.run(cmd, input=(data or "").encode(), capture_output=True, check=False)
        body, _, code = p.stdout.rpartition(b"\n")
        try:
            return int(code), body
        except ValueError:
            return 0, b""

    def json(self, path: str, **kw):
        code, body = self.fetch(path, **kw)
        if code != 200:
            raise RuntimeError(f"HTTP {code}")
        return json.loads(body)

    def check(self, name: str, fn) -> object:
        t = time.monotonic()
        try:
            detail = fn()
            ok = True
        except Exception as e:  # noqa: BLE001 - every failure is a failed check
            detail, ok = f"{type(e).__name__}: {e}", False
        self.results.append((name, ok, f"{detail} ({time.monotonic() - t:.1f}s)"))
        return detail if ok else None


def page(s: Smoke, path: str, must: str) -> str:
    code, body = s.fetch(path)
    if code != 200 or must.lower().encode() not in body.lower():
        raise RuntimeError(f"HTTP {code}, {len(body)} B, '{must}' {'found' if must.lower().encode() in body.lower() else 'missing'}")
    return f"200, {len(body)} B"


def questions_ready(s: Smoke, wait_seconds: float = 300) -> dict:
    """Health is liveness; questions also require the background similarity index.

    Retry only the API's explicit transient warming_up response. Permanent errors,
    malformed responses and transport failures remain failures, with their detail.
    """
    deadline = time.monotonic() + wait_seconds
    while True:
        code, raw = s.fetch(f"/api/condition/{RICH}/questions", timeout=120)
        try:
            body = json.loads(raw)
        except (ValueError, TypeError):
            raise RuntimeError(f"questions HTTP {code}: invalid JSON") from None
        if code == 200:
            if not isinstance(body, dict) or not isinstance(body.get("questions"), list):
                raise RuntimeError("questions HTTP 200: missing questions array")
            return body
        if code != 503 or not isinstance(body, dict) or body.get("status") != "warming_up":
            raise RuntimeError(f"questions HTTP {code}: {str(body)[:500]}")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError(f"questions still warming_up after {wait_seconds:g}s")
        time.sleep(min(5, remaining))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="https://zebratlas.org")
    ap.add_argument("--resolve")
    ap.add_argument("--expect-commit")
    ap.add_argument("--expect-data")
    ap.add_argument("--reference")
    ap.add_argument("--observed-out")
    ap.add_argument("--llm", action="store_true")
    ap.add_argument("--no-web", action="store_true", help="API only (local reference run)")
    ap.add_argument("--sparql", action="store_true", help="Include the independent public SPARQL smoke")
    ap.add_argument("--sparql-only", action="store_true", help="Check only SPARQL, separately from website rollback")
    ap.add_argument("--questions-wait", type=float, default=300, help="seconds to wait for explicit warming_up only")
    a = ap.parse_args()
    s = Smoke(a.base, a.resolve)
    obs: dict = {}

    if a.sparql or a.sparql_only:
        def sparql():
            answer = s.json("/sparql?" + urlencode({"query": "ASK {}", "infer": "false"}), timeout=15)
            if answer.get("boolean") is not True:
                raise RuntimeError("public SPARQL ASK did not return true")
            return "read-only ASK passed"
        s.check("SPARQL", sparql)
        if a.sparql_only:
            for name, ok, detail in s.results:
                print(f"{'PASS' if ok else 'FAIL'} {name}: {detail}")
            return 0 if all(ok for _, ok, _ in s.results) else 1

    s.check("health", lambda: s.json("/api/health") and "ok")

    def stats():
        st = s.json("/api/stats")
        rel = st.get("release") or {}
        obs["release"] = rel
        if a.expect_commit and not str(rel.get("commit") or "").startswith(a.expect_commit[:12]):
            raise RuntimeError(f"release.commit {rel.get('commit')} != {a.expect_commit}")
        if a.expect_data and rel.get("data") != a.expect_data:
            raise RuntimeError(f"release.data {rel.get('data')} != {a.expect_data}")
        return f"release {rel.get('commit')} / {rel.get('data')}"
    s.check("stats + release", stats)

    def integrity():
        i = s.json("/api/integrity", timeout=120)
        obs["integrity"] = {
            "nodes": i.get("nodes"),
            "edges_by_relation": i.get("edges_by_relation"),
            "contracts_failed": [c["id"] for c in i.get("contracts", []) if not c.get("passed")],
        }
        if obs["integrity"]["contracts_failed"]:
            raise RuntimeError(f"contracts failed: {obs['integrity']['contracts_failed']}")
        n = i.get("nodes") or {}
        return f"{sum(v for v in n.values() if isinstance(v, int))} nodes, {len(i.get('contracts', []))} contracts passed"
    s.check("integrity", integrity)

    def connections():
        c = s.json(f"/api/condition/{RICH}/connections", timeout=120)
        obs["connections"] = {k: v for k, v in sorted((c.get("counts") or {}).items())}
        tot = {k: v.get("exact", 0) for k, v in obs["connections"].items()}
        if not tot.get("patient_group") or not tot.get("researcher"):
            raise RuntimeError(f"implausible counts {tot}")
        return " ".join(f"{k}={v}" for k, v in tot.items())
    s.check("connections STXBP1", connections)

    def gap():
        g = s.json(f"/api/condition/{GAP}/gaps")
        obs["gap"] = {k: (len(v) if isinstance(v, (list, dict)) else v) for k, v in g.items() if k in ("found", "known", "missing", "searched", "next_questions")}
        if not g.get("missing"):
            raise RuntimeError("no gaps listed")
        return json.dumps(obs["gap"])
    s.check("STX1A gap", gap)

    s.check("questions (api)", lambda: f"{len(json.dumps(questions_ready(s, a.questions_wait)))} B")
    s.check("verify record", lambda: f"{len(s.json(f'/api/verify/{RICH}', timeout=120))} keys")

    def export():
        code, body = s.fetch(f"/api/export.ttl?condition={RICH}", timeout=120)
        if code != 200 or b"@prefix" not in body:
            raise RuntimeError(f"HTTP {code}, {len(body)} B")
        return f"{len(body)} B turtle"
    s.check("RDF export (api)", export)

    if not a.no_web:
        s.check("home /en", lambda: page(s, "/en", "atlas"))
        s.check("STXBP1 page", lambda: page(s, f"/en/c/{RICH}", "STXBP1"))
        s.check("questions page", lambda: page(s, f"/en/c/{RICH}/questions", "question"))
        s.check("STX1A gap page", lambda: page(s, f"/en/c/{GAP}", "STX1A"))
        s.check("RDF export (web)", lambda: page(s, f"/en/export?condition={RICH}", "@prefix"))

    if a.llm:
        def assistant():
            body = json.dumps({"question": "Which patient groups exist for STXBP1?", "lang": "en", "connection": "hosted-free"})
            for attempt in range(3):
                d = s.json("/api/ask", data=body, timeout=120)
                reason = (d.get("validation") or {}).get("fallback_reason") or d.get("provider_error")
                if not reason:
                    return f"{len(d.get('answer') or [])} cited sentences"
                if "busy" not in str(reason) or attempt == 2:
                    raise RuntimeError(f"fallback: {reason}")
                time.sleep(15)
        s.check("free assistant (1 call)", assistant)

    if a.reference:
        def reference():
            with open(a.reference, encoding="utf-8") as reference_file:
                ref = json.load(reference_file)
            diffs = [k for k in ("integrity", "connections", "gap") if ref.get(k) != obs.get(k)]
            if diffs:
                for k in diffs:
                    print(f"    reference {k}: {json.dumps(ref.get(k), sort_keys=True)[:300]}")
                    print(f"    observed  {k}: {json.dumps(obs.get(k), sort_keys=True)[:300]}")
                raise RuntimeError(f"differs from reference in {diffs}")
            return f"equal to {a.reference.rsplit('/', 1)[-1]}"
        s.check("counts == reference", reference)

    if a.observed_out:
        with open(a.observed_out, "w", encoding="utf-8") as f:
            json.dump(obs, f, indent=1, sort_keys=True)
    width = max(len(n) for n, _, _ in s.results)
    for name, ok, detail in s.results:
        print(f"{'PASS' if ok else 'FAIL'} {name:<{width}}  {detail}")
    failed = [n for n, ok, _ in s.results if not ok]
    print(f"smoke: {len(s.results) - len(failed)}/{len(s.results)} passed" + (f"; FAILED: {', '.join(failed)}" if failed else ""))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
