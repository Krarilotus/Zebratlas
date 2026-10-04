"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";
import { useEffect, useState } from "react";
import { initiatives } from "@/lib/zebra/client";
import type { Initiative, InitiativeAction, InitiativesResponse } from "@/lib/zebra/types";
import { useZebraLocale } from "./Locale";
import { contactCopy } from "./contact-copy";
import { closedAction, missingProgrammes, programmeActions, programmePriority, programmeUrl } from "./programme-routes";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./ProgrammeRoutes.module.css";

/** General programme information is separate from an exact diagnosis match. */
export default function ProgrammeRoutes() {
  const text = contactCopy[zebraCopyLocale(useZebraLocale())];
  const [open, setOpen] = useState(false), [data, setData] = useState<InitiativesResponse | null>(null), [error, setError] = useState(false), [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!open || data) return;
    const controller = new AbortController();
    void initiatives(controller.signal).then(value => { if (!controller.signal.aborted) { setData(value); setError(false); } }).catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [open, data, attempt]);
  function source(action: InitiativeAction) {
    const url = programmeUrl(action.source?.url);
    return <details><summary>{text.details}</summary><dl><dt>{text.checked}</dt><dd>{action.source?.retrieved_at || action.retrieved_at || action.date || text.unknown}</dd><dt>{text.source}</dt><dd>{url ? <a href={url} target="_blank" rel="noopener noreferrer">{action.source?.url}</a> : text.unknown}</dd>{action.source?.record_locator && <><dt>{text.source}</dt><dd>{action.source.record_locator}</dd></>}{action.source?.version && <><dt>{text.provenance}</dt><dd>{action.source.version}</dd></>}{action.source?.sha256 && <><dt>{text.checksum}</dt><dd>{action.source.sha256}</dd></>}</dl></details>;
  }
  function record(item: Initiative, reported: boolean) {
    return <li key={`${reported ? "reference" : "programme"}:${item.id}`}><h3>{item.initiative}</h3>{reported && <p className={styles.state}>{text.unverified}{item.reason ? `: ${item.reason}` : ""}</p>}
      {!reported && <ul className={styles.actions}>{programmeActions(item).map((action, index) => { const url = programmeUrl(action.url), closed = closedAction(action); return <li key={`${action.url}:${index}`}>{closed ? <span>{text.closed}: {action.action}</span> : url ? <a href={url} target="_blank" rel="noopener noreferrer">{action.action}</a> : <span>{action.action}</span>}{action.availability && <small>{action.availability}</small>}{action.outcome && <p>{action.outcome}</p>}{source(action)}</li>; })}</ul>}
      {reported && item.reported_actions?.map((action, index) => <details key={`${action.url}:${index}`}><summary>{text.reported}: {action.action}</summary><p>{text.inactive}</p><p>{action.verification || text.unknown}{action.destination_status ? ` · ${action.destination_status}` : ""}</p><p className={styles.raw}>{action.url}</p><dl><dt>{text.checked}</dt><dd>{action.source?.retrieved_at || text.unknown}</dd>{action.source?.record_locator && <><dt>{text.source}</dt><dd>{action.source.record_locator}</dd></>}{action.source?.sha256 && <><dt>{text.checksum}</dt><dd>{action.source.sha256}</dd></>}</dl></details>)}
      <details><summary>{text.scope}</summary><p>{text.general}</p>{item.scopes?.length ? <pre>{JSON.stringify(item.scopes, null, 2)}</pre> : <p>{item.scope || text.unknown}</p>}<a href={`/zebra/api/provenance?id=${encodeURIComponent(item.id)}`} target="_blank" rel="noopener noreferrer">{text.provenance}</a></details>
    </li>;
  }
  const order = (a: Initiative, b: Initiative) => programmePriority(a.initiative) - programmePriority(b.initiative) || a.initiative.localeCompare(b.initiative);
  return <details className={styles.programmes} open={open} onToggle={event => setOpen(event.currentTarget.open)}><summary>{text.programmes}</summary>
    {open && !data && !error && <p role="status"><ZebraLoader /> {text.loading}</p>}{error && <p role="status">{text.unavailable} <button type="button" onClick={() => { setError(false); setAttempt(value => value + 1); }}>{text.retry}</button></p>}
    {data && <><ul className={styles.records}>{[...data.initiatives].sort(order).map(item => record(item, false))}{[...data.references].sort(order).map(item => record(item, true))}</ul>{missingProgrammes(data).map(name => <p key={name} className={styles.state}>{text.missing} {name}.</p>)}{!data.initiatives.length && !data.references.length && <p>{text.noRecords}</p>}</>}
  </details>;
}
