"""Shared offline gates for finalization, independent validation, and publishing."""
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

MAIN = Path(__file__).resolve().parents[3]
CC_BY = "https://creativecommons.org/licenses/by/4.0/"
CC0 = "https://creativecommons.org/publicdomain/zero/1.0/"
# Reviewed *mapping artifacts*, not an inference from their self-reported licence.
MAPPING_POLICIES = {
    "disease-xrefs": (CC_BY, "MONDO cross-references; IDENTITY.md and LICENCES.md"),
    "disease-orphanet-xrefs": (CC_BY, "Orphadata product 1 cross-references; IDENTITY.md"),
    "gene-xrefs": (CC0, "HGNC and NCBI Gene identifier cross-references; IDENTITY.md"),
    "drug-xrefs": (CC0, "UniChem identifier pairs; DrugBank IDs only; IDENTITY.md"),
    "funder-ror": (CC0, "Only rows whose evidence is the CC0 ROR dump; other rows held"),
    "assets-cell-identifiers": (CC_BY, "Cellosaurus direct identifier cross-references; LICENCE-CROSSCHECK.md Q22"),
}
HELD_SETS = {
    "alliance-human-orthologs": "Artifact-level Alliance grant unresolved in LICENCE-CROSSCHECK.md",
    "trial-xrefs": "CT.gov, WHO ICTRP and EU registry permissions/acquisition unresolved",
    "org-ror": "Mixed unverified organisation-directory inputs; set held conservatively",
    "work-ids": "Mixed publication/research-cache input permissions unresolved; source links retained in graph",
}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def public_url(value: str) -> str:
    parsed = urlsplit(value)
    require(parsed.scheme in {"http", "https"} and parsed.netloc and not parsed.username,
            "invalid public provenance URL")
    require(not re.search(r"\s", value), "invalid URL whitespace")
    return urlunsplit((parsed.scheme, parsed.netloc, parsed.path, "", ""))


def cache_path(value: str) -> str:
    value = value.strip().replace("\\", "/").removeprefix("./").removeprefix("data/")
    value = value if value.startswith(("cache/", "raw/")) else "cache/" + value
    require(":" not in value and all(p not in {"", ".", ".."} for p in value.split("/")), "unsafe cache path")
    return value


def locator(value: str) -> str:
    value = str(value).strip().lstrip("#")
    if value == "*":
        return ""
    if value.isdecimal():
        return "L" + str(int(value))
    match = re.match(r"/records/(\d+)(?:/|$)", value)
    if match:
        return f"records[{int(match[1])}]"
    if re.fullmatch(r"line:\d+", value):
        return "L" + str(int(value[5:]))
    return value.split("]/", 1)[0] + "]" if "]/" in value else value


class Gate:
    """Consume the index parsed by atlas_core::withhold; never accept a second schema."""
    def __init__(self, data: Path):
        self.data = data
        executable = Path(__file__).resolve().parents[3] / "target/debug/atlas-release"
        if os.name == "nt":
            executable = executable.with_suffix(".exe")
        require(executable.is_file(), "build atlas-release before running release gates")
        self.executable_sha256 = sha256(executable)
        result = subprocess.run([str(executable), "withhold-index", "--data", str(data.resolve())],
                                capture_output=True, text=True, encoding="utf-8", check=False)
        require(result.returncode == 0, "shared withholding filter rejected manifests; fail-closed")
        index = json.loads(result.stdout)
        self.identity = index["identity"]
        self.quarantine_records = set(map(tuple, index["quarantine_records"]))
        self.quarantine_urls = set(index["quarantine_urls"])
        self.suppressed_keys = set(index["suppression_keys"])
        salt = os.environ.get("ATLAS_SUPPRESSION_SALT", "").strip() or "zebratlas-dev-suppression-salt-v1"
        self.salt = salt.encode()
        require(hashlib.sha256(self.salt).hexdigest()[:8] == index["salt_id"], "withholding salt changed")
        self.snapshots = {kind: {key: index[kind][key] for key in ["present", "manifest_sha256"]}
                          for kind in ["quarantine", "suppression"]}

    def blocked(self, identifiers=(), refs=()) -> bool:
        # Public Salt::key(Node, id) contract; all person/profile data are excluded separately.
        if self.suppressed_keys and any(hashlib.sha256(self.salt + b"\x1fnode:" + i.strip().encode()).hexdigest()
                                        in self.suppressed_keys for i in identifiers):
            return True
        for ref in refs:
            file = ref.get("cache_file", "")
            if file:
                file = cache_path(file)
                pair = (file, locator(ref.get("record_locator", "")))
                if pair in self.quarantine_records or (file, "") in self.quarantine_records:
                    return True
            for key in ["source_url", "record_url"]:
                url = ref.get(key)
                if url and url.split("?", 1)[0].split("#", 1)[0] in self.quarantine_urls:
                    return True
        return False

    def check_snapshot(self, report):
        for kind, snapshot in self.snapshots.items():
            actual = report.get(kind, {})
            for key, value in snapshot.items():
                require(actual.get(key) == value, f"{kind} changed since build; rebuild release")

    def check_identity_mapping(self, row):
        require(row.get("mapping_justification") == "semapv:MappingChaining"
                and row.get("rule_id") == "R-DIS-08" and row.get("rule_version") == "1.0.0",
                "unjustified exact mapping")
        manifest = self.identity["manifest_sha256"]
        require(manifest and row.get("identity_gate_manifest_sha256") == manifest,
                "identity mapping gate changed; rebuild release")
        representatives = self.identity["representatives"]
        representative = representatives.get(row["subject_id"])
        require(representative and representatives.get(row["object_id"]) == representative,
                "identity mapping endpoints lack accepted equivalence")
        assertions = json.loads(row.get("identity_assertion_ids", "[]"))
        decisions = json.loads(row.get("identity_decision_ids", "[]"))
        require(isinstance(assertions, list) and isinstance(decisions, list)
                and assertions and len(assertions) == len(decisions)
                and len(set(decisions)) == len(decisions), "identity mapping receipts missing or duplicated")
        for assertion, decision in zip(assertions, decisions, strict=True):
            require(self.identity["decisions"].get(decision) == {
                "assertion": assertion, "representative": representative}, "identity mapping receipt not accepted")


