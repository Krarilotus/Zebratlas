"""Check actual nrese inferred answers, asserted-only absence and verified proofs."""
import argparse
import json
from urllib.parse import urlencode
from urllib.request import Request, urlopen
from pathlib import Path


def get(url):
    with urlopen(Request(url, headers={'Accept': 'application/json'}), timeout=30) as response:
        return json.load(response)


def ask(base, triple, infer):
    query = 'ASK { ' + ' '.join(triple) + ' }'
    with urlopen(Request(base + '/sparql?' + urlencode({'infer': str(infer).lower()}),
                         data=query.encode(), headers={'Content-Type': 'application/sparql-query',
                                                      'Accept': 'application/sparql-results+json'}), timeout=30) as response:
        return json.load(response)['boolean']


def verify(base, manifest):
    results = []
    for example in manifest['examples'][:3]:
        triple = tuple(example[key] for key in ('subject', 'predicate', 'object'))
        assert ask(base, triple, True), 'expected nrese conclusion missing'
        assert not ask(base, triple, False), 'conclusion already asserted: no actual derivation'
        params = dict(zip(('subj', 'pred', 'obj'), triple))
        proof = get(base + '/explain?' + urlencode(params))
        assert proof['steps'][0]['origin'] == 'inferred'
        assert any(step['origin'] == 'asserted' for step in proof['steps'])
        for step in proof['steps']:
            if step['origin'] == 'inferred':
                assert step['rule'] and step['premises'], 'derived fact lacks rule/premises'
        justification = get(base + '/explain?' + urlencode({**params, 'justifications': 'one'}))
        assert justification['verified'], 'nrese did not verify proof'
        source_records = []
        for step in proof['steps']:
            if step['origin'] != 'asserted':
                continue
            statement = ' '.join('<' + step[key] + '>' for key in ('subject', 'predicate', 'object'))
            query = '''PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX prov: <http://www.w3.org/ns/prov#>
PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#>
PREFIX dcterms: <http://purl.org/dc/terms/>
SELECT ?record ?locator ?sha256 ?source WHERE {
 ?edge rdf:reifies <<( ''' + statement + ''' )>> ; prov:wasDerivedFrom ?record .
 ?record ra:recordLocator ?locator ; ra:sha256 ?sha256 ; dcterms:isPartOf ?source .
} LIMIT 32'''
            records = get(base + '/sparql?' + urlencode({'query': query, 'infer': 'false'}))['results']['bindings']
            # Ontology fixture can omit source metadata; production evidence cannot.
            if not step['subject'].startswith('urn:test:'):
                assert records, 'asserted proof premise lacks source record/hash/locator'
            source_records.append({'statement': step, 'records': records})
        results.append({'triple': example, 'asserted': False, 'inferred': True,
                        'proof': proof, 'justification': justification, 'source_records': source_records})
    return {'engine': 'nrese', 'rule': manifest['rule'], 'verified_examples': results}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', default='http://127.0.0.1:3163/api/v1/repositories/nrese')
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    evidence = verify(args.base, json.loads(args.manifest.read_text(encoding='utf-8')))
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(evidence, stream, indent=2)
    print(f"Verified {len(evidence['verified_examples'])} actual nrese conclusions and proofs")
