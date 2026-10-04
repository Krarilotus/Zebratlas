"""Copy a verified private store into one new Atlas release's writable runtime path."""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path("/srv/zebratlas")


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def copy_store(source, destination, receipt):
    if source.is_symlink() or destination.exists() or destination.is_relative_to(source) or source.is_relative_to(destination):
        raise ValueError("store clone must be new and separate from immutable evidence")
    inventory = receipt.get("store_files", [])
    paths = set()
    for item in inventory:
        relative = Path(item["file"])
        if relative.is_absolute() or ".." in relative.parts or relative.as_posix() in paths:
            raise ValueError("unsafe certified store inventory")
        paths.add(relative.as_posix())
        original = source / relative
        if original.is_symlink() or not original.is_file() or digest(original) != item["sha256"] or original.stat().st_size != item["size"]:
            raise ValueError("immutable engine store does not match its receipt")
    if not paths or any(p.is_symlink() for p in source.rglob("*")) or {p.relative_to(source).as_posix() for p in source.rglob("*") if p.is_file()} != paths:
        raise ValueError("immutable engine store inventory differs")
    shutil.copytree(source, destination, copy_function=shutil.copy2)
    for item in inventory:
        relative = Path(item["file"])
        if digest(destination / relative) != item["sha256"] or digest(source / relative) != item["sha256"]:
            raise ValueError("store changed during private clone creation")


def main():
    import fcntl
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("release", type=Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError("root session required")
    directory = args.release.resolve()
    if not directory.is_relative_to(ROOT / "releases/astra"):
        raise ValueError("release is outside the Atlas staging directory")
    record = json.loads((directory / "release.json").read_bytes())
    commit, data = record["source_commit"], record["data_id"]
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or not re.fullmatch(r"[0-9a-f]{16}", data):
        raise ValueError("invalid frozen release identity")
    profile = Path(record["reasoning_profile"]["path"]).resolve()
    if not profile.is_relative_to(ROOT / "state/reasoning") or digest(profile) != record["reasoning_profile"]["sha256"]:
        raise ValueError("private reasoning profile is not pinned")
    receipt_path = profile.parent / "linux-import.receipt.json"
    if digest(receipt_path) != record["reasoning_profile"]["import_receipt_sha256"]:
        raise ValueError("Linux store receipt is not pinned")
    receipt = json.loads(receipt_path.read_bytes())
    if receipt.get("certified") is not True or receipt.get("workers_stopped") is not True:
        raise ValueError("uncertified engine source store")
    destination = ROOT / "state/reasoning-working" / (commit + "-" + data) / "store"
    if Path(record["reasoning_profile"]["working_store"]) != destination:
        raise ValueError("release clone path differs from its frozen identity")
    size = sum(item["size"] for item in receipt["store_files"])
    used = int(subprocess.run(["du", "-sx", "--block-size=1", str(ROOT)], capture_output=True, check=True).stdout.split()[0])
    if used + size > 40 * 1024**3 or shutil.disk_usage(ROOT).free - size < 25 * 1024**3:
        raise ValueError("private working-store clone exceeds the existing Atlas disk reservation")
    if not args.apply:
        print("DRY RUN: verified private per-release store clone; no service or source change")
        return
    with open("/run/zebratlas-release.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        destination.parent.mkdir(parents=True, mode=0o755, exist_ok=False)
        copy_store(profile.parent / "store", destination, receipt)
        uid = next(int(line.split(":")[1]) + 10000 for line in Path("/etc/subuid").read_text().splitlines() if line.startswith("zebratlas:"))
        gid = next(int(line.split(":")[1]) + 10000 for line in Path("/etc/subgid").read_text().splitlines() if line.startswith("zebratlas:"))
        for path in [destination, *destination.rglob("*")]:
            os.chown(path, uid, gid)
            os.chmod(path, 0o700 if path.is_dir() else 0o600)
    print("Verified private working-store clone ready; immutable evidence retained")


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001 - paths/store contents are not echoed on failure
        raise SystemExit("Private engine clone refused; sensitive details withheld") from None
