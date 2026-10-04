"""Check public-package boundaries and copied-source hashes without printing secrets."""
from __future__ import annotations

import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RULES = "data/cache/mappings/rules.json"
SAMPLE = "data/public-sample/"
SAMPLE_MANIFEST_SHA256 = "488ffbe794453718e38694e45aedf4df6a320a7bb4bc343c011a4f98de1db213"
SKIP = {".git", "node_modules", "target", ".next", ".next-build", "__pycache__"}
PRIVATE_DIRS = {"raw", "eval", "research_notes", "outreach", "reports", "test-results"}
PRIVATE_SUFFIXES = {".snapshot", ".sqlite", ".sqlite3", ".db", ".pem", ".key", ".log"}
TOKEN = re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----|(?:sk-or-v1-[A-Za-z0-9]{32,}|sk-proj-[A-Za-z0-9_-]{32,}|hf_[A-Za-z0-9]{25,}|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})")
ASSIGNED_SECRET = re.compile(rb"(?i)(?:API_KEY|API_TOKEN|AUTH_TOKEN|CLIENT_SECRET|ACCESS_TOKEN)\s*[=:]\s*[\"']?[A-Za-z0-9_-]{28,}")
EMAIL = re.compile(rb"[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})")
NOTICE_NAMES = {"NOTICE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.txt", "SPARQL-LICENSES.txt"}
REQUIRED = {"Cargo.toml", "Cargo.lock", "NOTICE", "LICENSE-MIT", "LICENSE-APACHE", RULES, "docs/design/SOURCES.md", "web/package-lock.json", "web/app/zebra/page.tsx", "web/public/query-by-graph/THIRD-PARTY-LICENSES.txt", "web/public/query-by-graph/SPARQL-LICENSES.txt", "web/public/query-by-graph/PERMISSION.txt", "web/public/query-by-graph/graph-view-manifest.json", "SOURCE-MANIFEST.json"}

