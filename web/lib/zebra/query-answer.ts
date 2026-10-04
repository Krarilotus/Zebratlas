/** Preserve SPARQL lexical values. Numeric literals must never pass through Number(). */
export type AnswerCell = { type: "uri" | "literal" | "bnode"; value: string; datatype?: string; lang?: string };
export type AnswerTable = {
  kind: "table"; columns: string[]; rows: (AnswerCell | null)[][]; totalRows: number;
  totalColumns: number; truncated: boolean; backend?: string; metadata: Record<string, unknown>;
};
export type AnswerBoolean = { kind: "boolean"; value: boolean; backend?: string; metadata: Record<string, unknown> };
export type AnswerData = AnswerTable | AnswerBoolean;
export type AnswerView = { status: "executed" | "not_executed" | "unsupported" | "invalid"; questionStatus?: "partial" | "not_executed"; results: AnswerData[]; diagnostic?: string; truncated?: boolean; metadata: Record<string, unknown> };

const MAX_ROWS = 40;
const MAX_COLUMNS = 10;
const record = (value: unknown): Record<string, unknown> | null => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;

function cell(value: unknown): AnswerCell | null | undefined {
  if (value == null) return null; // Unbound SPARQL variable, distinct from an empty literal.
  const input = record(value);
  if (!input || typeof input.value !== "string" || !["uri", "literal", "typed-literal", "bnode"].includes(String(input.type))) return undefined;
  return {
    type: input.type === "typed-literal" ? "literal" : input.type as AnswerCell["type"], value: input.value,
    ...(typeof input.datatype === "string" ? { datatype: input.datatype } : {}),
    ...(typeof input["xml:lang"] === "string" ? { lang: input["xml:lang"] } : {}),
  };
}

/** A bounded view of the canonical SearchAnswer envelope; the original remains untouched. */
export function normalizeQueryAnswer(execution: unknown): AnswerView | null {
  const outer = record(execution);
  if (!outer) return null;
  const metadata = outer;
  if (outer.status === "unsupported") return { status: "unsupported", results: [], diagnostic: typeof outer.diagnostic === "string" ? outer.diagnostic : undefined, metadata };
  if (outer.status === "not_executed") return { status: "not_executed", questionStatus: "not_executed", results: [], diagnostic: typeof outer.diagnostic === "string" ? outer.diagnostic : undefined, metadata };
  if (outer.status !== undefined && outer.status !== "executed") return null;
  // Exact canonical SearchAnswer is the public contract; tolerate an explicit status wrapper.
  const nested = record(outer.answer);
  const search = outer.status === "executed" && nested && record(nested.answer) ? nested : outer;
  const answer = record(search?.answer);
  const trace = Array.isArray(answer?.tool_trace) ? answer.tool_trace.map(record).filter(value => value?.stage === "query_status") : [];
  const terminal = trace.at(-1);
  const questionStatus = outer.question_status === "partial" || terminal?.status === "partial" ? "partial" : outer.question_status === "not_executed" || terminal?.status === "not_executed" ? "not_executed" : undefined;
  const inputs = answer?.results;
  if (!Array.isArray(inputs)) return { status: "invalid", results: [], metadata };
  const results: AnswerData[] = [];
  let invalid = false;
  for (const input of inputs.slice(0, 6)) {
    const result = record(input);
    const data = record(result?.data);
    if (!result || !data) { invalid = true; continue; }
    const backend = typeof result.backend === "string" ? result.backend : undefined;
    if (typeof data.boolean === "boolean") {
      results.push({ kind: "boolean", value: data.boolean, backend, metadata: result });
      continue;
    }
    const vars = record(data.head)?.vars;
    const bindings = record(data.results)?.bindings;
    if (!Array.isArray(vars) || !vars.every((key) => typeof key === "string" && key.length > 0) || new Set(vars).size !== vars.length || !Array.isArray(bindings)) { invalid = true; continue; }
    const columns = (vars as string[]).slice(0, MAX_COLUMNS);
    const rows: (AnswerCell | null)[][] = [];
    for (const raw of bindings.slice(0, MAX_ROWS)) {
      const binding = record(raw);
      if (!binding) { invalid = true; continue; }
      const values = columns.map((key) => cell(binding[key]));
      if (values.some((value) => value === undefined)) { invalid = true; continue; }
      rows.push(values as (AnswerCell | null)[]);
    }
    results.push({ kind: "table", columns, rows, totalRows: bindings.length, totalColumns: vars.length,
      truncated: result.truncated === true || bindings.length > MAX_ROWS || vars.length > MAX_COLUMNS,
      backend, metadata: result });
  }
  return { status: invalid ? "invalid" : "executed", questionStatus, results, truncated: inputs.length > 6, metadata };
}