SECRET_PATTERNS = [
    re.compile(rb"\bhf_[A-Za-z0-9]{20,}\b"),
    # Legacy keys are alphanumeric; project/service keys have explicit markers.
    # A hyphenated source identifier beginning sk- is not a legacy API key.
    re.compile(rb"\b(?:sk-[A-Za-z0-9]{20,}|sk-(?:proj|svcacct)-[A-Za-z0-9_-]{20,})\b"),
    re.compile(rb"\b(?:ghp_|github_pat_)[A-Za-z0-9_]{20,}\b"),
    re.compile(rb"\bAKIA[0-9A-Z]{16}\b"),
    re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    re.compile(rb"(?i)(?:api[_-]?key|access[_-]?token|password|client[_-]?secret)\s*[=:]\s*[\"']?[A-Za-z0-9_./+-]{16,}"),
]
SECRET_HINTS = [
    (b"hf_",), (b"sk-",), (b"ghp_", b"github_pat_"), (b"akia",), (b"private key",),
    (b"api_key", b"api-key", b"apikey", b"access_token", b"access-token", b"accesstoken",
     b"password", b"client_secret", b"client-secret", b"clientsecret"),
]


def check_secrets(path: Path, env_path: Path | None = None):
    secrets = []
    if env_path and env_path.exists():
        from dotenv import dotenv_values
        secrets = [v.encode() for k, v in dotenv_values(env_path).items()
                   if v and len(v) >= 12 and re.search(r"TOKEN|SECRET|KEY|PASSWORD", k, re.IGNORECASE)]
    overlap = max([1024] + [len(s) for s in secrets])
    tail = b""
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            data = tail + chunk
            lower = data.lower()
            require(not any(pattern.search(data) for pattern, hints in zip(SECRET_PATTERNS, SECRET_HINTS, strict=True)
                            if any(hint in lower for hint in hints)), f"SECRET SCAN FAILED: {path.name}")
            require(not any(s in data for s in secrets), f"PRIVATE ENV VALUE LEAK: {path.name}")
            tail = data[-overlap:]


def check_cff(root: Path, publishing=False):
    import yaml
    from cffconvert import Citation
    # Validate the actual CFF schema. The literal ORCID placeholder lives in a
    # comment, because an orcid field containing it is not a schema-valid URI.
    Citation((root / "CITATION.cff").read_text(encoding="utf-8")).validate()
    cff = yaml.safe_load((root / "CITATION.cff").read_text(encoding="utf-8"))
    require(cff.get("type") == "dataset" and cff.get("license") == "CC-BY-4.0", "CFF dataset/licence mismatch")
    card_text = (root / "README.md").read_text(encoding="utf-8")
    require(card_text.startswith("---\n"), "missing dataset-card YAML")
    card = yaml.safe_load(card_text.split("---", 2)[1])
    require(card.get("license") == "cc-by-4.0" and card.get("tags") and card.get("size_categories"), "invalid dataset-card metadata")
    if publishing:
        require("{AUTHOR_NAME}" not in json.dumps(cff, default=str), "fill AUTHOR_NAME before upload")
        require("{AUTHOR_ORCID}" not in (root / "CITATION.cff").read_text(encoding="utf-8"),
                "fill AUTHOR_ORCID or remove the optional comment before upload")
    return {"cff_schema_validated": True, "dataset_card_validated": True}
