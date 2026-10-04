"""Derive the new account settings from the existing private Resend configuration."""
import argparse
import os
import re
import stat
import tempfile
from pathlib import Path


def configure(path, apply=False):
    if path.is_symlink() or not path.is_file():
        raise ValueError("existing regular secrets file required")
    info = path.stat()
    if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o600:
        raise ValueError("root-owned 0600 secrets file required")
    text = path.read_text()
    entries = {}
    for line in text.splitlines():
        match = re.fullmatch(r"\s*(?:export\s+)?([A-Z][A-Z0-9_]*)\s*=\s*(.*?)\s*", line)
        if match:
            name, value = match.groups()
            if name in entries:
                raise ValueError("duplicate environment entry")
            entries[name] = value.strip("\"'")
    if not entries.get("RESEND_API_KEY") or entries.get("RESEND_FROM") != "Zebratlas <noreply@auth.zebratlas.org>":
        raise ValueError("existing authorized Resend settings required")
    if not apply:
        return
    changes = {"ATLAS_ACCOUNT_EMAIL_FROM": '"Zebratlas <noreply@auth.zebratlas.org>"',
               "ATLAS_ACCOUNT_PUBLIC_ORIGIN": "https://zebratlas.org"}
    lines = [line for line in text.splitlines() if not re.match(r"^\s*(?:export\s+)?(?:ATLAS_ACCOUNT_EMAIL_FROM|ATLAS_ACCOUNT_PUBLIC_ORIGIN)\s*=", line)]
    fd, temporary = tempfile.mkstemp(prefix=".account-config-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write("\n".join(lines) + "\n" + "\n".join(f"{key}={value}" for key, value in changes.items()) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    import fcntl
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    with open("/run/zebratlas-release.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        configure(Path("/etc/zebratlas/zebratlas.env"), args.apply)
    print("Account email configuration " + ("installed; restart pending" if args.apply else "verified; dry run"))


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001 - redact everything at the secret boundary
        raise SystemExit("Account email configuration failed; details withheld") from None