def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main() -> int:
    findings = []
    files = {}
    manifest = ROOT / "SOURCE-MANIFEST.json"
    source_manifest = json.loads(manifest.read_text(encoding="utf-8")) if manifest.exists() else {}
    contact_review = source_manifest.get("contact_review", {})
    approved_sample = set()
    sample_manifest = ROOT / SAMPLE / 'sample-manifest.json'
    if sample_manifest.exists():
        if digest(sample_manifest) != SAMPLE_MANIFEST_SHA256:
            findings.append({'path': SAMPLE + 'sample-manifest.json', 'reason': 'unreviewed-public-sample-manifest'})
        else:
            sample = json.loads(sample_manifest.read_text(encoding='utf-8'))
            if sample.get('format') != 'zebratlas-public-sample-manifest-v1' or sample.get('status') != 'verified_local_sample':
                findings.append({'path': SAMPLE + 'sample-manifest.json', 'reason': 'unverified-public-sample'})
            else:
                approved_sample.add(SAMPLE + 'sample-manifest.json')
                for entry in sample['files']:
                    relative = SAMPLE + entry['file']
                    path = ROOT / relative
                    if '..' in Path(entry['file']).parts or Path(entry['file']).is_absolute():
                        findings.append({'path': SAMPLE, 'reason': 'unsafe-sample-manifest-path'})
                        continue
                    if not path.is_file() or path.stat().st_size != entry['bytes'] or digest(path) != entry['sha256']:
                        findings.append({'path': relative, 'reason': 'public-sample-checksum-mismatch'})
                    else:
                        approved_sample.add(relative)
    for directory, dirs, names in os.walk(ROOT, followlinks=False):
        for name in list(dirs):
            candidate = Path(directory) / name
            attributes = candidate.lstat()
            if stat.S_ISLNK(attributes.st_mode) or getattr(attributes, "st_file_attributes", 0) & 0x400:
                findings.append({"path": candidate.relative_to(ROOT).as_posix(), "reason": "reparse-point"})
                dirs.remove(name)
            elif name in SKIP:
                dirs.remove(name)
        for name in names:
            file = Path(directory) / name
            relative = file.relative_to(ROOT).as_posix()
            attributes = file.lstat()
            if stat.S_ISLNK(attributes.st_mode) or getattr(attributes, "st_file_attributes", 0) & 0x400:
                findings.append({"path": relative, "reason": "reparse-point"})
                continue
            if file.suffix in {".pyc", ".tsbuildinfo"} or relative == "web/next-env.d.ts":
                continue
            sample_file = relative in approved_sample
            private = name.startswith(".env") or (file.suffix.lower() in PRIVATE_SUFFIXES and not sample_file) or any(part in PRIVATE_DIRS for part in file.parts)
            private |= relative.startswith("data/") and relative != RULES and not sample_file
            private |= relative.startswith("web/mock/") or relative.startswith("web/lib/mock/")
            if private:
                findings.append({"path": relative, "reason": "private-or-excluded-artifact"})
                continue
            data = file.read_bytes()
            if TOKEN.search(data) or ASSIGNED_SECRET.search(data):
                findings.append({"path": relative, "reason": "secret-pattern"})
            compiler_artifacts = {
                'web/public/query-by-graph/query_by_graph_bg.wasm': '92f16081bf3f81cf89a4bb0e0f546d9275dd685da30d63b77f3ac5076ad2a1e9',
                'web/public/query-by-graph/editor.js': 'cd4733a9254e00ccc8e5be82b2010a26b9e1f080f6edccd352574035053532b5',
            }
            compiler_metadata = hashlib.sha256(data).hexdigest() == compiler_artifacts.get(relative)
            if not compiler_metadata and re.search(rb'(?i)(?<![A-Za-z0-9.:])(?:C:[/\\]+Users[/\\]+[A-Za-z0-9_.-]+[/\\]|/(?:Users|home)/[A-Za-z0-9_.-]+/)', data):
                findings.append({'path': relative, 'reason': 'private-absolute-local-path'})
            if name not in NOTICE_NAMES:
                approved = contact_review.get(relative, {}).get("approved_email_sha256", [])
                for match in EMAIL.finditer(data):
                    domain = match[1].decode("ascii").lower()
                    example = domain in {"example.com", "example.org", "example.net"} or domain.endswith((".invalid", ".test", ".example"))
                    if not example and hashlib.sha256(match[0]).hexdigest() not in approved:
                        findings.append({"path": relative, "reason": "unreviewed-personal-or-operator-contact"})
                        break
            if name.endswith(".json") and relative != "SOURCE-MANIFEST.json":
                try:
                    pending = [json.loads(data)]
                except (ValueError, UnicodeError):
                    pending = []
                while pending:
                    node = pending.pop()
                    if isinstance(node, dict):
                        for key, scalar in node.items():
                            if isinstance(scalar, str):
                                personal = key.lower() in {"patient_id", "person_name", "researcher_name", "full_name", "first_name", "last_name"} and bool(scalar.strip())
                                personal |= sample_file and key.lower() in {'kind', 'type', 'node_kind'} and scalar.lower() in {'person', 'patient', 'researcher', 'contact'}
                                phone = key.lower() in {"phone", "phone_number", "contact_phone"} and len(re.sub(r"\D", "", scalar)) >= 8
                                orcid = key.lower() == "orcid" and re.search(r"\d{4}-\d{4}-\d{4}-[\dX]{4}", scalar)
                                if personal or phone or orcid:
                                    findings.append({"path": relative, "reason": "scalar-person-record-field"})
                            pending.append(scalar)
                    elif isinstance(node, list):
                        pending.extend(node)
            files[relative] = hashlib.sha256(data).hexdigest()
    for relative in sorted(REQUIRED - files.keys()):
        findings.append({"path": relative, "reason": "required-file-missing"})
    if manifest.exists():
        entries = source_manifest["entries"]
        expected_files = {entry["path"] for entry in entries} | {"SOURCE-MANIFEST.json"}
        for relative in sorted(files.keys() - expected_files):
            findings.append({"path": relative, "reason": "unmanifested-package-file"})
        for entry in entries:
            if files.get(entry["path"]) != entry["sha256"]:
                findings.append({"path": entry["path"], "reason": "source-manifest-hash-mismatch"})
        if "--staged" in sys.argv:
            specs = "".join(":" + entry["path"] + "\n" for entry in entries).encode("utf-8")
            result = subprocess.run(["git", "cat-file", "--batch"], input=specs, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
            offset = 0
            for entry in entries:
                end = result.stdout.index(b"\n", offset)
                header = result.stdout[offset:end].split()
                offset = end + 1
                if header[-1] == b"missing":
                    findings.append({"path": entry["path"], "reason": "staged-file-missing"})
                    continue
                size = int(header[-1])
                data = result.stdout[offset:offset + size]
                offset += size + 1
                if hashlib.sha256(data).hexdigest() != entry["sha256"]:
                    findings.append({"path": entry["path"], "reason": "staged-source-bytes-changed"})
    vendor = ROOT / "web/public/query-by-graph"
    view_manifest = vendor / "graph-view-manifest.json"
    if view_manifest.exists():
        pinned = json.loads(view_manifest.read_text(encoding="utf-8"))
        for name, key in (("editor.js", "editor_sha256"), ("query_by_graph_bg.wasm", "wasm_sha256"), ("query-parser.js", "parser_sha256")):
            if not (vendor / name).is_file() or digest(vendor / name) != pinned[key]:
                findings.append({"path": "web/public/query-by-graph/" + name, "reason": "vendor-hash-mismatch"})
    print(json.dumps({"passed": not findings, "files": len(files), "findings": findings}, indent=2))
    return int(bool(findings))

if __name__ == "__main__":
    sys.exit(main())
