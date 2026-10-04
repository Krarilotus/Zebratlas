"""Certify a fresh Linux private hierarchy store using an already certified engine image."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def run(args, output=None, timeout=300):
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    if output:
        with output.open("wb") as stream:
            result = subprocess.run(args, stdout=stream, stderr=subprocess.STDOUT, timeout=timeout, creationflags=flags, check=False)
    else:
        result = subprocess.run(args, capture_output=True, timeout=timeout, creationflags=flags, check=False)
    if result.returncode:
        raise ValueError("owned Linux certification command failed; private logs retained")
    return getattr(result, "stdout", b"")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--scripts", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--image", required=True)
    parser.add_argument("--python-image", default="python:3.13-slim-trixie")
    parser.add_argument("--reuse-preflight", type=Path, help="Reuse this recipe's completed matching RDF/rule preflight; never reuse a store")
    args = parser.parse_args()
    if not re.fullmatch(r"sha256:[a-f0-9]{64}", args.image):
        raise ValueError("certified immutable engine image required")
    data, scripts, output = args.data.resolve(), args.scripts.resolve(), args.output.resolve()
    if output.exists() or output.is_relative_to(data):
        raise ValueError("reasoning certification requires a new private directory")
    manifest = json.loads((data / "cache/runtime/operational.manifest.json").read_bytes())
    rdf = data / "cache/runtime/operational.ttl"
    if manifest.get("public_release") is not False or digest(rdf) != manifest["rdf_sha256"]:
        raise ValueError("private matched operational input required")
    platform = run(["docker", "image", "inspect", "--format", "{{.Os}}/{{.Architecture}}", args.image]).decode().strip()
    if platform != "linux/amd64":
        raise ValueError("certified Linux amd64 engine required")
    output.mkdir(parents=True)
    for name in ("prepare-zebra-reasoning.py", "verify-zebra-reasoning.py", "finalize-zebra-reasoning.py", "zebra-hierarchy.n3"):
        shutil.copy2(scripts / name, output / name)
    mounts = ["--mount", f"type=bind,src={data},dst=/data,readonly", "--mount", f"type=bind,src={output},dst=/reasoning"]
    python = ["docker", "run", "--rm", "--memory", "512m", "--memory-swap", "512m", "--cpus", "1", *mounts,
              args.python_image, "python"]
    if args.reuse_preflight:
        previous = args.reuse_preflight.resolve()
        checked = json.loads((previous / "reasoning.manifest.json").read_bytes())
        if ((previous / "store").exists() or checked["rdf_sha256"] != manifest["rdf_sha256"]
                or checked["rules_sha256"] != digest(output / "zebra-hierarchy.n3")
                or (previous / "prepare-zebra-reasoning.py").read_bytes() != (output / "prepare-zebra-reasoning.py").read_bytes()):
            raise ValueError("preflight reuse requires the same RDF/current rules and no prior loaded store")
        for filename in ("native.toml", "reasoning.manifest.json"):
            shutil.copy2(previous / filename, output / filename)
    else:
        run(python + ["/reasoning/prepare-zebra-reasoning.py", "--rdf", "/data/cache/runtime/operational.ttl", "--output", "/reasoning"],
            output / "preflight.log")
    mondo_preflight = """import importlib.util,json
