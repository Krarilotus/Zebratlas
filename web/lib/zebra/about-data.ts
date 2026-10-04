type ObjectValue = Record<string, unknown>;
// Verified against the local OWL-RS repository's git remote origin.
export const NRESE_REPOSITORY_URL = "https://github.com/Krarilotus/OWL-RS";
const object = (value: unknown): ObjectValue => value && typeof value === "object" && !Array.isArray(value) ? value as ObjectValue : {};
export function metadataCount(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}
/** Presence of one field only; marginal counts do not establish an intersection. */
export function metadataPresent(total: unknown, missing: unknown): number | undefined {
  const denominator = metadataCount(total), absent = metadataCount(missing);
  return denominator === undefined || absent === undefined || absent > denominator ? undefined : denominator - absent;
}
function counts(value: unknown): Record<string, number> {
  return Object.fromEntries(Object.entries(object(value)).flatMap(([key, raw]) => {
    const count = metadataCount(raw); return count === undefined ? [] : [[key, count]];
  }));
}
function text(value: unknown): string | undefined { return typeof value === "string" && value.trim() ? value.trim() : undefined; }
function hash(value: unknown): string | undefined { const raw = text(value); return raw && /^[a-f0-9]{64}$/i.test(raw) ? raw : undefined; }
export function metadataUrl(value: unknown): string | undefined {
  const raw = text(value); if (!raw) return undefined;
  try { const url = new URL(raw); return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : undefined; } catch { return undefined; }
}
function source(value: unknown) {
  const item = object(value);
  const date = text(item.retrieved_at);
  return { id: text(item.id), name: text(item.label) ?? text(item.name), url: metadataUrl(item.url), version: text(item.version), sha256: hash(item.sha256),
    retrievedAt: date && /^\d{4}-\d{2}-\d{2}T/.test(date) && Number.isFinite(Date.parse(date)) ? date : undefined };
}
function proofMeasure(value: unknown, key: string) {
  const row = object(value);
  return { key, total: metadataCount(row.total), missingUrl: metadataCount(row.missing_url ?? row.missing_effective_url),
    missingHash: metadataCount(row.missing_sha256 ?? row.zero_sha256), missingDate: metadataCount(row.missing_retrieved_at ?? row.missing_effective_retrieved_at) };
}
export function aboutMetadata(raw: unknown) {
  const schema = object(raw), data = object(schema.data), loaded = object(data.counts), visible = object(data.serving_visible), withheld = object(data.withheld);
  const provenance = object(data.provenance), execution = object(schema.execution), adapter = object(execution.dataset);
  const classCounts: Record<string, number> = {};
  if (Array.isArray(schema.classes)) for (const entry of schema.classes) {
    const item = object(entry), kind = text(item.kind), count = metadataCount(item.count);
    if (kind && count !== undefined) classCounts[kind] = count;
  }
  const visibleKinds = counts(visible.connected_nodes_by_kind);
  const visibleTotal = Object.keys(visibleKinds).length ? Object.values(visibleKinds).reduce((sum, count) => sum + count, 0) : undefined;
  const measures = ["atlas_sources", "connected_sources", "connected_records"].map(key => proofMeasure(provenance[key], key));
  return {
    scope: text(data.scope), atlas: counts(schema.atlas), classes: classCounts,
    loaded: { totalNodes: metadataCount(loaded.total_nodes), nodes: counts(loaded.nodes_by_kind), connectedEdges: metadataCount(loaded.connected_edges),
      geneEdges: metadataCount(loaded.atlas_gene_edges), phenotypeEdges: metadataCount(loaded.atlas_phenotype_edges), absentPhenotypeEdges: metadataCount(loaded.atlas_absent_phenotype_edges), evidence: counts(loaded.connected_edge_kinds ?? loaded.edge_kinds) },
    visible: { totalNodes: visibleTotal, nodes: visibleKinds, connectedEdges: metadataCount(visible.connected_edges), records: metadataCount(visible.referenced_records) ?? metadataCount(object(visible.referenced_records).total), sources: metadataCount(visible.referenced_sources) ?? metadataCount(object(visible.referenced_sources).total), evidence: counts(visible.connected_edge_kinds ?? visible.edge_kinds),
      recordsProof: proofMeasure(visible.referenced_records, "connected_records"), sourcesProof: proofMeasure(visible.referenced_sources, "connected_sources") },
    withheld: { nodes: metadataCount(withheld.connected_nodes), edges: metadataCount(withheld.connected_edges), records: metadataCount(withheld.connected_records) },
    measures, sources: Array.isArray(data.sources) ? data.sources.map(source).filter(item => item.name || item.url) : [],
    limits: counts(schema.limits), sparqlConfigured: typeof execution.sparql_configured === "boolean" ? execution.sparql_configured : undefined,
    // This status is never upgraded because a URL is merely configured.
    equivalence: text(execution.store_snapshot_equivalence),
    adapter: { rdfSha256: hash(adapter.rdf_sha256), graphSha256: hash(adapter.graph_snapshot_sha256), atlasSha256: hash(adapter.atlas_snapshot_sha256),
      runtimeReasoning: typeof adapter.runtime_reasoning === "boolean" ? adapter.runtime_reasoning : undefined, visibility: text(adapter.visibility) },
    limitations: Array.isArray(data.limitations) ? data.limitations.filter((value): value is string => typeof value === "string") : [],
  };
}
