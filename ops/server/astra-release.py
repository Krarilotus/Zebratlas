"""Activate only a certified Zebratlas release; restore matched configuration on failure."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request
from pathlib import Path

_backup_spec = importlib.util.spec_from_file_location("astra_backup", Path(__file__).with_name("astra-backup.py"))
_backup_module = importlib.util.module_from_spec(_backup_spec)
_backup_spec.loader.exec_module(_backup_module)
backup = _backup_module.backup

ROOT = Path("/srv/zebratlas")
NGINX = Path("/etc/nginx")
LIMIT = 3584 * 1024 * 1024
CONFIGS = {"compose.yml": ROOT / "app/compose.yml", "release.env": ROOT / "app/release.env",
           "nginx-site.conf": NGINX / "sites-available/zebratlas.org",
           "nginx-ratelimit.conf": NGINX / "conf.d/zebratlas-ratelimit.conf"}


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def pinned(path, expected):
    if path.is_symlink() or not path.is_file() or not re.fullmatch(r"[a-f0-9]{64}", expected or "") or digest(path) != expected:
        raise ValueError("release artifact is not a pinned regular file")


def command(arguments, *, environment=None, timeout=90):
    result = subprocess.run(arguments, env=environment, capture_output=True, timeout=timeout, check=False)
    if result.returncode:
        # Compose can echo env values; upstream logs and exception text never leave this boundary.
        raise ValueError("scoped release command failed")
    return result.stdout


def daemon_environment():
    uid = command(["id", "-u", "zebratlas"]).decode().strip()
    if not uid.isdecimal():
        raise ValueError("invalid Atlas service uid")
    environment = dict(os.environ)
    environment["DOCKER_HOST"] = f"unix:///run/user/{uid}/docker.sock"
    return environment, uid


def compose(environment, config, release, *arguments):
    return command(["docker", "compose", "--project-name", "zebratlas", "--project-directory", str(ROOT / "app"),
                    "-f", str(config), "--env-file", str(release), *arguments], environment=environment, timeout=300)


def check_capacity(capacity):
    if (capacity.get("slice_limit_bytes") != LIMIT or capacity.get("serving_peak_bytes", LIMIT + 1) > LIMIT
            or capacity.get("serving_peak_bytes", 0) <= 0 or capacity.get("observation_seconds", 0) < 60
            or capacity.get("new_oom_events") != 0):
        raise ValueError("release does not fit the approved permanent website budget")


class SliceMonitor:
    """Measure the Atlas parent cgroup, including daemon and kernel memory."""

    def __init__(self, uid, root=Path("/sys/fs/cgroup/user.slice")):
        if not str(uid).isdecimal():
            raise ValueError("invalid Atlas service uid")
        self.directory = root / f"user-{uid}.slice"
        self.peak = 0
        self.failure = False
        self.stopped = threading.Event()
        self.began = time.monotonic()
        self.before = self.read()[1]
        self.worker = threading.Thread(target=self.watch, daemon=True)

    def read(self):
        if int((self.directory / "memory.max").read_text()) != LIMIT:
            raise ValueError("permanent Atlas parent memory limit changed")
        current = int((self.directory / "memory.current").read_text())
        events = dict(line.split() for line in (self.directory / "memory.events").read_text().splitlines())
        return current, {name: int(events.get(name, 0)) for name in ("oom", "oom_kill")}

    def watch(self):
        while not self.stopped.is_set():
            try:
                current, events = self.read()
                self.peak = max(self.peak, current)
                if current > LIMIT or any(events[name] > self.before[name] for name in self.before):
                    self.failure = True
            except Exception:
                self.failure = True
            self.stopped.wait(0.25)

    def start(self):
        self.worker.start()

    def close(self):
        self.stopped.set()
        self.worker.join(timeout=2)

    def receipt(self):
        current, events = self.read()
        self.peak = max(self.peak, current)
        if self.failure or any(events[name] != self.before[name] for name in self.before):
            raise ValueError("actual Atlas parent cgroup exceeded capacity or reported an OOM")
        receipt = {"slice_limit_bytes": LIMIT, "serving_peak_bytes": self.peak,
                   "observation_seconds": time.monotonic() - self.began, "new_oom_events": 0,
                   "measurement": "actual Atlas parent cgroup during startup and serving"}
        check_capacity(receipt)
        return receipt


def verify_live_capacity(monitor):
    """Bounded public fixtures exercise the actual services without a model or account write."""
    pages = ("/zebra?lang=en", "/zebra/community?lang=en", "/zebra/account?lang=en",
             "/zebra/privacy?lang=en", "/zebra/request-removal?lang=en", "/zebra/imprint?lang=en")
    began = time.monotonic()
    for tick in range(13):
        with urllib.request.urlopen("http://127.0.0.1:3210" + pages[tick % len(pages)], timeout=10):
            pass
        with urllib.request.urlopen("http://127.0.0.1:3211/api/stats", timeout=15):
            pass
        request = urllib.request.Request("http://127.0.0.1:3210/zebra/api/lookup", data=b'{"q":"STXBP1"}',
                                         headers={"Content-Type": "application/json", "Origin": "http://127.0.0.1:3210"})
        with urllib.request.urlopen(request, timeout=10) as response:
            if response.status != 200:
                raise ValueError("actual bounded lookup pressure failed")
        if monitor.failure:
            raise ValueError("actual Atlas parent capacity failed during serving")
        time.sleep(max(0, began + (tick + 1) * 5 - time.monotonic()))
    return monitor.receipt()


def validate(directory, environment, uid):
    if directory.is_symlink() or not directory.resolve().is_relative_to(ROOT / "releases/astra"):
        raise ValueError("release must be staged under the dedicated Atlas release directory")
    release_bytes = (directory / "release.json").read_bytes()
    record = json.loads(release_bytes)
    record["_release_hash"] = hashlib.sha256(release_bytes).hexdigest()
    if record.get("format") != "zebratlas-astra-release-v1" or not re.fullmatch(r"[a-f0-9]{40}", record.get("source_commit", "")):
        raise ValueError("missing frozen source release record")
    if not re.fullmatch(r"[a-f0-9]{16}", record.get("data_id", "")):
        raise ValueError("invalid immutable dataset identity")
    if set(record.get("files", {})) != set(CONFIGS) | {"acceptance.json"}:
        raise ValueError("release configuration inventory is incomplete")
    for name, expected in record["files"].items():
        pinned(directory / name, expected)
    dataset = ROOT / "datasets" / record["data_id"]
    pinned(dataset / "MANIFEST.tsv", record["data_manifest_sha256"])
    if record["data_manifest_sha256"][:16] != record["data_id"]:
        raise ValueError("dataset manifest identity mismatch")
    for line in (dataset / "MANIFEST.tsv").read_text().splitlines():
        expected, size, relative = line.split("  ", 2)
        name = Path(relative)
        if name.is_absolute() or ".." in name.parts:
            raise ValueError("unsafe dataset manifest entry")
        artifact = dataset / name
        pinned(artifact, expected)
        if artifact.stat().st_size != int(size):
            raise ValueError("dataset size mismatch")
    pinned(dataset / "cache/runtime/runtime.manifest.json", record["runtime_manifest_sha256"])
    runtime = json.loads((dataset / "cache/runtime/runtime.manifest.json").read_bytes())
    if runtime.get("public_release") is not False:
        raise ValueError("private website dataset expected")
    profile = Path(record["reasoning_profile"]["path"])
    if not profile.resolve().is_relative_to(ROOT / "state/reasoning"):
        raise ValueError("reasoning profile must remain private and scoped")
    pinned(profile, record["reasoning_profile"]["sha256"])
    import_receipt_path = profile.parent / "linux-import.receipt.json"
    pinned(import_receipt_path, record["reasoning_profile"]["import_receipt_sha256"])
    imported = json.loads(import_receipt_path.read_bytes())
    if (imported.get("certified") is not True or imported.get("workers_stopped") is not True
            or imported.get("readonly_reference_clone_boot_verified") is not True
            or imported.get("image") != record["images"]["nrese"]
            or imported.get("rdf_sha256") != runtime["rdf_sha256"]
            or imported.get("reasoning_profile_sha256") != record["reasoning_profile"]["sha256"]):
        raise ValueError("fresh Linux store receipt differs from the deployment")
    working_store = Path(record["reasoning_profile"]["working_store"])
    expected_store = ROOT / "state/reasoning-working" / (record["source_commit"] + "-" + record["data_id"]) / "store"
    if working_store.is_symlink() or working_store.resolve() != expected_store:
        raise ValueError("engine writable clone must stay within its dedicated runtime directory")
    store_files = imported.get("store_files", [])
    if not store_files:
        raise ValueError("Linux import receipt omits the complete store inventory")
    for store in (profile.parent / "store", working_store):
        inventory = set()
        for item in store_files:
            relative = Path(item["file"])
            if relative.is_absolute() or ".." in relative.parts or relative.as_posix() in inventory:
                raise ValueError("unsafe store inventory entry")
            inventory.add(relative.as_posix())
            pinned(store / relative, item["sha256"])
            if (store / relative).stat().st_size != item["size"]:
                raise ValueError("store artifact size mismatch")
        if {p.relative_to(store).as_posix() for p in store.rglob("*") if p.is_file()} != inventory:
            raise ValueError("engine store contains unpinned state")
    reasoning = json.loads(profile.read_bytes())
    if (reasoning.get("format") != "zebratlas-verified-reasoning-profile-v1"
            or reasoning.get("public_release") is not False or reasoning.get("runtime_reasoning") is not True
            or reasoning.get("source_rdf_sha256") != runtime["rdf_sha256"]):
        raise ValueError("reasoning profile does not describe this private projection")
    for key in ("config", "rules", "checkpoint", "reasoning_state", "preflight"):
        relative = Path(reasoning[key]).relative_to("/reasoning")
        if ".." in relative.parts:
            raise ValueError("unsafe reasoning artifact path")
        pinned(profile.parent / relative, reasoning[key + "_sha256"])
    proofs = reasoning.get("verification_receipts", [])
    if len(proofs) < 2:
        raise ValueError("fresh Linux ontology proof receipts required")
    for proof in proofs:
        relative = Path(proof["path"]).relative_to("/reasoning")
        if ".." in relative.parts:
            raise ValueError("unsafe reasoning proof path")
        pinned(profile.parent / relative, proof["sha256"])
    receipt = json.loads((directory / "acceptance.json").read_bytes())
    if (receipt.get("format") != "zebratlas-astra-acceptance-v1" or receipt.get("source_commit") != record["source_commit"]
            or receipt.get("data_id") != record["data_id"] or receipt.get("images") != record.get("images")
            or receipt.get("runtime_manifest_sha256") != record["runtime_manifest_sha256"]
            or receipt.get("reasoning_profile_sha256") != record["reasoning_profile"]["sha256"]):
        raise ValueError("acceptance receipt describes another release")
    if receipt.get("configuration") != {name: record["files"][name] for name in CONFIGS}:
        raise ValueError("capacity and acceptance checks used different runtime configuration")
    required = ("clean_package_build", "native_reader_current", "linux_engine_import", "inference_source_proofs",
                "privacy_withholding", "astra_routes", "account_stub", "guarded_rerun_no_model", "rollback_compatible")
    if any(receipt.get("checks", {}).get(name) is not True for name in required):
        raise ValueError("release acceptance gates remain open")
    check_capacity(receipt.get("capacity", {}))
    current_limit = Path(f"/sys/fs/cgroup/user.slice/user-{uid}.slice/memory.max").read_text().strip()
    if current_limit != str(LIMIT):
        raise ValueError("website slice must retain the approved permanent cap")
    for name, image in record["images"].items():
        if name not in ("server", "web", "nrese") or not re.fullmatch(r"sha256:[a-f0-9]{64}", image):
            raise ValueError("unrecognized release image")
        actual = command(["docker", "image", "inspect", "--format", "{{.Id}}", image], environment=environment).decode().strip()
        if actual != image:
            raise ValueError("certified image is absent from the Atlas daemon")
    if set(record["images"]) != {"server", "web", "nrese"}:
        raise ValueError("matched API, web and private engine images are required")
    binary_hash = command(["docker", "run", "--rm", "--network", "none", "--memory", "64m", "--memory-swap", "64m",
                           "--cpus", "0.25", "--entrypoint", "sha256sum", record["images"]["nrese"],
                           "/usr/local/bin/nrese-server"], environment=environment).decode().split()[0]
    if binary_hash != reasoning["binary_sha256"]:
        raise ValueError("Linux engine image differs from the verified profile")
    configuration = json.loads(compose(environment, directory / "compose.yml", directory / "release.env", "config", "--format", "json"))
    services = configuration.get("services", {})
    if set(services) != {"server", "web", "nrese"}:
        raise ValueError("release may replace only the three Atlas services")
    for name, image in record["images"].items():
        if services[name].get("image") != image or services[name].get("build"):
            raise ValueError("compose is not pinned to certified off-host images")
    required_env = {"ATLAS_OPERATIONAL_MANIFEST": "/data/cache/runtime/operational.manifest.json",
                    "RARE_ATLAS_ATLAS_SNAPSHOT": "/data/cache/runtime/atlas.snapshot",
                    "RARE_ATLAS_GRAPH_SNAPSHOT": "/data/cache/runtime/graph.snapshot",
                    "RARE_ATLAS_MECHANISM_SNAPSHOT": "/data/cache/runtime/mechanism.snapshot",
                    "RARE_ATLAS_MAPPING_DIR": "/data/cache/mappings", "RARE_ATLAS_REQUIRE_IDENTITY_GATE": "1",
                    "ATLAS_QUERY_SCHEMA": "/data/cache/runtime/schema.json",
                    "ATLAS_REASONING_MANIFEST": "/reasoning/reasoning-profile.json",
                    "ATLAS_SPARQL_URL": "http://127.0.0.1:3163/api/v1/repositories/nrese/sparql",
                    "ATLAS_QUERY_ENDPOINT": "http://127.0.0.1:3163/api/v1/repositories/nrese/sparql"}
    if any(str(services["server"].get("environment", {}).get(key)) != value for key, value in required_env.items()):
        raise ValueError("server runtime paths must bind the certified immutable artifacts")
    for key in ("ZEBRA_BACKEND_URL", "ACCOUNTS_API_URL", "CONTRIB_API_URL"):
        if services["web"].get("environment", {}).get(key) != "http://server:8000":
            raise ValueError("selected frontend proxy is not routed to the matched API")
    if services["nrese"].get("network_mode") != "service:server" or services["nrese"].get("ports"):
        raise ValueError("private engine must share only the API loopback namespace")
    if (services["nrese"].get("entrypoint") != ["/usr/local/bin/nrese-server"]
            or services["nrese"].get("command") != ["--config", "/reasoning/native.toml"]):
        raise ValueError("private engine must boot the verified native configuration")
    for name, port in (("server", 3211), ("web", 3210)):
        bindings = services[name].get("ports", [])
        if len(bindings) != 1 or bindings[0].get("host_ip") != "127.0.0.1" or str(bindings[0].get("published")) != str(port):
            raise ValueError("release may bind only the existing Atlas loopback ports")
    for name in ("server", "nrese"):
        mounts = services[name].get("volumes", [])
        if not any(v.get("target") == "/reasoning" and v.get("source") == str(profile.parent) and v.get("read_only") for v in mounts):
            raise ValueError("reasoning artifacts must be mounted immutable")
    if not any(v.get("target") == "/reasoning/store" and v.get("source") == str(working_store) and not v.get("read_only")
               for v in services["nrese"].get("volumes", [])):
        raise ValueError("engine locks and WAL require only its matched private working-store clone")
    if not any(v.get("target") == "/data" and v.get("source") == str(dataset) and v.get("read_only")
               for v in services["server"].get("volumes", [])):
        raise ValueError("serving dataset must be mounted immutable")
    pinned(directory / "release.json", record["_release_hash"])
    return record


def atomic_copy(source, destination, expected):
    pinned(source, expected)
    with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".astra-", delete=False) as stream:
        temporary = Path(stream.name)
        os.chmod(temporary, 0o640)
        stream.write(source.read_bytes())
        stream.flush()
        os.fsync(stream.fileno())
    try:
        pinned(temporary, expected)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def healthy():
    for url in ("http://127.0.0.1:3211/api/health", "http://127.0.0.1:3210/"):
        with urllib.request.urlopen(url, timeout=10) as response:
            if response.status != 200:
                return False
    return True


def wait_healthy():
    deadline = time.monotonic() + 240
    while time.monotonic() < deadline:
        try:
            if healthy():
                return
        except Exception:  # noqa: BLE001,S110 - no HTTP response or URL detail is logged
            pass
        time.sleep(2)
    raise ValueError("website failed bounded readiness gate")


def verify_live_engine(environment, record):
    container = compose(environment, CONFIGS["compose.yml"], CONFIGS["release.env"], "ps", "-q", "server").decode().strip()
    if not re.fullmatch(r"[a-f0-9]{64}", container):
        raise ValueError("matched server container is unavailable")
    profile = Path(record["reasoning_profile"]["path"])
    preflight = json.loads((profile.parent / "reasoning.manifest.json").read_bytes())
    example = preflight["examples"][0]
    triple = " ".join(example[key] for key in ("subject", "predicate", "object"))
    base = "http://127.0.0.1:3163/api/v1/repositories/nrese"

    def get(path, body=None):
        args = ["docker", "exec", container, "curl", "--fail", "--silent", "--max-time", "15"]
        if body is not None:
            args += ["-H", "Content-Type: application/sparql-query", "--data-binary", body]
        return json.loads(command([*args, base + path], environment=environment, timeout=20))

    query = "ASK { " + triple + " }"
    if get("/sparql?infer=true", query).get("boolean") is not True or get("/sparql?infer=false", query).get("boolean") is not False:
        raise ValueError("live engine did not preserve the certified asserted/inferred distinction")
    parameters = dict(zip(("subj", "pred", "obj"), (example[key] for key in ("subject", "predicate", "object"))))
    proof = get("/explain?" + urllib.parse.urlencode(parameters))
    if not proof.get("steps") or proof["steps"][0].get("origin") != "inferred" or not any(s.get("origin") == "asserted" for s in proof["steps"]):
        raise ValueError("live inference lacks actual asserted premises")
    if get("/explain?" + urllib.parse.urlencode(parameters | {"justifications": "one"})).get("verified") is not True:
        raise ValueError("live engine justification verification failed")


def activate(directory, record, environment, uid):
    previous = backup(ROOT, NGINX)
    for name in CONFIGS:
        if not (previous / name).is_file():
            raise ValueError("matched rollback configuration is missing")
    monitor = SliceMonitor(uid)
    monitor.start()
    try:
        for name, destination in CONFIGS.items():
            atomic_copy(directory / name, destination, record["files"][name])
        command(["nginx", "-t"])
        compose(environment, CONFIGS["compose.yml"], CONFIGS["release.env"], "up", "-d", "--remove-orphans")
        wait_healthy()
        verify_live_engine(environment, record)
        capacity = verify_live_capacity(monitor)
        command(["systemctl", "reload", "nginx"])
        command(["python3", str(Path(__file__).resolve().parents[1] / "zebra_smoke.py"), "--base", "https://zebratlas.org"], timeout=300)
        capacity = monitor.receipt() | {"serving_pressure_seconds": capacity["observation_seconds"]}
        capacity.update({"source_commit": record["source_commit"], "data_id": record["data_id"], "images": record["images"]})
        (directory / "live-capacity.receipt.json").write_text(json.dumps(capacity, indent=2) + "\n")
        os.chmod(directory / "live-capacity.receipt.json", 0o600)
        atomic_copy(directory / "release.json", ROOT / "app/release.json", record["_release_hash"])
        return previous
    except Exception:  # noqa: BLE001 - rollback always precedes a redacted failure
        monitor.close()
        for name, destination in CONFIGS.items():
            atomic_copy(previous / name, destination, digest(previous / name))
        command(["nginx", "-t"])
        compose(environment, CONFIGS["compose.yml"], CONFIGS["release.env"], "up", "-d", "--remove-orphans")
        wait_healthy()
        command(["systemctl", "reload", "nginx"])
        raise ValueError("candidate failed; previous matched configuration restored and healthy") from None
    finally:
        monitor.close()


def main():
    import fcntl
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("release", type=Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError("root session required")
    environment, uid = daemon_environment()
    with open("/run/zebratlas-release.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        record = validate(args.release, environment, uid)
        if not args.apply:
            print("Certified Astra release dry run passed; no service or permanent budget changed")
            return
        previous = activate(args.release, record, environment, uid)
    print(f"Astra release healthy; matched rollback backup: {previous}")


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001 - subprocess/API/account values never enter logs
        raise SystemExit("Astra release stopped or rolled back; sensitive details withheld") from None
