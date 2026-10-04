"""Read-only Astra release checks; no model calls, account writes or secret output."""
from __future__ import annotations

import argparse
import json
import urllib.error
import urllib.parse
import urllib.request

MAX_BODY = 2 * 1024 * 1024


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(base: str, path: str, *, forbidden_origin: bool = False, payload=None):
    headers = {"Accept": "application/json" if "/api/" in path else "text/html"}
    if forbidden_origin or payload is not None:
        headers.update({"Origin": "https://unrelated.invalid" if forbidden_origin else base,
                        "Content-Type": "application/json"})
    body = json.dumps(payload or {}).encode() if forbidden_origin or payload is not None else None
    req = urllib.request.Request(base + path, headers=headers, data=body)
    opener = urllib.request.build_opener(NoRedirect)
    try:
        response = opener.open(req, timeout=45)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        body = response.read(MAX_BODY + 1)
        if len(body) > MAX_BODY:
            raise ValueError("response exceeds smoke size ceiling")
        return response.status, response.headers, body


def check_page(code, headers, body):
    if code != 200 or b"zebra" not in body.lower() or b"<html" not in body.lower():
        raise ValueError("selected frontend did not return an HTML page with status 200")
    policy = headers.get("Content-Security-Policy", "")
    for directive in ("'strict-dynamic'", "object-src 'none'", "frame-ancestors 'none'"):
        if directive not in policy:
            raise ValueError("selected frontend is missing a required CSP directive")


def check_root(base, code, headers, body):
    target = urllib.parse.urljoin(base + "/", headers.get("Location", ""))
    if code not in (307, 308) or target != base + "/zebra?lang=en":
        raise ValueError("root did not redirect to the selected English landing")


def check_lookup(code, headers, body):
    if code != 200 or "application/json" not in headers.get("Content-Type", ""):
        raise ValueError("lookup did not return JSON with status 200")
    value = json.loads(body)
    hits = value.get("results") if isinstance(value, dict) else None
    if not isinstance(hits, list) or not any(
        isinstance(hit, dict) and isinstance(hit.get("node"), dict)
        and hit["node"].get("id") and "STXBP1" in str(hit["node"].get("label", "")).upper()
        for hit in hits
    ):
        raise ValueError("lookup has no identifier-backed STXBP1 result")
    if "no-store" not in headers.get("Cache-Control", ""):
        raise ValueError("lookup lacks private cache protection")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    args = parser.parse_args()
    parsed = urllib.parse.urlsplit(args.base)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/"):
        parser.error("base must be an HTTP(S) origin without credentials, path, query or fragment")
    base = args.base.rstrip("/")
    checks = [("selected root redirect", lambda: check_root(base, *request(base, "/")))]
    for path in ("/zebra?lang=en", "/zebra/community?lang=en", "/zebra/about?lang=en", "/zebra/account?lang=en", "/zebra/contribute?lang=en",
                 "/zebra/privacy?lang=en", "/zebra/request-removal?lang=en", "/zebra/imprint?lang=en"):
        checks.append((path, lambda path=path: check_page(*request(base, path))))
    checks.append(("identifier-backed lookup", lambda: check_lookup(*request(base, "/zebra/api/lookup", payload={"q": "STXBP1"}))))

    def denied_origin():
        code, _, _ = request(base, "/zebra/api/search", forbidden_origin=True)
        if code != 403:
            raise ValueError("cross-origin search was not rejected before execution")

    checks.append(("cross-origin search denied", denied_origin))
    failed = 0
    for name, check in checks:
        try:
            check()
        except Exception:  # noqa: BLE001 - no upstream exception text at this boundary
            # Bodies, URLs echoed by upstreams, exception reprs and cookies stay out of logs.
            print(f"FAIL {name}")
            failed += 1
        else:
            print(f"PASS {name}")
    print(f"Astra smoke: {len(checks) - failed}/{len(checks)} passed; no model calls")
    return bool(failed)


if __name__ == "__main__":
    raise SystemExit(main())
