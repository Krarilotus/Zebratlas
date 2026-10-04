"""Only publishes after validating an isolated, checksum-verified copy. Never logs tokens."""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "crates/atlas-release/scripts"))
from release_gate import MAIN, Gate, require, sha256


def verify_published(repo_id: str, revision: str, root: Path) -> dict:
    """Verify anonymous public downloads at the exact uploaded commit, byte for byte."""
    from huggingface_hub import HfApi, hf_hub_download
    api = HfApi(token=False)
    info = api.repo_info(repo_id, repo_type="dataset", revision=revision)
    require(not info.private, "dataset is not publicly accessible")
    expected = {name: digest for digest, name in
                (line.split("  ", 1) for line in (root / "SHA256SUMS").read_text().splitlines())}
    expected["SHA256SUMS"] = sha256(root / "SHA256SUMS")
    remote = set(api.list_repo_files(repo_id, repo_type="dataset", revision=revision))
    # Hub creates/maintains this transport file for LFS/Xet; it is not a dataset payload.
    require(remote - {".gitattributes"} == set(expected), "remote file inventory mismatch")
    verified = {}
    with tempfile.TemporaryDirectory(prefix="verify-hub-") as cache:
        for name, digest in sorted(expected.items()):
            path = Path(hf_hub_download(repo_id, name, repo_type="dataset", revision=revision,
                                       token=False, cache_dir=cache))
            require(sha256(path) == digest, f"remote checksum mismatch: {name}")
            verified[name] = {"sha256": digest, "bytes": path.stat().st_size}
            print(f"Verified public download: {name}", flush=True)
    return {"dataset_url": f"https://huggingface.co/datasets/{repo_id}", "revision": revision,
            "public": True, "verified_files": verified, "sha256sums_sha256": expected["SHA256SUMS"],
            "platform_metadata_files": sorted(remote - set(expected))}


def publish(repo_id: str, root: Path):
    require(bool(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", repo_id)),
            "dataset repository must be OWNER/DATASET")
    require(root.is_dir() and not root.is_symlink(), "release directory missing or unsafe")
    require(not any(p.is_symlink() for p in root.rglob("*")), "symlinks are forbidden in upload")
    source_files = [p for p in root.rglob("*") if p.is_file()]
    required_bytes = sum(p.stat().st_size for p in source_files)
    require(shutil.disk_usage(REPO).free > required_bytes + (512 << 20), "insufficient disk for isolated validation copy")
    # Source mutation during copy is detected against the copied SHA256SUMS. All
    # validation happens on the same private tree whose bytes will be uploaded.
    with tempfile.TemporaryDirectory(prefix="atlas-publish-", dir=REPO) as temp:
        stage = Path(temp) / "dataset"
        shutil.copytree(root, stage)
        subprocess.run([sys.executable, str(REPO / "crates/atlas-release/scripts/validate_release.py"),
                        str(stage), "--data", str(MAIN / "data"), "--env-file", str(MAIN / ".env"),
                        "--publishing"], check=True, cwd=REPO)
        Gate(MAIN / "data").check_snapshot(json.loads((stage / "release-report.json").read_text()))
        # Authentication is deferred until every gate passes. The validator
        # reads .env only to detect secret leakage. No token on argv or login.
        from dotenv import dotenv_values
        token = dotenv_values(MAIN / ".env").get("HF_TOKEN")
        require(bool(token), "HF_TOKEN is missing from the main checkout .env")
        from huggingface_hub import HfApi
        from huggingface_hub.errors import RepositoryNotFoundError
        api = HfApi(token=token)
        try:
            existing = api.list_repo_files(repo_id=repo_id, repo_type="dataset")
            require(not (set(existing) - {".gitattributes"}), "destination dataset is not empty; use a new version repo")
        except RepositoryNotFoundError:
            api.create_repo(repo_id=repo_id, repo_type="dataset", private=False)
        api.update_repo_settings(repo_id=repo_id, repo_type="dataset", private=False)
        files = sorted(p.relative_to(stage).as_posix() for p in stage.rglob("*") if p.is_file())
        result = api.upload_folder(repo_id=repo_id, repo_type="dataset", folder_path=str(stage),
            allow_patterns=files, commit_message="Publish validated Zebratlas knowledge graph v0.1")
        verification = verify_published(repo_id, result.oid, stage)
        evidence = REPO / "release/logs/published-v0.1.json"
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_text(json.dumps(verification, indent=2) + "\n", encoding="utf-8")
        print(f"Published https://huggingface.co/datasets/{repo_id} at commit {result.oid}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("dataset_repo")
    parser.add_argument("release", type=Path, nargs="?", default=REPO / "release/zebratlas-kg-v0.1")
    args = parser.parse_args()
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"
    try:
        publish(args.dataset_repo, args.release.resolve())
    except Exception:  # noqa: BLE001 - HTTP errors may contain credential-bearing request details.
        # Library exceptions can contain request headers/URLs. Do not emit them.
        print("Upload stopped. Review the preceding validation output, repository permissions, and local HF_TOKEN configuration.", file=sys.stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
