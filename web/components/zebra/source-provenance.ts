import type { ProvChain } from "@/lib/types";
import type { GraphEdge, GraphEvidence } from "@/lib/zebra/types";

/** adapt.provenance keeps the untouched envelope separately from its source arrays. */
export function originalProvenance(chain: ProvChain): unknown {
  const envelope = chain.raw && typeof chain.raw === "object" && !Array.isArray(chain.raw) ? chain.raw : {};
  return { ...envelope, entities: chain.entities, activities: chain.activities };
}

/** Preserve actual flattened source metadata without inventing assertion semantics. */
export function graphSourceRecord(evidence: GraphEvidence) {
  const raw = evidence as GraphEvidence & Record<string, unknown>;
  const text = (...values: unknown[]) => values.find((value): value is string => typeof value === "string" && !!value);
  return {
    ...evidence,
    url: text(raw.url, raw.source_url),
    record: text(raw.record, raw.record_locator),
    retrieved_at: text(raw.retrieved_at, raw.fetched_at),
    sha256: text(raw.sha256, raw.source_sha256, raw.source_hash),
    version: text(raw.version, raw.source_version),
    sha256_scope: text(raw.sha256_scope),
    generated_by: text(raw.generated_by),
    assertion: text(raw.assertion),
    kind: text(raw.kind),
    references: Array.isArray(raw.references) ? raw.references.filter((value): value is string => typeof value === "string") : undefined,
  };
}

/** Runtime proofs have returned premises, rather than persisted graph provenance IDs. */
export function sourceProvenanceIds(assertions: GraphEdge[], edgeIds: string[], entityId?: string, conditionId?: string): string[] {
  const runtime = new Set(assertions.filter(assertion => assertion.runtime_reasoning === true || (assertion.proof && typeof assertion.proof === "object" && "engine" in assertion.proof && assertion.proof.engine === "nrese")).map(assertion => assertion.id));
  return [...new Set([...edgeIds, ...assertions.map(assertion => assertion.id), ...(entityId ? [entityId] : []), ...(conditionId ? [conditionId] : [])])].filter(id => !runtime.has(id));
}
