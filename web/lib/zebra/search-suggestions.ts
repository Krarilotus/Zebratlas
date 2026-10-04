export type LookupHit = { node: { id: string; label: string; kind: string }; matched?: string };
export type SearchLookup = { results: LookupHit[] };
export type SearchSuggestion = { key: string; label: string; query: string; kind?: string; operator?: "AND" | "OR" };
type Context = { tail: string; start: number; prefix: string; operator?: { start: number; end: number; value: string }; first?: string };
const norm = (value: string) => value.trim().replace(/\s+/g, " ").toLocaleLowerCase();

export function suggestionContext(query: string): Context | null {
  if (!query.trim() || query.length > 512 || /[\r\n]/.test(query)) return null;
  const operators = [...query.matchAll(/\b(AND|OR)\b/gi)];
  const last = operators.at(-1);
  let start = last ? (last.index ?? 0) + last[0].length : 0;
  if (!last && query.trim().includes(" ")) {
    const word = /\S+\s*$/.exec(query);
    if (word) start = word.index;
  }
  while (/\s/.test(query[start] ?? "") && start < query.length) start++;
  const tail = query.slice(start).trim();
  if (/[\x00-\x1f\x7f]/.test(query)) return null;
  if (tail.length < 2 || new TextEncoder().encode(tail).length > 128 || /["()]/.test(tail)) return null;
  const prefix = query.slice(0, start);
  const leftTerm = last ? /\S+$/.exec(query.slice(0, last.index).trim())?.[0] : prefix.trim();
  const first = leftTerm && /^[A-Za-z][A-Za-z0-9:_-]{1,40}$/.test(leftTerm) ? leftTerm : undefined;
  return { tail, start, prefix, first, operator: last ? { start: last.index ?? 0, end: (last.index ?? 0) + last[0].length, value: last[0].toUpperCase() } : undefined };
}
function cleanHits(lookup: SearchLookup): LookupHit[] {
  const seen = new Set<string>();
  return (Array.isArray(lookup?.results) ? lookup.results : []).filter((hit) => {
    if (!hit?.node || typeof hit.node.id !== "string" || typeof hit.node.label !== "string" || typeof hit.node.kind !== "string") return false;
    const label = hit.node.label.trim();
    if (!label || label.length > 160 || /[\r\n\x00-\x1f]/.test(label) || seen.has(hit.node.id)) return false;
    seen.add(hit.node.id); return true;
  });
}
const exact = (term: string, hits: LookupHit[]) => hits.some((hit) => [hit.node.id, hit.node.label, hit.matched ?? ""].some((label) => norm(label) === norm(term)));

/** Each entity chip is an actual indexed node; operator chips only edit syntax. */
export function buildSuggestions(query: string, lookup: SearchLookup, firstLookup?: SearchLookup): SearchSuggestion[] {
  const context = suggestionContext(query); if (!context) return [];
  const hits = cleanHits(lookup);
  const choices: SearchSuggestion[] = hits.map((hit) => ({ key: hit.node.id, label: hit.node.label, query: context.prefix + hit.node.label, kind: hit.node.kind }))
    .filter((choice) => choice.query.trim() !== query.trim()).slice(0, 4);
  if (exact(context.tail, hits)) {
    for (const operator of ["AND", "OR"] as const) {
      let next: string;
      if (context.operator) {
        if (!context.first || !firstLookup || !exact(context.first, cleanHits(firstLookup))) continue;
        next = query.slice(0, context.operator.start) + operator + query.slice(context.operator.end);
      }
      else if (context.first && firstLookup && exact(context.first, cleanHits(firstLookup))) next = context.first + " " + operator + " " + context.tail;
      else next = query.trimEnd() + " " + operator + " ";
      if (next.trim() !== query.trim()) choices.push({ key: operator, label: operator, query: next, operator });
    }
  }
  return choices;
}

type Timer = { set: (callback: () => void, delay: number) => unknown; clear: (handle: unknown) => void };
const realTimer: Timer = { set: (callback, delay) => setTimeout(callback, delay), clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>) };
export function createSuggestionScheduler({ lookup, publish, timer = realTimer }: {
  lookup: (term: string, signal: AbortSignal) => Promise<SearchLookup>;
  publish: (suggestions: SearchSuggestion[]) => void;
  timer?: Timer;
}) {
  let pending: unknown;
  let controller: AbortController | null = null;
  let generation = 0;
  let disposed = false;
  function clear(visible = true) {
    generation++;
    if (pending !== undefined) timer.clear(pending);
    pending = undefined; controller?.abort(); controller = null;
    if (visible && !disposed) publish([]);
  }
  return {
    edit(query: string, eligible = true) {
      clear();
      const context = suggestionContext(query);
      if (disposed || !eligible || !context) return;
      const revision = generation;
      pending = timer.set(() => {
        pending = undefined;
        const request = new AbortController(); controller = request;
        const terms = [context.tail, ...(context.first ? [context.first] : [])];
        void Promise.all(terms.map((term) => lookup(term, request.signal))).then(([tail, first]) => {
          if (!disposed && revision === generation && !request.signal.aborted) publish(buildSuggestions(query, tail, first));
        }).catch(() => { /* Optional indexed lookup stays quiet if unavailable. */ });
      }, 2000);
    },
    cancel: () => clear(),
    dispose() { clear(false); disposed = true; },
  };
}
