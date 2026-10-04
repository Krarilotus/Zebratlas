export type NavigationPanel = "overview" | "evidence" | "request" | "plan";
export type NavigationView = { search: string; page: "results" | "info"; node?: string; panel?: NavigationPanel; context?: string; returnsToResults?: boolean };

export function decodeNavigation(value: unknown): NavigationView | null {
  if (!value || typeof value !== "object") return null;
  const view = (value as { zebra?: unknown }).zebra;
  if (!view || typeof view !== "object") return null;
  const record = view as Record<string, unknown>;
  if (typeof record.search !== "string" || !record.search || !["results", "info"].includes(String(record.page))) return null;
  const panel = ["overview", "evidence", "request", "plan"].includes(String(record.panel)) ? record.panel as NavigationPanel : undefined;
  return { search: record.search, page: record.page as NavigationView["page"], node: typeof record.node === "string" ? record.node : undefined, context: typeof record.context === "string" ? record.context : undefined, panel, returnsToResults: record.returnsToResults === true };
}

/** One detail entry per result page; switching a result replaces that detail entry. */
export function infoNavigation(previous: NavigationView | null, search: string, node: string, panel: NavigationPanel, context?: string) {
  const replace = previous?.page === "info" && previous.search === search;
  return {
    method: replace ? "replaceState" as const : "pushState" as const,
    view: { search, page: "info", node, panel, context, returnsToResults: replace ? previous.returnsToResults : true } satisfies NavigationView,
  };
}

/** Re-selecting the current public node keeps its camera and in-flight request. */
export function isCurrentInfo(previous: NavigationView | null, search: string, node: string, context?: string) {
  return previous?.page === "info" && previous.search === search && previous.node === node
    && (context === undefined || context === previous.context);
}

export function restoreNavigation<T>(state: unknown, snapshots: ReadonlyMap<string, T>): { view: NavigationView; snapshot: T } | null {
  const view = decodeNavigation(state);
  const snapshot = view && snapshots.get(view.search);
  return view && snapshot ? { view, snapshot } : null;
}

/** Both aborts and a changed submitted search reject an obsolete neighborhood response. */
export function acceptNavigationResponse(signal: AbortSignal, requestedSearch: string, activeSearch: string, id: string, nodes: ReadonlyArray<{ id: string }>) {
  return !signal.aborted && requestedSearch === activeSearch && nodes.some((node) => node.id === id);
}

export function heroProgress(scrollTop: number, expandedHeight: number, compactHeight: number) {
  const distance = Math.max(0, expandedHeight - compactHeight);
  const clipped = Math.min(Math.max(0, scrollTop), distance);
  return { distance, offset: clipped / 2, expanded: clipped < distance - 16 };
}
