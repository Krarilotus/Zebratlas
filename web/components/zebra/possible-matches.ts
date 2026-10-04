import type { ExploreNameCandidate, ExplorePlan, ExploreResponse } from "@/lib/zebra/types";

/** Bound only the display caption; retain the complete prepared query separately. */
export function publicSearchCaption(display: string): string {
  const bytes = new TextEncoder().encode(display);
  let end = Math.min(bytes.length, 512);
  while (end < bytes.length && end > 0 && (bytes[end] & 0xc0) === 0x80) end--;
  return new TextDecoder().decode(bytes.subarray(0, end));
}

/** Name-index candidates never become scientific results, graph links or diagnoses. */
export function possibleMatchState(data: ExploreResponse | null): { matches: ExploreNameCandidate[]; invalid: boolean } | null {
  if (!data || data.execution.engine !== "indexed-atlas" || !["candidates", "empty"].includes(data.retrieval?.status ?? "") || data.retrieval?.scope !== "name_candidates_only" || data.results.length || data.graph.nodes.length || data.graph.edges.length || data.execution.sparql || data.execution.queries?.length) return null;
  const returned = data.possible_matches ?? [], ids = new Set<string>();
  if (!Array.isArray(returned) || returned.length > 10 || (data.retrieval.status === "empty" && returned.length > 0)) return { matches: [], invalid: true };
  // One malformed record must not hide the other actual source matches.
  // Bound source bytes without changing the canonical label; presentation clips only.
  const matches = returned.filter(match => {
    if (!match || typeof match.id !== "string" || !match.id.trim() || match.id.length > 256 || typeof match.label !== "string" || !match.label.trim() || match.label.length > 32768 || new TextEncoder().encode(match.label).length > 32768 || typeof match.kind !== "string" || !match.kind || match.match !== "fuzzy" || match.method !== "lexical" || (match.score !== undefined && (typeof match.score !== "number" || !Number.isFinite(match.score))) || ids.has(match.id)) return false;
    ids.add(match.id); return true;
  });
  return { matches, invalid: returned.length > 0 && matches.length === 0 };
}

/** Only a deliberate selection of a returned identifier enables model-free lookup. */
export function possibleMatchPlan(data: ExploreResponse | null, id: string): ExplorePlan | null {
  const state = possibleMatchState(data);
  if (!state || state.invalid || !state.matches.some(match => match.id === id)) return null;
  return { focus: [id], intent: "all", filters: {} };
}
