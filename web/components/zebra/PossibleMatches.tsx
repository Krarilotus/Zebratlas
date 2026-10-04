"use client";

import type { ExploreNameCandidate } from "@/lib/zebra/types";
import { useZebraCopy, useZebraKindLabel } from "./Locale";
import styles from "./PossibleMatches.module.css";

export default function PossibleMatches({ matches, invalid = false, busy = false, onSelect }: { matches: ExploreNameCandidate[]; invalid?: boolean; busy?: boolean; onSelect: (match: ExploreNameCandidate) => void }) {
  const words = useZebraCopy().possibleMatches, kindLabel = useZebraKindLabel();
  return <section className={styles.matches} aria-label={words.title}>
    <header><h2>{words.title}</h2><span>{words.scope}</span></header>
    {invalid ? <p role="status">{words.unavailable}</p> : !matches.length ? <p role="status">{words.empty}</p> : <ul>{matches.map(match => <li key={match.id}><button type="button" disabled={busy} onClick={() => onSelect(match)} aria-label={`${match.label} · ${words.select}`} title={match.label}><span><strong>{match.label}</strong><small>{kindLabel(match.kind)}</small></span><span className={styles.action}>{words.select}<span aria-hidden="true"> →</span></span></button></li>)}</ul>}
  </section>;
}
