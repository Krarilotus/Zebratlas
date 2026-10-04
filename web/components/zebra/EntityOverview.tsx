"use client";

import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { compactOverviewText, fetchOverview, loadOverview, normalizeOverview, overviewHref, syncOverviewAccountRevision, type EntityOverviewData, type OverviewEntity, type OverviewLoader } from "@/lib/zebra/overview";
import { ACCOUNT_CHANGED, getAccountRevision, subscribeAccountIdentity } from "@/lib/zebra/account-events";
import { getOverviewCopy } from "@/lib/zebra/overview-copy";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./EntityOverview.module.css";

function subscribeRevision(changed: () => void) {
  const notify = () => { syncOverviewAccountRevision(getAccountRevision()); changed(); };
  window.addEventListener(ACCOUNT_CHANGED, notify);
  const unsubscribe = subscribeAccountIdentity(notify);
  return () => { window.removeEventListener(ACCOUNT_CHANGED, notify); unsubscribe(); };
}
const serverRevision = () => 0;

export default function EntityOverview({ entity, initialOverview, loader = fetchOverview }: {
  entity: OverviewEntity | null; initialOverview?: unknown; loader?: OverviewLoader;
}) {
  const locale = useZebraLocale();
  const copy = useZebraCopy();
  const text = getOverviewCopy(locale);
  const accountRevision = useSyncExternalStore(subscribeRevision, getAccountRevision, serverRevision);
  const id = entity?.id;
  const kind = entity?.kind;
  const label = entity?.label;
  const identity = id && kind ? `${kind}:${id}:${locale}:${accountRevision}` : "";
  const initial = useMemo(() => id && kind && label ? normalizeOverview(initialOverview, { id, kind, label }, locale) : null, [initialOverview, id, kind, label, locale]);
  const [snapshot, setSnapshot] = useState<{ identity: string; value: EntityOverviewData; enhancing: boolean } | null>(null);
  useEffect(() => {
    if (!id || !kind || !label) return;
    const controller = new AbortController();
    void loadOverview({ id, kind, label }, locale, controller.signal, loader, (value, enhancing) => {
      if (!controller.signal.aborted) setSnapshot({ identity, value, enhancing });
    }, initial, { accountRevision: getAccountRevision });
    return () => controller.abort();
  }, [id, kind, label, locale, identity, initial, loader]);
  const value = snapshot?.identity === identity ? snapshot.value : initial;
  if (!value || !entity || !identity) return null;
  const sourceUrls = [...new Set(value.evidence.map(item => overviewHref(item.url ?? item.source_url)).filter((url): url is string => !!url))];
  let sourceLanguage = value.language;
  try { sourceLanguage = new Intl.DisplayNames([locale], { type: "language" }).of(value.language) ?? value.language; } catch { /* Preserve the actual returned language tag. */ }
  const geneIdentity = value.mode === "source" && value.entity.kind === "gene" && value.entity.official_name
    ? text.geneIdentity.replace("{symbol}", () => value.entity.label).replace("{name}", () => value.entity.official_name!) : undefined;
  const sourceCaption = value.mode === "ai" ? text.ai : geneIdentity || value.language === locale ? text.source : text.original.replace("{language}", sourceLanguage);
  return <section className={styles.overview} aria-labelledby="z-entity-overview-title">
    <h2 id="z-entity-overview-title">{copy.what}</h2>
    <p className={styles.body} lang={geneIdentity ? locale : value.language}>{geneIdentity ?? compactOverviewText(value.text, value.language)}</p>
    <div className={styles.footer}>
      <span>{sourceCaption}</span>
      {value.mode === "ai" && value.model && <span className={styles.model}>{value.model.connection} / {value.model.id}</span>}
      {sourceUrls[0] && <a href={sourceUrls[0]} target="_blank" rel="noopener noreferrer">{copy.source}</a>}
      {snapshot?.identity === identity && snapshot.enhancing && <span className={styles.pending} role="status"><ZebraLoader /><span className={styles.srOnly}>{copy.loading}</span></span>}
      <details className={styles.sources}><summary>{text.sourceDetails}</summary><div className={styles.records}>
        {value.evidence.map((source, index) => { const href = overviewHref(source.url ?? source.source_url); const locator = source.locator ?? source.record_locator ?? source.record; return <div key={index}>
          {href ? <a href={href} target="_blank" rel="noopener noreferrer">{new URL(href).hostname}</a> : <span>{source.name || source.source || source.source_id || value.source_ids[index] || copy.source}</span>}
          {locator && <span>{locator}</span>}{source.retrieved_at && <span>{source.retrieved_at}</span>}{source.version && <span>{source.version}</span>}{source.sha256 && <code>{source.sha256}</code>}
        </div>; })}
        {value.source_ids.length > 0 && <p>{value.source_ids.join(", ")}</p>}
      </div></details>
    </div>
  </section>;
}
