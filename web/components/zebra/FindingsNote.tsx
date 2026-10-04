"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";

import Link from "next/link";
import { useMemo, useState } from "react";
import type { ExploreResponse } from "@/lib/zebra/types";
import { saveItem, ZebraApiError } from "@/lib/zebra/client";
import { zebraHref } from "@/lib/zebra/locale";
import { useZebraLocale } from "./Locale";
import { buildFindingsSnapshot, findingsNoteWords } from "./findings-note";
import type { FindingRow } from "./result-overview";
import { Icon } from "./Icon";
import styles from "./ResearchPanels.module.css";

export default function FindingsNote({ query, data, rows, limitations = [], onClose }: { query: string; data: ExploreResponse; rows: FindingRow[]; limitations?: string[]; onClose: () => void }) {
  const locale = useZebraLocale();
  const words = findingsNoteWords[zebraCopyLocale(locale)];
  const snapshot = useMemo(() => buildFindingsSnapshot(query, data, rows, zebraCopyLocale(locale), [...limitations]), [query, data, rows, locale, limitations]);
  const [editedBody, setBody] = useState<string | null>(null);
  const [editedSubject, setSubject] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [saving, setSaving] = useState(false);
  const [signin, setSignin] = useState(false);
  const body = editedBody ?? snapshot.body;
  const subject = editedSubject ?? snapshot.subject;
  const exportText = `${subject}\n\n${body}`;
  function download() {
    const href = URL.createObjectURL(new Blob([exportText], { type: "text/markdown;charset=utf-8" }));
    const link = document.createElement("a"); link.href = href; link.download = "zebratlas-findings.md"; link.click(); setTimeout(() => URL.revokeObjectURL(href), 1000);
  }
  async function copy() { try { await navigator.clipboard.writeText(exportText); setStatus(words.copied); } catch { setStatus(words.copyFailed); } }
  async function save() {
    const payload = { subject, body, query: query.slice(0, 12000), href: zebraHref("/zebra/saved", locale), findings_snapshot: snapshot.meta };
    const input = { kind: "message_draft" as const, title: subject, refs: snapshot.refs, payload };
    if (new TextEncoder().encode(JSON.stringify(input)).length > 65536) { setStatus(words.tooLarge); return; }
    setSaving(true); setSignin(false);
    try { await saveItem(input); setStatus(words.saved); }
    catch (error) { if (error instanceof ZebraApiError && error.status === 401) { setSignin(true); setStatus(words.signInNeeded); } else setStatus(words.failed); }
    finally { setSaving(false); }
  }
  return <section className={styles.panel} aria-label={words.title}>
    <header className={styles.header}><h3>{words.title}</h3><button className={styles.iconButton} onClick={onClose} aria-label={words.close}><Icon name="close" /></button></header>
    <p className={styles.status}>{words.free}{snapshot.meta.snapshot_limited ? ` · ${words.subset}` : ""}</p>
    <label className={styles.field}><span>{words.subject}</span><input value={subject} onChange={event => setSubject(event.target.value)} /></label>
    <label className={styles.field}><span>{words.body}</span><textarea value={body} onChange={event => setBody(event.target.value)} spellCheck /></label>
    <div className={styles.actions}><button className={styles.primary} onClick={save} disabled={saving}>{saving ? words.saving : words.save}</button><button onClick={download}>{words.download}</button><button onClick={copy}>{words.copy}</button></div>
    {status && <p className={styles.status} role="status">{status}</p>}{signin && <Link href={zebraHref("/zebra/account", locale)}>{words.signin}</Link>}
  </section>;
}
