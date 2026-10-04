import assert from 'node:assert/strict';
import { visualizeQuery } from '../public/query-by-graph/query-parser.js';
const query = `PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT DISTINCT ?s ?label WHERE {
 VALUES ?gene { <https://w3id.org/rare-disease-atlas/id/HGNC%3A888> }
 ?gene ra:gene_associated_with_condition ?s .
 ?s ra:nodeKind "disease" .
 OPTIONAL { ?s rdfs:label ?label }
 FILTER(?s != ?gene)
} ORDER BY ?s LIMIT 20`;
const graph = visualizeQuery(query);
assert.equal(graph.edges.length, 2);
assert.equal(graph.nodes.length, 3);
assert(graph.nodes.some(node => node.label === '?s · disease'));
assert(graph.edges.some(edge => edge.relation === 'OPTIONAL · label'));
assert(graph.edges.some(edge => edge.relation === 'gene_associated_with_condition'));
assert.equal(graph.settings.limit, 20);
assert.throws(() => visualizeQuery('SELECT ?s WHERE { { ?s <urn:p> ?o } UNION { ?s <urn:q> ?o } }'));
assert.throws(() => visualizeQuery('SELECT ?s WHERE { SERVICE <https://example.org/sparql> { ?s ?p ?o } }'));
assert.throws(() => visualizeQuery('SELECT ?s WHERE { ?s <urn:p>* ?o }'));
assert.throws(() => visualizeQuery('ASK { ?s ?p ?o }'));
assert.throws(() => visualizeQuery('this is not SPARQL'));
console.log('PASS: actual executed SELECT patterns/OPTIONAL/types/bindings/limit; unsupported operator guards');
