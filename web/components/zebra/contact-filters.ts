import type { ExplorePlan, ExploreResponse } from "@/lib/zebra/types";

export const CONTACT_ROLES = ["all", "groups", "researchers", "studies", "models", "resources", "funding"] as const;
export type ContactRole = typeof CONTACT_ROLES[number];
export function contactRole(data: ExploreResponse | null, community: boolean): ContactRole {
  const intent = data?.plan?.intent ?? data?.interpretation.intent;
  return CONTACT_ROLES.includes(intent as ContactRole) ? intent as ContactRole : community ? "researchers" : "all";
}
/** Filter jobs are new, model-free searches rooted only in actual resolved inputs. */
export function contactPlan(data: ExploreResponse | null, intent: ContactRole): ExplorePlan | null {
  if (!data) return null;
  const candidates = data.graph.community?.seeds.map(seed => seed.id) ?? data.plan?.focus ?? data.interpretation.entities.map(entity => entity.id);
  const focus = [...new Set(candidates)].filter(Boolean).slice(0, 8);
  const filters = { ...data.plan?.filters };
  if (intent !== "all" && intent !== "studies") filters.recruiting = null;
  if (data.plan && intent !== data.plan.intent) filters.kind = null;
  return focus.length ? { focus, intent, filters } : null;
}
