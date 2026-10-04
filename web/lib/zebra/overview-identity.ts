import type { ExploreResponse } from "./types";

export type OverviewSubject = { id: string; label: string; kind: "gene" | "disease" };

/** An explanation belongs to an explicitly resolved subject, never the first search hit. */
export function overviewSubject(data: ExploreResponse | null): OverviewSubject | null {
  if (!data || data.retrieval?.scope === "name_candidates_only") return null;
  const semantic = data.query_execution?.semantic_focus ?? data.execution.rerun_context?.semantic_focus;
  const focus = semantic?.length ? semantic : data.plan?.focus?.length ? data.plan.focus : data.interpretation.entities.map(entity => entity.id);
  const ids = [...new Set(focus)];
  if (ids.length !== 1) return null;
  const entity = data.graph.nodes.find(node => node.id === ids[0])
    ?? data.interpretation.entities.find(node => node.id === ids[0]);
  if (!entity || (entity.kind !== "gene" && entity.kind !== "disease")) return null;
  return { id: entity.id, label: entity.label, kind: entity.kind };
}
