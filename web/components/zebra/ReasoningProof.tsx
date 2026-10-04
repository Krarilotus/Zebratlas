"use client";

import { useZebraLocale } from "./Locale";
import { normalizeReasoningProof, proofSourceUrl, reasoningTermLabel } from "@/lib/zebra/reasoning-proof";
import styles from "./ReasoningProof.module.css";

const copy = {
  en: { inferred: "Why this connection", asserted: "Source assertion", relation: "is a subclass of", premises: "From these assertions", sources: "Sources", locator: "Record", hash: "Record SHA-256", sourceHash: "Source SHA-256", version: "Version", retrieved: "Retrieved", source: "Source", rule: "Transitive hierarchy", more: "Additional proof steps" },
  de: { inferred: "Warum diese Verbindung", asserted: "Quellenaussage", relation: "ist Unterklasse von", premises: "Aus diesen Aussagen", sources: "Quellen", locator: "Datensatz", hash: "Datensatz-SHA-256", sourceHash: "Quellen-SHA-256", version: "Version", retrieved: "Abgerufen", source: "Quelle", rule: "Transitive Hierarchie", more: "Weitere Beweisschritte" },
};
const engineName = "nrese";

export function ReasoningProof({ proof, labels }: { proof: unknown; labels?: ReadonlyMap<string, string> }) {
  const locale = useZebraLocale();
  const words = copy[locale === "de" ? "de" : "en"];
  const normalized = normalizeReasoningProof(proof);
  if (!normalized) return null;
  const label = (term: string) => reasoningTermLabel(term, labels);
  const head = normalized.steps[0];
  const premises = normalized.steps.filter(step => step.origin === "asserted");
  return <details className={styles.proof}>
    <summary>{normalized.origin === "inferred" ? words.inferred : words.asserted}</summary>
    <div className={styles.content}>
      <p className={styles.conclusion}><strong>{label(head.subject)}</strong> {words.relation} <strong>{label(head.object)}</strong></p>
      <p className={styles.engine}><span>{engineName}</span><span>{normalized.origin === "inferred" ? words.rule : words.asserted}</span></p>
      {normalized.origin === "inferred" && <div className={styles.premises}>
        <p>{words.premises}</p>
        <ul>{premises.slice(0, 6).map((step, index) => <li key={`${step.subject}-${step.object}-${index}`}>{label(step.subject)} → {label(step.object)}</li>)}</ul>
        {premises.length > 6 && <details><summary>{words.more}</summary><ul>{premises.slice(6).map((step, index) => <li key={index}>{label(step.subject)} → {label(step.object)}</li>)}</ul></details>}
      </div>}
      <details className={styles.sources}><summary>{words.sources}</summary>
        {normalized.records.map((record, index) => {
          const url = proofSourceUrl(record.url);
          return <div className={styles.record} key={`${record.id}-${index}`}>
            {url && <a href={url} target="_blank" rel="noreferrer">{words.source}</a>}
            <dl><div><dt>{words.locator}</dt><dd>{record.locator}</dd></div>
              {record.version && <div><dt>{words.version}</dt><dd>{record.version}</dd></div>}
              {record.retrieved && <div><dt>{words.retrieved}</dt><dd>{record.retrieved}</dd></div>}
              <div><dt>{words.hash}</dt><dd>{record.hash}</dd></div>
              <div><dt>{words.sourceHash}</dt><dd>{record.sourceHash}</dd></div>
            </dl>
          </div>;
        })}
      </details>
    </div>
  </details>;
}

export default ReasoningProof;
