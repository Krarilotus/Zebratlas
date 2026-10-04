"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";

import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";


import { useMemo } from "react";
import type { ExploreResponse, SearchGraph } from "@/lib/zebra/types";
import { answerNodeId, answerUriLabel, isGraphOnlyTable, normalizeQueryAnswer, safeAnswerHref, type AnswerCell, type AnswerData } from "@/lib/zebra/query-answer";
import { useZebraLocale } from "./Locale";
import styles from "./QueryAnswer.module.css";
import { formatMessage } from "@/lib/i18n/format";

export { hasRenderableAnswer } from "@/lib/zebra/query-answer";

const words = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).answerWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["answerWords"]>;
const executionWords = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).executionWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["executionWords"]>;

function download(value: unknown) {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: "application/json;charset=utf-8" });
  const href = URL.createObjectURL(blob);
  const link = document.createElement("a"); link.href = href; link.download = "zebratlas-query-answer.json"; link.click();
  window.setTimeout(() => URL.revokeObjectURL(href), 1000);
}

export default function QueryAnswer({ execution, graph, onSelectNode }: {
  execution: ExploreResponse["query_execution"];
  graph?: SearchGraph;
  onSelectNode?: (id: string) => void;
}) {
  const locale = useZebraLocale();
  const text = words[zebraCopyLocale(locale)];
  const statusText = executionWords[zebraCopyLocale(locale)];
  const view = useMemo(() => normalizeQueryAnswer(execution), [execution]);
  const labels = useMemo(() => new Map(graph?.nodes.map((node) => [node.id, node.label]) ?? []), [graph]);
  if (!view) return null;

  function value(cell: AnswerCell | null) {
    if (!cell) return <span className={styles.unbound}>{text.unbound}</span>;
    if (cell.type === "uri") {
      const id = answerNodeId(cell.value);
      const label = answerUriLabel(cell.value, labels);
      if (id && labels.has(id) && onSelectNode) return <button type="button" className={styles.node} title={cell.value} onClick={() => onSelectNode(id)}>{label}</button>;
      const href = safeAnswerHref(cell.value);
      return href ? <a href={href} title={cell.value} target="_blank" rel="noopener noreferrer">{label}</a> : <span title={cell.value}>{label}</span>;
    }
    const title = [cell.datatype && `${text.datatype}: ${answerUriLabel(cell.datatype)}`, cell.lang && `${text.language}: ${cell.lang}`].filter(Boolean).join(" · ");
    return <span title={title || undefined} className={cell.datatype?.match(/#(?:integer|decimal|double|float|long|int|nonNegativeInteger)$/) ? styles.number : undefined}>{cell.type === "bnode" ? `_:${cell.value}` : cell.value}{cell.lang && <small className={styles.language}> {cell.lang}</small>}</span>;
  }

  function rows(result: AnswerData, index: number) {
    if (result.kind === "boolean") return <p className={styles.boolean}>{result.value ? text.yes : text.no}</p>;
    if (!result.rows.length) return <p className={styles.muted}>{result.totalRows === 0 ? text.empty : text.invalid}</p>;
    return <div className={styles.scroll} tabIndex={0} role="region" aria-label={`${text.answer} ${index + 1}`}>
      <table><caption>{formatMessage(text.rowCount, { shown: result.rows.length, total: result.totalRows }, locale)}{result.totalColumns > result.columns.length ? ` · ${formatMessage(text.columnCount, { shown: result.columns.length, total: result.totalColumns }, locale)}` : ""}</caption>
        <thead><tr>{result.columns.map((column) => <th key={column} scope="col">{column.replaceAll("_", " ")}</th>)}</tr></thead>
        <tbody>{result.rows.map((row, rowIndex) => <tr key={rowIndex}>{row.map((cell, columnIndex) => <td key={result.columns[columnIndex]}>{value(cell)}</td>)}</tr>)}</tbody>
      </table>
    </div>;
  }

  return <section className={styles.answer} aria-label={view.status === "not_executed" ? statusText.status : text.answer}>
    <h2>{view.status === "not_executed" ? statusText.status : text.answer}</h2>
    {view.status === "not_executed" && <p role="status">{statusText.notExecuted}</p>}
    {view.questionStatus === "partial" && <p role="status">{statusText.partial}</p>}
    {view.status === "unsupported" && <p role="status">{text.unsupported}</p>}
    {view.status === "invalid" && <p className={styles.muted} role="status">{text.invalid}</p>}
    {view.truncated && <p className={styles.muted}>{text.truncated}</p>}
    {view.results.map((result, index) => <div key={index} className={styles.result}>
      {view.results.length > 1 && <h3>{text.queryNumber} {index + 1}</h3>}
      {result.kind === "table" && isGraphOnlyTable(result, labels) ? <details className={styles.graphRows}><summary>{formatMessage(text.connectedCount, { count: result.totalRows }, locale)} · {text.inspect}</summary>{rows(result, index)}</details> : rows(result, index)}
      {result.kind === "table" && result.truncated && <p className={styles.muted}>{text.limited}</p>}
    </div>)}
    <details className={styles.details}><summary>{text.details}</summary>
      {view.diagnostic && <p>{view.diagnostic}</p>}
      {execution?.answer?.tool_trace?.length ? <details><summary>{statusText.trace}</summary><pre>{JSON.stringify(execution.answer.tool_trace, null, 2).slice(0, 16000)}</pre>{JSON.stringify(execution.answer.tool_trace).length > 16000 && <p className={styles.muted}>{text.truncated}</p>}</details> : null}
      {view.results.map((result, index) => {
        const source = result.metadata.lineage as Record<string, unknown> | undefined;
        const provenance = JSON.stringify(result.metadata.provenance ?? [], null, 2);
        const notes = Array.isArray(result.metadata.notes) ? result.metadata.notes.filter((note): note is string => typeof note === "string") : [];
        const href = typeof source?.source_url === "string" ? safeAnswerHref(source.source_url) : undefined;
        return <div key={index} className={styles.trace}>
          <dl><dt>{text.backend}</dt><dd>{result.backend ?? "—"}</dd>
            {source && <><dt>{text.source}</dt><dd>{href ? <a href={href} target="_blank" rel="noopener noreferrer">{String(source.source_url)}</a> : String(source.source_url ?? "—")}</dd><dt>{text.retrieved}</dt><dd>{String(source.retrieved_at ?? "—")}</dd><dt>{text.version}</dt><dd>{String(source.version ?? "—")}</dd><dt>{text.hash}</dt><dd><code>{String(source.sha256 ?? "—")}</code></dd></>}
          </dl>
          {notes.length > 0 && <div><h3>{text.notes}</h3><ul>{notes.map((note, i) => <li key={i}>{note}</li>)}</ul></div>}
          {typeof result.metadata.query === "string" && <details><summary>{text.query}</summary><pre>{result.metadata.query}</pre></details>}
          {provenance !== "[]" && <details><summary>{text.provenance}</summary><pre aria-label={text.preview}>{provenance.slice(0, 16000)}</pre>{provenance.length > 16000 && <p className={styles.muted}>{text.truncated}</p>}</details>}
        </div>;
      })}
      <button type="button" className={styles.download} onClick={() => download(execution)}>{text.original}</button>
    </details>
  </section>;
}
