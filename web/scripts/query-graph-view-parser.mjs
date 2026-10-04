// Reuse the checked Query-by-Graph integration's actual SPARQL AST adapter.
import { parseQuery } from '@zebratlas/qbg-sparql-model';
export { parseQuery, inspectQuery, addConnection, removeElement, setQueryLimit, setNeighborhoodRelations } from '@zebratlas/qbg-sparql-model';
export function visualizeQuery(sparql) {
  const parsed = parseQuery(sparql);
  return {
    ...parsed.graph,
    nodes: parsed.graph.nodes.map(node => ({ ...node, label: node.kind ? `${node.label} · ${node.kind}` : node.label })),
    edges: parsed.graph.edges.map(edge => ({ ...edge, relation: `${edge.optional ? 'OPTIONAL · ' : ''}${edge.relation.replace(/^.*[#/]/, '')}` })),
  };
}
