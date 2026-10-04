import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";
import type { ExploreResponse } from "@/lib/zebra/types";
import { answerUriLabel, normalizeQueryAnswer } from "@/lib/zebra/query-answer";
import { safeActionUrl, type FindingRow } from "./result-overview";

export const findingsNoteWords = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).findingsWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["findingsWords"]>;
export type FindingsSnapshot = { subject: string; body: string; refs: string[]; meta: { record_total: number; record_included: number; query_row_included: number; question_truncated: boolean; snapshot_limited: boolean; captured_at: string } };
const clip = (value: string, limit: number) => value.length > limit ? `${value.slice(0, limit)}…` : value;

/** A local record snapshot, never a scientific synthesis or an invented ranking. */
export function buildFindingsSnapshot(query: string, data: ExploreResponse, rows: FindingRow[], locale: ZebraLocale, limitations: string[] = [], now = new Date().toISOString()): FindingsSnapshot {
  const words = findingsNoteWords[locale];
  const lines = [`${words.question}\n${clip(query, 12000)}`, `${words.records} (${Math.min(rows.length, 24)}/${rows.length})`];
  const refs: string[] = [];
  let bytes = new TextEncoder().encode(lines.join("\n\n")).length;
  let included = 0;
  for (const row of rows.slice(0, 24)) {
    const item = row.result;
    const sources = [...row.sources, ...(item.metadata_evidence || [])].filter((source, index, values) => values.findIndex(other => other.record === source.record && other.url === source.url && other.source === source.source) === index);
    const action = safeActionUrl(item.official_action?.url) || safeActionUrl(item.how_to_get?.url);
    const block = [`- [${item.id}] ${clip(item.label, 400)} (${item.kind})`, ...(row.conditionId ? [`  ${words.scope}: ${row.conditionId}`] : []),
      ...(item.reason && item.reason !== `Matched ${item.label}` ? [`  ${words.relation}: ${clip(item.reason, 700)}`] : []),
      ...(item.holder ? [`  ${words.holder}: ${clip(item.holder.name, 300)}`] : []), ...(item.status ? [`  ${words.status}: ${item.status}`] : []),
      ...(item.facts || []).slice(0, 4).map(fact => `  ${fact.key.replaceAll("_", " ")}: ${clip(fact.value, 500)}`), ...(action ? [`  ${words.route}: ${action}`] : []),
      ...sources.slice(0, 3).map(source => `  ${words.source} [${source.record || words.unknown}]: ${source.source}; ${safeActionUrl(source.url) || words.unknown}; ${words.retrieved}: ${source.retrieved_at || words.unknown}${source.version ? `; ${words.version}: ${source.version}` : ""}${source.sha256 ? `; ${words.checksum}: ${source.sha256}` : ""}`),
    ].join("\n");
    const length = new TextEncoder().encode(block).length;
    if (bytes + length > 28000) break;
    lines.push(block); bytes += length; refs.push(item.id); included++;
  }
  const answer = normalizeQueryAnswer(data.query_execution);
  const returnedLimitations = [...limitations];
  const labels = new Map(data.graph.nodes.map(node => [node.id, node.label]));
  let queryRows = 0;
  let answerLimited = false;
  if (answer) {
    lines.push(words.queryAnswer);
    if (answer.status !== "executed") lines.push(`${words.status}: ${answer.status}`);
    if (answer.questionStatus) lines.push(`question_status: ${answer.questionStatus}`);
    const trace = data.query_execution?.answer.tool_trace;
    const terminal = trace?.filter(item => item.stage === "query_status").at(-1);
    if (terminal) lines.push(clip(JSON.stringify(terminal), 1200));
    if (answer.diagnostic) lines.push(clip(answer.diagnostic, 600));
    for (const result of answer.results) {
      if (result.kind === "boolean") lines.push(result.value ? words.yes : words.no);
      else {
        for (const row of result.rows.slice(0, 6)) {
          const block = row.map((cell, column) => `${result.columns[column]}: ${cell ? clip(cell.type === "uri" ? `${answerUriLabel(cell.value, labels)} <${cell.value}>` : `${cell.value}${cell.datatype ? ` <${cell.datatype}>` : ""}${cell.lang ? ` @${cell.lang}` : ""}`, 500) : words.unbound}`).join("; ");
          if (new TextEncoder().encode(lines.join("\n\n") + block).length > 40000) { answerLimited = true; break; }
          lines.push(block); queryRows++;
        }
        answerLimited ||= result.totalRows > 6 || result.truncated;
      }
      const lineage = result.metadata.lineage as Record<string, unknown> | undefined;
      if (lineage) lines.push(`${words.source}: ${typeof lineage.source_url === "string" ? safeActionUrl(lineage.source_url) || words.unknown : words.unknown}; ${words.retrieved}: ${String(lineage.retrieved_at || words.unknown)}; ${words.version}: ${String(lineage.version || words.unknown)}; ${words.checksum}: ${String(lineage.sha256 || words.unknown)}`);
      if (Array.isArray(result.metadata.notes)) returnedLimitations.push(...result.metadata.notes.filter((note): note is string => typeof note === "string"));
    }
  }
  const uniqueLimits = [...new Set(returnedLimitations)].slice(0, 8);
  if (uniqueLimits.length) lines.push(words.limitations, ...uniqueLimits.map(note => `- ${clip(note, 1000)}`));
  const limited = included < rows.length || answerLimited || answer?.truncated === true || query.length > 12000;
  if (limited) lines.push(words.bounds);
  return { subject: `${words.title}: ${clip(query.replaceAll("\n", " "), 110)}`, body: lines.join("\n\n"), refs,
    meta: { record_total: rows.length, record_included: included, query_row_included: queryRows, question_truncated: query.length > 12000, snapshot_limited: limited, captured_at: now } };
}
