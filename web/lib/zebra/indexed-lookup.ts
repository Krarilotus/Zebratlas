export type IndexedLookupMatch = { id: string; label: string; kind: string; match: "exact" | "fuzzy"; score?: number };
export type IndexedLookup = { query: string; corrected_query?: string; mode: "indexed_name_lookup"; model_calls: 0; engine: "indexed-atlas"; matches: IndexedLookupMatch[] };

/** Only short typed names may automatically enter the model-free correction path. */
export function isShortIndexedName(query: string): boolean {
  const name = query.trim();
  return name.length > 0 && name.length <= 80 && name.split(/\s+/u).length <= 4 && /^[\p{L}\p{N}\s:._-]+$/u.test(name);
}

/** Admit only the backend's explicit model-free lookup contract and real returned identifiers. */
export function normalizeIndexedLookup(value: unknown): IndexedLookup {
  if (!value || typeof value !== "object") throw new Error("invalid_indexed_lookup_response");
  const data = value as Record<string, unknown>;
  if (data.mode !== "indexed_name_lookup" || data.model_calls !== 0 || data.engine !== "indexed-atlas" || typeof data.query !== "string" || !Array.isArray(data.matches) || data.matches.length > 10) throw new Error("invalid_indexed_lookup_response");
  const ids = new Set<string>();
  const matches = data.matches.map(raw => {
    if (!raw || typeof raw !== "object") throw new Error("invalid_indexed_lookup_response");
    const match = raw as Record<string, unknown>;
    if (typeof match.id !== "string" || !match.id.trim() || match.id.length > 512 || typeof match.label !== "string" || !match.label.trim() || match.label.length > 500 || typeof match.kind !== "string" || !["exact", "fuzzy"].includes(String(match.match)) || (match.score !== undefined && (typeof match.score !== "number" || !Number.isFinite(match.score))) || ids.has(match.id)) throw new Error("invalid_indexed_lookup_response");
    ids.add(match.id);
    return match as IndexedLookupMatch;
  });
  if (data.corrected_query != null && (typeof data.corrected_query !== "string" || data.corrected_query.length > 512)) throw new Error("invalid_indexed_lookup_response");
  return { query: data.query, ...(typeof data.corrected_query === "string" ? { corrected_query: data.corrected_query } : {}), mode: "indexed_name_lookup", model_calls: 0, engine: "indexed-atlas", matches };
}

export async function lookupIndexedNames(query: string, signal?: AbortSignal): Promise<IndexedLookup> {
  if (!query.trim() || query.length > 512) throw new Error("invalid_indexed_lookup_query");
  const response = await fetch("/zebra/api/lookup", { method: "POST", credentials: "same-origin", cache: "no-store", headers: { "content-type": "application/json" }, body: JSON.stringify({ query, limit: 10 }), signal });
  if (!response.ok) throw new Error("indexed_lookup_unavailable");
  return normalizeIndexedLookup(await response.json());
}
