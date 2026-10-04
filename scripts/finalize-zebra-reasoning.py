"""Pin the actual loaded reasoning profile separately from its asserted RDF input."""
import argparse
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen
from urllib.parse import urlencode


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', required=True, type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--rdf-manifest', required=True, type=Path)
    parser.add_argument('--base', default='http://127.0.0.1:3163/api/v1/repositories/nrese')
    args = parser.parse_args()
    p = args.directory.resolve()
    preflight = json.loads((p / 'reasoning.manifest.json').read_text(encoding='utf-8'))
    evidence = []
    for name in ('verified.json', 'verified-mondo.json'):
        path = p / name
        report = json.loads(path.read_text(encoding='utf-8'))
        assert len(report['verified_examples']) >= 3
        for item in report['verified_examples']:
            assert item['asserted'] is False and item['inferred'] is True
            assert item['justification']['verified'] is True
            assert all(x['records'] for x in item['source_records'])
        evidence.append({'path': str(path), 'sha256': digest(path)})
    counts = []
    for infer in ('false', 'true'):
        query = 'SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }'
        with urlopen(args.base + '/sparql?' + urlencode({'query': query, 'infer': infer}), timeout=30) as response:
            counts.append(int(json.load(response)['results']['bindings'][0]['count']['value']))
    assert counts[1] - counts[0] == preflight['expected_derived_triples']
    rdf_manifest = json.loads(args.rdf_manifest.read_text(encoding='utf-8'))
    assert rdf_manifest['rdf_sha256'] == preflight['rdf_sha256']
    rules = Path(__file__).with_name('zebra-hierarchy.n3').resolve()
    checkpoints = sorted((p / 'store').glob('checkpoint-*.nck'))
    assert checkpoints
    profile = {
        'format': 'zebratlas-verified-reasoning-profile-v1', 'public_release': False,
        'runtime_reasoning': True, 'engine': 'nrese', 'mode': 'custom',
        'rule': 'rdfs-subclass-transitivity', 'endpoint': args.base + '/sparql',
        'proof_endpoint': args.base + '/explain',
        'source_rdf': preflight['rdf'], 'source_rdf_sha256': preflight['rdf_sha256'],
        'source_manifest': str(args.rdf_manifest.resolve()), 'source_manifest_sha256': digest(args.rdf_manifest),
        'config': str(p / 'native.toml'), 'config_sha256': digest(p / 'native.toml'),
        'rules': str(rules), 'rules_sha256': digest(rules),
        'binary': str(args.binary.resolve()), 'binary_sha256': digest(args.binary),
        'store': str(p / 'store'), 'checkpoint': str(checkpoints[-1]), 'checkpoint_sha256': digest(checkpoints[-1]),
        'reasoning_state': str(p / 'store' / 'reasoning.state'),
        'reasoning_state_sha256': digest(p / 'store' / 'reasoning.state'),
        'asserted_quads': counts[0], 'derived_quads': counts[1] - counts[0],
        'asserted_hierarchy_triples': preflight['asserted_hierarchy_triples'],
        'preflight': str(p / 'reasoning.manifest.json'), 'preflight_sha256': digest(p / 'reasoning.manifest.json'),
        'verification_receipts': evidence, 'limitations': preflight['limitation'],
    }
    with (p / 'reasoning-profile.json').open('x', encoding='utf-8') as stream:
        json.dump(profile, stream, indent=2)
    print(json.dumps(profile, indent=2))