from pathlib import Path
spec=importlib.util.spec_from_file_location('prepare','/reasoning/prepare-zebra-reasoning.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
edges=set()
with Path('/data/cache/runtime/operational.ttl').open() as stream:
 for line in stream:
  match=module.TRIPLE.match(line)
  if match:edges.add(match.groups())
added=module.closure(edges)
report=json.loads(Path('/reasoning/reasoning.manifest.json').read_text())
assert len(added)==report['expected_derived_triples']
report['examples']=[dict(subject=s,predicate='<'+module.PRED+'>',object=o) for s,o in sorted(added)
                    if 'MONDO' in s.upper() or 'MONDO' in o.upper()][:10]
assert len(report['examples'])>=3
Path('/reasoning/reasoning-mondo.manifest.json').write_text(json.dumps(report))
"""
    run(python + ["-c", mondo_preflight], output / "mondo-preflight.log")
    text = (output / "native.toml").read_text()
    load_config = re.sub(r'^rules = .*\n', '', text.replace('mode = "custom"', 'mode = "disabled"'), flags=re.MULTILINE)
    (output / "load.toml").write_text(load_config)
    name = "zebratlas-private-check-" + hashlib.sha256(str(output).encode()).hexdigest()[:12]
    common = ["--memory", "2g", "--memory-swap", "2g", "--cpus", "1", "--pids-limit", "128",
              "--security-opt", "no-new-privileges:true", "--cap-drop", "ALL", "--env", "RAYON_NUM_THREADS=1", *mounts]
    run(["docker", "run", "--rm", "--network", "none", *common, "--env", "NRESE_CONFIG_PATH=/reasoning/load.toml",
         "--entrypoint", "/usr/local/bin/nrese-server", args.image, "load", "/data/cache/runtime/operational.ttl"], output / "load.log", timeout=600)
    log = re.sub(r"\x1b\[[0-9;]*m", "", (output / "load.log").read_text())
    rows = re.findall(r"bulk load complete[^\r\n]*", log)
    if len(rows) != 1:
        raise ValueError("exactly one successful serial import required")
    counts = {key: int(value) for key, value in re.findall(r"\b(parsed|inserted|skipped)=(\d+)\b", rows[0])}
    if counts.get("skipped") != 0 or counts.get("parsed", 0) <= 0 or counts.get("inserted", 0) <= 0:
        raise ValueError("private Linux import incomplete or skipped statements")
    started = False
    try:
        run(["docker", "run", "-d", "--name", name, *common, "--env", "NRESE_CONFIG_PATH=/reasoning/native.toml",
             "--entrypoint", "/usr/local/bin/nrese-server", args.image, "--config", "/reasoning/native.toml"])
        started = True
        # The native service binds its private namespace loopback; no host port is published.
        probe = ["docker", "run", "--rm", "--network", "container:" + name, "--memory", "256m", "--memory-swap", "256m",
                 "--cpus", "0.25", *mounts, args.python_image, "python"]
        ready = """import time,urllib.request
for _ in range(120):
 try:
  urllib.request.urlopen('http://127.0.0.1:3163/readyz',timeout=2).close();break
 except Exception:time.sleep(1)
else:raise SystemExit('private engine readiness failed')
"""
        run(probe + ["-c", ready], output / "ready.log", timeout=150)
        for source, target in (("reasoning.manifest.json", "verified.json"), ("reasoning-mondo.manifest.json", "verified-mondo.json")):
            run(probe + ["/reasoning/verify-zebra-reasoning.py", "--manifest", "/reasoning/" + source,
                         "--output", "/reasoning/" + target], output / (target + ".log"))
        run(["docker", "cp", name + ":/usr/local/bin/nrese-server", str(output / "nrese-server")])
        run(probe + ["/reasoning/finalize-zebra-reasoning.py", "--directory", "/reasoning", "--binary", "/reasoning/nrese-server",
                     "--rdf-manifest", "/data/cache/runtime/operational.manifest.json"], output / "finalize.log")
        if digest(rdf) != manifest["rdf_sha256"]:
            raise ValueError("RDF changed during Linux certification")
    finally:
        if started:
            run(["docker", "stop", "--time", "10", name])
            run(["docker", "rm", name])
    profile = json.loads((output / "reasoning-profile.json").read_bytes())
    for key in ("config", "rules", "checkpoint", "reasoning_state", "preflight", "binary"):
        artifact = output / Path(profile[key]).relative_to("/reasoning")
        if digest(artifact) != profile[key + "_sha256"]:
            raise ValueError("reasoning artifacts changed after the owned engine stopped")
    if profile["asserted_quads"] != counts["inserted"]:
        raise ValueError("serving store count differs from successful Linux load")
    # Engine LOCK/WAL writes belong to a separate runtime clone, while reference artifacts stay read-only.
    trial = output / "working-trial/store"
    shutil.copytree(output / "store", trial, copy_function=shutil.copy2)
    immutable_mounts = ["--mount", f"type=bind,src={data},dst=/data,readonly",
                        "--mount", f"type=bind,src={output},dst=/reasoning,readonly",
                        "--mount", f"type=bind,src={trial},dst=/reasoning/store"]
    readonly_name = name + "-readonly"
    readonly_started = False
    try:
        run(["docker", "run", "-d", "--name", readonly_name, "--memory", "512m", "--memory-swap", "512m", "--cpus", "1",
             "--read-only", "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m", *immutable_mounts,
             "--entrypoint", "/usr/local/bin/nrese-server", args.image, "--config", "/reasoning/native.toml"])
        readonly_started = True
        readonly_probe = ["docker", "run", "--rm", "--network", "container:" + readonly_name,
                          "--memory", "256m", "--memory-swap", "256m", "--cpus", "0.25", *mounts, args.python_image, "python"]
        run(readonly_probe + ["-c", ready], output / "readonly-ready.log", timeout=150)
        for source, target in (("reasoning.manifest.json", "readonly-verified.json"), ("reasoning-mondo.manifest.json", "readonly-verified-mondo.json")):
            run(readonly_probe + ["/reasoning/verify-zebra-reasoning.py", "--manifest", "/reasoning/" + source,
                                  "--output", "/reasoning/" + target], output / (target + ".log"))
    finally:
        if readonly_started:
            run(["docker", "stop", "--time", "10", readonly_name])
            run(["docker", "rm", readonly_name])
    for key in ("checkpoint", "reasoning_state"):
        if digest(output / Path(profile[key]).relative_to("/reasoning")) != profile[key + "_sha256"]:
            raise ValueError("engine runtime clone changed immutable reference artifacts")
    receipt = {"format": "zebratlas-linux-private-import-v1", "public_release": False, "image": args.image,
               "rdf_sha256": manifest["rdf_sha256"], "counts": counts, "workers_stopped": True,
               "recipe_sha256": digest(Path(__file__)), "readonly_reference_clone_boot_verified": True,
               "reasoning_profile_sha256": digest(output / "reasoning-profile.json"), "certified": True,
               "store_files": [{"file": p.relative_to(output / "store").as_posix(), "sha256": digest(p), "size": p.stat().st_size}
                               for p in sorted((output / "store").rglob("*")) if p.is_file()]}
    (output / "linux-import.receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print("Fresh private Linux import, hierarchy proofs and source chains verified")


if __name__ == "__main__":
    try:
        main()
    except Exception:  # noqa: BLE001 - only fixed summaries leave private logs
        raise SystemExit("Private Linux certification stopped; owned logs retained without printing data") from None
