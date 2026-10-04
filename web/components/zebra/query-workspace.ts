import type { ExploreResponse } from "@/lib/zebra/types";
import { executedQueryTexts } from "@/lib/zebra/query-answer";

export type QueryNode = { key: string; ids: string[]; label: string; kind: string | null };
export type QueryEdge = { key: string; from: string; to: string; relation: string; step: number; optional?: boolean };
export type QueryDiagram = { version: 1; nodes: QueryNode[]; edges: QueryEdge[]; settings: unknown };
export type QueryInspection = { branches: { key: string; label: string; triples: unknown[] }[]; constraints: { key: string; label: string; variables: string[]; text: string; truncated?: boolean }[]; totalTriples: number; editable: boolean };
export type QueryNeighborhood = { seed_ids: string[]; relations: string[]; directions: ["incoming", "outgoing"]; limit: number };
export type QueryModel = { graph: QueryDiagram; text: string; editable: boolean; branch?: number; inspection: QueryInspection; neighborhood?: QueryNeighborhood };
export type QueryLink = { id: string; label: string; kind?: string };
export type QuerySuggestion = { relation: string; direction: "incoming" | "outgoing"; target_class: string; target_kind: string; count: number; node: QueryLink | null; witness_edge: string; witness_status?: string | null; evidence_sample: { source_url: string; retrieved_at: string | null; version: string | null; sha256: string | null; record_locator: string }[] };
export type QuerySuggestionPage = { phase: "properties" | "targets"; items: QuerySuggestion[]; total: number; offset: number; index_sha256: string };
export type QueryRunSettings = { limit: number; reasoning: boolean; focus: string[]; semantic_focus: string[]; linked: QueryLink[] };
export type QueryReceipt = { key: string; text: string; stage: string; backend: string; identity?: string; settings?: QueryRunSettings; linked: QueryLink[] };
export type QueryParser = { parseQuery(text: string, linked?: QueryLink[]): QueryModel; inspectQuery(text: string, linked?: QueryLink[], branch?: number): QueryModel; addConnection(model: QueryModel, key: string, suggestion: QuerySuggestion, linked?: QueryLink[]): QueryModel; removeElement(model: QueryModel, key: string, edge?: boolean, linked?: QueryLink[]): QueryModel; setQueryLimit(text: string, limit: number): string; setNeighborhoodRelations(text: string, relations: string[]): string };
export type QueryPhantom = { key: string; label: string; detail: string; accessibleName: string; elementId?: string };
export type QueryCanvas = { destroy(): void; fit(): Promise<void>; zoom(factor: number): void; update(graph: QueryDiagram, selected: string, phantoms: QueryPhantom[], labels: Record<string, string>, callbacks: { onSelect(key: string): void; onPhantom(index: number): void; onEdge?(key: string): void }): Promise<void> };
type QueryPackage = { mount(element: HTMLElement): Promise<QueryCanvas> };
const object = (value: unknown): Record<string, unknown> => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
const ids = (value: unknown): string[] => Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : [];
const cap = (value: unknown): number | undefined => typeof value === "number" && Number.isInteger(value) && value > 0 && value <= 160 ? value : undefined;
// UI draft identity is distinct from the server's content-addressed activity ID.
// Cached answers retain their drafts; a newly returned execution starts fresh.
const receiptScopes = new WeakMap<ExploreResponse, number>();
let nextReceiptScope = 0;
function receiptScope(data: ExploreResponse): number {
  let scope = receiptScopes.get(data);
  if (scope === undefined) { scope = ++nextReceiptScope; receiptScopes.set(data, scope); }
  return scope;
}
/** Receipt settings, linked display labels and semantic focus remain separate from query text. */
export function queryReceipts(data: ExploreResponse | null): QueryReceipt[] {
  if (!data) return [];
  const linked = [...data.graph.nodes.map(({ id, label, kind }) => ({ id, label, kind })), ...(data.execution.rerun_context?.linked || []).filter(link => !data.graph.nodes.some(node => node.id === link.id))];
  const raw = object(data), execution = object(raw.execution), queryExecution = object(raw.query_execution), answer = object(queryExecution.answer);
  const results = Array.isArray(answer.results) ? answer.results.map(object) : [];
  const entries = Array.isArray(execution.queries) ? execution.queries.map(object) : [];
  return executedQueryTexts(data).map((text, index) => {
    const leaf = (results[index]?.query === text ? results[index] : results.find(result => result.query === text)) || (entries[index]?.sparql === text ? entries[index] : entries.find(entry => entry.sparql === text)) || execution;
    const activity = object(leaf.activity), parameters = object(activity.parameters);
    const limit = cap(parameters.row_cap) ?? cap(leaf.row_cap) ?? cap(leaf.limit) ?? cap(execution.limit);
    const inference = typeof parameters.infer === "boolean" ? parameters.infer : typeof leaf.reasoning === "boolean" ? leaf.reasoning : typeof leaf.infer === "boolean" ? leaf.infer : typeof execution.reasoning === "boolean" ? execution.reasoning : undefined;
    const focus = ids(leaf.focus).length ? ids(leaf.focus) : ids(leaf.seed_ids).length ? ids(leaf.seed_ids) : ids(queryExecution.focus).length ? ids(queryExecution.focus) : data.execution.rerun_context?.focus || [];
    const semantic_focus = ids(queryExecution.semantic_focus).length ? ids(queryExecution.semantic_focus) : data.execution.rerun_context?.semantic_focus || [];
    const identity = typeof activity["@id"] === "string" ? activity["@id"] : undefined;
    return { key: `${receiptScope(data)}:${index}:${identity || "query"}:${text}`, text, identity, stage: typeof leaf.stage === "string" ? leaf.stage : "", backend: typeof leaf.backend === "string" ? leaf.backend : String(execution.engine || ""), linked,
      settings: limit !== undefined && inference !== undefined ? { limit, reasoning: inference, focus, semantic_focus, linked } : undefined };
  });
}
export function queryFitsBudget(text: string): boolean { return !!text.trim() && new TextEncoder().encode(text).length <= 16384; }
export function canMutateQuery(model: QueryModel | null, draft: string, visualText: string): boolean { return (model?.editable === true || !!model?.neighborhood) && draft === visualText; }
/** Canvas-only display semantics; never change predicates or query algebra in the model. */
export function queryCanvasState(input: QueryDiagram | null, selected: string, phantoms: QueryPhantom[], optional: string) {
  const empty: QueryDiagram = { version: 1, nodes: [], edges: [], settings: {} };
  const graph = input || empty;
  const nodeKeys = new Set(graph.nodes.map(node => node.key));
  const selection = nodeKeys.has(selected) ? selected : "";
  return { graph: { ...graph, edges: graph.edges.filter(edge => nodeKeys.has(edge.from) && nodeKeys.has(edge.to)).map(edge => edge.optional ? { ...edge, relation: `${optional} · ${edge.relation.replace(/^.*[#/]/, "")}` } : edge) }, selected: selection, phantoms: selection ? phantoms : [] };
}
/** The exact adapter used by the mounted native editor, including an empty revision. */
export async function updateQueryCanvas(editor: QueryCanvas, input: QueryDiagram | null, selected: string, phantoms: QueryPhantom[], optional: string, elementPrefix: string, callbacks: Parameters<QueryCanvas["update"]>[4]): Promise<void> {
  const state = queryCanvasState(input, selected, phantoms, optional);
  const labels = Object.fromEntries(state.graph.nodes.map(node => [node.key, node.kind && node.label.startsWith("?") ? `${node.label} · ${node.kind.replaceAll("_", " ")}` : node.label]));
  await editor.update(state.graph, state.selected, state.phantoms.map((phantom, index) => ({ ...phantom, elementId: `${elementPrefix}-${index}` })), labels, callbacks);
}
let parserPromise: Promise<QueryParser> | undefined, packagePromise: Promise<QueryPackage> | undefined;
export function queryParser(): Promise<QueryParser> { const url = "/query-by-graph/query-parser.js"; return parserPromise ??= import(/* webpackIgnore: true */ url).catch(error => { parserPromise = undefined; throw error; }); }
export function queryPackage(): Promise<QueryPackage> { const url = "/query-by-graph/editor.js"; return packagePromise ??= import(/* webpackIgnore: true */ url).catch(error => { packagePromise = undefined; throw error; }); }
export async function inspectQuery(text: string, linked: QueryLink[], branch = 0): Promise<QueryModel> {
  const parser = await queryParser();
  if (branch !== 0) return parser.inspectQuery(text, linked, branch);
  try { const model = parser.parseQuery(text, linked); return model.editable && model.graph.nodes.length <= 12 && model.graph.edges.length <= 16 ? model : parser.inspectQuery(text, linked); }
  catch { return parser.inspectQuery(text, linked); }
}
/** Discovery is explicit, indexed and bounded. No suggestions run during search or draft typing. */
export async function querySuggestions(query: Record<string, string | number | undefined>, signal: AbortSignal): Promise<QuerySuggestionPage> {
  const values = { ...query, limit: 5 };
  const search = new URLSearchParams(Object.entries(values).filter(([, value]) => value !== undefined).map(([key, value]) => [key, String(value)]));
  const response = await fetch(`/zebra/api/query-suggestions?${search}`, { signal, cache: "no-store" });
  if (!response.ok) throw new Error("query_suggestions_unavailable");
  const page: QuerySuggestionPage = await response.json();
  return { ...page, items: page.items.slice(0, 5) };
}
/** Keep raw prepared text out of public history and preserve unrelated history fields. */
export function queryHistoryState(state: unknown, search?: string): Record<string, unknown> {
  const next = { ...object(state) }; delete next.zebraQuery;
  if (search) next.zebraQuery = search;
  return next;
}
