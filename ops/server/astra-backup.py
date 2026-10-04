"""Consistent private SQLite/config backup for the Zebratlas rollout only."""
import argparse
import datetime
import hashlib
import json
import os
import shutil
import sqlite3
from contextlib import closing
from pathlib import Path


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def backup(root, nginx_root):
    if root.is_symlink() or nginx_root.is_symlink():
        raise ValueError("backup roots must not be symlinks")
    stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%dT%H%M%S%fZ")
    destination = root / "backups/rollout" / stamp
    destination.mkdir(parents=True, mode=0o700)
    os.chmod(destination.parent, 0o700)
    inputs = {"compose.yml": root / "app/compose.yml", "release.env": root / "app/release.env",
              "release.env.prev": root / "app/release.env.prev", "release.json": root / "app/release.json",
              "nginx-site.conf": nginx_root / "sites-available/zebratlas.org",
              "nginx-ratelimit.conf": nginx_root / "conf.d/zebratlas-ratelimit.conf"}
    for name, source in inputs.items():
        if source.exists():
            if source.is_symlink() or not source.is_file():
                raise ValueError("backup config must be a regular file")
            shutil.copy2(source, destination / name)
            os.chmod(destination / name, 0o600)
    app = root / "state/app"
    for source in app.glob("*.sqlite"):
        if source.is_symlink() or not source.is_file():
            raise ValueError("account storage must be a regular file")
        target = destination / source.name
        with closing(sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)) as original, closing(sqlite3.connect(target)) as copied:
            original.backup(copied)
            if copied.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
                raise ValueError("private SQLite backup failed integrity checking")
        os.chmod(target, 0o600)
    receipt = {"format": "zebratlas-rollout-backup-v1", "observed_at": stamp,
               "secret_file_copied": False, "database_restore": "Only if required; review post-cutover writes first",
               "files": [{"file": p.name, "sha256": digest(p)} for p in sorted(destination.iterdir()) if p.is_file()]}
    (destination / "backup.json").write_text(json.dumps(receipt, indent=2) + "\n")
    os.chmod(destination / "backup.json", 0o600)
    return destination


def main():
    import fcntl
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError("root SSH session required")
    if not args.apply:
        print("DRY RUN: private Zebratlas configuration/SQLite online backup; no service restart")
        return
    with open("/run/zebratlas-release.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        directory = backup(Path("/srv/zebratlas"), Path("/etc/nginx"))
    print(f"Private rollout backup verified: {directory}")


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001 - private database details must never enter logs
        raise SystemExit("Private rollout backup failed; details withheld") from None
