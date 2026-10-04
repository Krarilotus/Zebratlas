"use client";

import Link from "next/link";
import { classifyExploreError } from "@/lib/zebra/explore-error";
import { zebraHref } from "@/lib/zebra/locale";
import type { IndexedLookupMatch } from "@/lib/zebra/indexed-lookup";
import { useZebraCopy, useZebraLocale } from "./Locale";

export function ExploreError({ error, onRetry, onIndexedLookup, matches, correctedQuery, indexedBusy, onSelectMatch, compact = false }: {
  error: unknown; onRetry?: () => void; onIndexedLookup?: () => void; matches?: IndexedLookupMatch[]; correctedQuery?: string; indexedBusy?: boolean; onSelectMatch?: (match: IndexedLookupMatch) => void; compact?: boolean;
}) {
  const copy = useZebraCopy(), locale = useZebraLocale();
  const presentation = classifyExploreError(error), words = copy.exploreError;
  const title = presentation.kind === "quota" ? words.quotaTitle : presentation.kind === "provider" ? words.providerTitle : words.title;
  const body = presentation.kind === "quota" ? words.quotaBody : presentation.kind === "provider" ? words.providerBody : presentation.kind === "unsupported" ? words.unsupportedBody : presentation.kind === "invalid" ? words.invalidBody : words.body;
  return <div className={compact ? "z-section" : "z-result-status"} role="alert">
    {compact ? <strong>{title}</strong> : <h2>{title}</h2>}<p>{body}</p>
    <div className="z-inline-actions">
      {presentation.modelSettings && <Link className="z-text-button" href={zebraHref("/zebra/account", locale)}>{words.settings}</Link>}
      {onIndexedLookup && <button type="button" className="z-text-button" disabled={indexedBusy} onClick={onIndexedLookup}>{words.lookup}</button>}
      {presentation.retryable && onRetry && <button type="button" className="z-button" onClick={onRetry}>{copy.retry}</button>}
    </div>
    {indexedBusy && <p role="status">{copy.loading}</p>}
    {correctedQuery && <p>{words.suggestedName}: <strong>{correctedQuery}</strong></p>}
    {!!matches?.length && onSelectMatch && <ul className="z-simple-list">{matches.slice(0, 10).map(match => <li key={match.id}><button type="button" className="z-text-button" onClick={() => onSelectMatch(match)}>{match.label} <small>({match.match === "fuzzy" ? words.possibleMatch : words.exactMatch})</small></button></li>)}</ul>}
  </div>;
}