export function hasRenderableAnswer(execution: unknown): boolean {
  const value = normalizeQueryAnswer(execution);
  return !!value && (value.status === "unsupported" || value.status === "not_executed" || value.status === "invalid" || value.questionStatus === "partial" || value.results.length > 0);
}

/** Only actual completed queries may be labelled as executed in the query inspector. */
export function executedQueryTexts(response: unknown): string[] {
  const outer = record(response);
  const execution = record(outer?.execution);
  const queryExecution = record(outer?.query_execution);
  if (execution?.engine === "none" || queryExecution?.status === "not_executed" || queryExecution?.status === "unsupported") return [];
  const results = record(queryExecution?.answer)?.results;
  const queries = Array.isArray(results) ? results.flatMap(result => {
    const query = record(result)?.query;
    return typeof query === "string" && query.trim() ? [query] : [];
  }) : [];
  if (queries.length) return queries;
  // Neighborhood and frontier requests are separate executions. Preserve each
  // exact query so it can be inspected and rerun independently.
  if (Array.isArray(execution?.queries)) return execution.queries.flatMap(value => {
    const entry = record(value);
    return entry?.engine === "nrese" && typeof entry.sparql === "string" && entry.sparql.trim() ? [entry.sparql] : [];
  });
  return typeof execution?.sparql === "string" && execution.sparql.trim() ? [execution.sparql] : [];
}

export function safeAnswerHref(value: string): string | undefined {
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : undefined;
  } catch { return undefined; }
}

const ATLAS_BASE = "https://w3id.org/rare-disease-atlas/";
const PREFIXES: [string, string][] = [
  [`${ATLAS_BASE}id/`, ""], [`${ATLAS_BASE}edge/`, ""], [ATLAS_BASE, "ra:"],
  ["http://www.w3.org/2001/XMLSchema#", "xsd:"], ["http://www.w3.org/1999/02/22-rdf-syntax-ns#", "rdf:"],
  ["http://www.w3.org/2000/01/rdf-schema#", "rdfs:"], ["http://www.w3.org/ns/prov#", "prov:"],
  ["http://www.w3.org/2002/07/owl#", "owl:"], ["http://purl.obolibrary.org/obo/", "obo:"],
];

export function answerNodeId(uri: string): string | undefined {
  if (!uri.startsWith(`${ATLAS_BASE}id/`)) return undefined;
  try { return decodeURIComponent(uri.slice(`${ATLAS_BASE}id/`.length)); } catch { return undefined; }
}

/** Unknown URI namespaces retain the full URI; never invent a namespace prefix. */
export function answerUriLabel(uri: string, labels: ReadonlyMap<string, string> = new Map()): string {
  const id = answerNodeId(uri);
  if (id && labels.has(id)) return labels.get(id)!;
  for (const [base, prefix] of PREFIXES) {
    if (!uri.startsWith(base)) continue;
    try { return prefix + decodeURIComponent(uri.slice(base.length)); } catch { return uri; }
  }
  return uri;
}

export function isGraphOnlyTable(table: AnswerTable, labels: ReadonlyMap<string, string>): boolean {
  return table.rows.length > 0 && table.rows.every((row) => {
    const known = row.filter((value) => value?.type === "uri").map((value) => answerNodeId(value!.value)).filter((id): id is string => !!id && labels.has(id));
    return known.length > 0 && row.every((value, column) => !value ||
      (value.type === "uri" && !!answerNodeId(value.value) && labels.has(answerNodeId(value.value)!)) ||
      (value.type === "literal" && /^(label|name)$/i.test(table.columns[column]) && known.some((id) => labels.get(id) === value.value)));
  });
}
