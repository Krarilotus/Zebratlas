"""Preflight an isolated nrese hierarchy store; never enable an unchecked closure.

Input must be the line-oriented private operational projection, not arbitrary Turtle.
The original RDF/provenance is loaded unchanged. This script does not infer clinical facts.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

PRED = "http://www.w3.org/2000/01/rdf-schema#subClassOf"
TRIPLE = re.compile(r'^\s*(<[^>]+>)\s+(?:rdfs:subClassOf|<' + re.escape(PRED) + r'>)\s+(<[^>]+>)\s*(?:\.|~)')


def closure(edges, max_added=300_000, max_ancestors=4096):
    direct = {}
    for child, parent in edges:
        direct.setdefault(child, set()).add(parent)
    added = set()
    for child in sorted(direct):
        seen, todo = set(), list(direct[child])
        while todo:
            parent = todo.pop()
            if parent in seen:
                continue
            seen.add(parent)
            if len(seen) > max_ancestors:
                raise ValueError("hierarchy ancestor budget exceeded")
            todo.extend(direct.get(parent, set()) - seen)
        added.update((child, parent) for parent in seen - direct[child])
        if len(added) > max_added:
            raise ValueError("hierarchy conclusion budget exceeded")
    return added


def prepare(rdf, output, port=3163):
    edges = set()
    digest = hashlib.sha256()
    with rdf.open('rb') as stream:
        for line in stream:
            digest.update(line)
            match = TRIPLE.match(line.decode('utf-8'))
            if match:
                edges.add(match.groups())
    if not edges:
        raise ValueError("no verified direct rdfs:subClassOf assertions in the operational projection")
    added = closure(edges)
    if not added:
        raise ValueError("hierarchy has no new conclusions: reasoning would not add results")
    output.mkdir(parents=True, exist_ok=True)
    rules = Path(__file__).with_name('zebra-hierarchy.n3').resolve()
    config = output / 'native.toml'
    text = f'''[server]
bind_address = "127.0.0.1:{port}"
deployment_posture = "read-only-demo"
[store]
mode = "on-disk"
data_dir = {json.dumps((output / 'store').resolve().as_posix())}
map_checkpoints = true
[reasoner]
mode = "custom"
rules = {json.dumps(rules.as_posix())}
[budgets]
query_memory = "256MiB"
total_query_memory = "512MiB"
bulk_load_memory = "512MiB"
query_timeout = "3s"
query_text = "64KiB"
upload_size = "8MiB"
result_cache = "64MiB"
[policy.exposure]
operator_ui = false
metrics = false
[auth]
mode = "none"
local_logins = false
[ai]
enabled = false
[federation]
allow = []
'''
    with config.open('x', encoding='utf-8') as stream:
        stream.write(text)
    report = {
        'format': 'zebratlas-hierarchy-reasoning-v1', 'runtime_reasoning': True,
        'reasoning_engine': 'nrese', 'reasoning_profile': 'custom',
        'rule': 'rdfs-subclass-transitivity', 'rdf': str(rdf.resolve()),
        'rdf_sha256': digest.hexdigest(), 'rules_sha256': hashlib.sha256(rules.read_bytes()).hexdigest(),
        'asserted_hierarchy_triples': len(edges), 'expected_derived_triples': len(added),
        'max_added': 300_000, 'max_ancestors': 4096,
        'examples': [dict(subject=s, predicate=f'<{PRED}>', object=o) for s, o in sorted(added)[:10]],
        'config': str(config.resolve()), 'proof_required': True,
        'limitation': 'Ontology hierarchy only; no clinical, mechanism, gene-association or eligibility inference.',
    }
    with (output / 'reasoning.manifest.json').open('x', encoding='utf-8') as stream:
        json.dump(report, stream, indent=2)
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rdf', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--port', type=int, default=3163)
    args = parser.parse_args()
    print(json.dumps(prepare(args.rdf, args.output, args.port), indent=2))
