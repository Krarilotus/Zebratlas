"use client";

import { ZebraLoader } from "./ZebraLoader";
import { useState } from "react";
import type { Coverage, Edge, Evidence, LlmCall, SourceRecord, Validator } from "@/lib/types";
import type { GraphEdge, GraphEvidence, ProvChain } from "@/lib/zebra/types";
import { provenance, rdfExportHref, verify } from "@/lib/zebra/client";
import { useZebraCatalog, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import styles from "./ResearchPanels.module.css";
import { sourceDetailCopy } from "./sourceDetailCopy";
import { graphSourceRecord, originalProvenance, sourceProvenanceIds } from "./source-provenance";
import { safeSourceLink } from "./result-overview";

/** Only public web URLs are accepted as evidence destinations. */
export function safeSourceUrl(value?: string | null): string | undefined {
  return safeSourceLink(value || undefined);
}

export type SourcesProps = {
  evidence?: GraphEvidence[]; sources?: SourceRecord[]; edges?: Edge[];
  assertions?: GraphEdge[]; coverage?: Coverage[]; limits?: string[]; conflicts?: string[];
  relation?: string; conditionId?: string; entityId?: string; edgeIds?: string[]; onClose?: () => void;
  llm?: LlmCall[]; validator?: Validator;
};

function downloadProvenance(id: string, value: unknown) {
  const href = URL.createObjectURL(new Blob([JSON.stringify(value, null, 2)], { type: "application/json;charset=utf-8" }));
  const link = document.createElement("a"); link.href = href; link.download = `zebratlas-provenance-${id.replace(/[^\w.-]/g, "_")}.json`; link.click();
  setTimeout(() => URL.revokeObjectURL(href), 1000);
}

/** Keep each study/reference attached to its actual source assertion. */
function EvidenceRecords({ items }: { items: Evidence[] }) {
  const { sourceWords: words } = useZebraCatalog();
  const text = sourceDetailCopy[useZebraLocale()];
  return <ol className={styles.sourceList}>{items.map((item, index) => <li key={`${item.record || item.source}-${index}`}>
    {safeSourceUrl(item.url) ? <a href={safeSourceUrl(item.url)} target="_blank" rel="noopener noreferrer" className={styles.sourceLink}>{item.source}<Icon name="external" size={12} /></a> : <strong>{item.source}</strong>}{item.quote && <blockquote>{item.quote}</blockquote>}
    <dl className={styles.metadata}>
      {item.record && <><dt>{words.record}</dt><dd>{item.record}</dd></>}
      {item.locator && <><dt>{text.locator}</dt><dd>{item.locator}</dd></>}
      {item.date && <><dt>{text.date}</dt><dd>{item.date}</dd></>}
      {item.evidence_code && <><dt>{text.code}</dt><dd>{item.evidence_code}</dd></>}
      {item.frequency && <><dt>{text.frequency}</dt><dd>{item.frequency}</dd></>}
      {typeof item.confidence === "number" && <><dt>{text.confidence}</dt><dd>{String(item.confidence)}</dd></>}
      {item.version && <><dt>{words.version}</dt><dd>{item.version}</dd></>}
      {item.sha256 && <><dt>{words.hash}</dt><dd><code>{item.sha256}</code></dd></>}
      {item.sha256_scope && <><dt>{text.hashScope}</dt><dd>{item.sha256_scope}</dd></>}
      {item.generated_by && <><dt>{text.generated}</dt><dd>{item.generated_by}</dd></>}
      {!!item.references.length && <><dt>{text.references}</dt><dd>{item.references.map((reference, position) => <span key={`${reference}-${position}`} className={styles.reference}>{safeSourceUrl(reference) ? <a href={safeSourceUrl(reference)} target="_blank" rel="noopener noreferrer">{reference}</a> : reference}</span>)}</dd></>}
    </dl>
  </li>)}</ol>;
}

function Trace({ id }: { id: string }) {
  const { copy, sourceWords: words } = useZebraCatalog();
  const text = sourceDetailCopy[useZebraLocale()];
  const [chain, setChain] = useState<ProvChain | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [check, setCheck] = useState("");
  const [checking, setChecking] = useState(false);
  async function load() {
    setBusy(true); setError("");
    try { setChain(await provenance(id)); }
    catch { setError(words.failed); }
    finally { setBusy(false); }
  }
  async function checkIntegrity() {
    setChecking(true);
    try { const result = await verify(id); setCheck(`${words[result.state]} · ${result.checked_at}${result.summary ? ` · ${result.summary}` : ""}`); }
    catch { setCheck(words.checkFailed); }
    finally { setChecking(false); }
  }
  return <div className={styles.trace}>
    <code>{id}</code>
    {!chain && <button type="button" onClick={load} disabled={busy}>{busy ? <><ZebraLoader /> {words.loading}</> : words.provenance}</button>}
    {error && <p role="status">{error}</p>}
    {chain && <>
      {chain.edge && <dl className={styles.metadata}>
        <dt>{words.relation}</dt><dd>{chain.edge.from} → {chain.edge.relation} → {chain.edge.to}</dd>
        {chain.edge.kind && <><dt>{words.edgeKind}</dt><dd>{chain.edge.kind}</dd></>}
        {chain.edge.reason && <><dt>{words.rationale}</dt><dd>{chain.edge.reason}</dd></>}
      </dl>}
      {!!chain.edge?.records.length && <><h4>{words.lineageRecords}</h4>{chain.edge.records.map((record, index) => <dl key={`${record.record}-${index}`} className={styles.metadata}>
        <dt>{words.record}</dt><dd>{record.record}{record.locator ? ` · ${record.locator}` : ""}</dd>
        {safeSourceUrl(record.url) && <><dt>{copy.evidenceUrl}</dt><dd><a href={safeSourceUrl(record.url)} target="_blank" rel="noopener noreferrer">{record.url}</a></dd></>}
        {record.fetched_at && <><dt>{words.retrieved}</dt><dd>{record.fetched_at}</dd></>}
        {record.sha256 && <><dt>{words.hash}</dt><dd><code>{record.sha256}</code></dd></>}
      </dl>)}</>}
      {chain.entities.length > 0 && <><h4>{words.sourceFiles}</h4>{chain.entities.map(entity => <dl key={entity["@id"]} className={styles.metadata}>
        <dt>{words.record}</dt><dd>{entity["@id"]}</dd>
        {safeSourceUrl(entity["prov:atLocation"]) && <><dt>{copy.evidenceUrl}</dt><dd><a href={safeSourceUrl(entity["prov:atLocation"])} target="_blank" rel="noopener noreferrer">{entity["prov:atLocation"]}</a></dd></>}
        {entity.file && <><dt>{text.file}</dt><dd>{entity.file}</dd></>}
        {typeof entity.bytes === "number" && <><dt>{text.bytes}</dt><dd>{entity.bytes}</dd></>}
        {entity.version && <><dt>{words.version}</dt><dd>{entity.version}</dd></>}
        {entity.retrieved_at && <><dt>{words.retrieved}</dt><dd>{entity.retrieved_at}</dd></>}
        {entity.sha256 && <><dt>{words.hash}</dt><dd><code>{entity.sha256}</code></dd></>}
        {entity.licence && <><dt>{words.licence}</dt><dd>{entity.licence}</dd></>}
      </dl>)}</>}
      {chain.activities.length > 0 && <><h4>{words.trace}</h4><ol className={styles.steps}>{chain.activities.map(activity => <li key={activity["@id"]}>
        <span>{activity["rdfs:label"] || activity["@id"]}</span>
        {activity["prov:startedAtTime"] && <small>{activity["prov:startedAtTime"]}</small>}
        {activity["prov:endedAtTime"] && <small>{text.ended}: {activity["prov:endedAtTime"]}</small>}
        {activity["prov:wasAssociatedWith"]?.name && <small>{[activity["prov:wasAssociatedWith"]?.name, activity["prov:wasAssociatedWith"]?.version].filter(Boolean).join(" · ")}</small>}
        {activity["prov:wasAssociatedWith"]?.commit && <small>{text.commit}: {activity["prov:wasAssociatedWith"]?.commit}</small>}
        {!!activity["prov:used"]?.length && <small>{text.inputs}: {activity["prov:used"]?.join(" · ")}</small>}
      </li>)}</ol></>}
      {!chain.edge && chain.entities.length === 0 && chain.activities.length === 0 && <p>{words.noProvenance}</p>}
      <button type="button" disabled={checking} onClick={checkIntegrity}>{checking ? <><ZebraLoader /> {words.checking}</> : words.check}</button>
      {check && <p role="status">{check}</p>}
      <details><summary>{text.original}</summary><pre className={styles.proof}>{JSON.stringify(originalProvenance(chain), null, 2)}</pre><button onClick={() => downloadProvenance(id, originalProvenance(chain))}>{text.download}</button></details>
    </>}
  </div>;
}

export default function Sources({ evidence = [], sources = [], edges = [], assertions = [], coverage = [], limits = [], conflicts = [], relation, conditionId, entityId, edgeIds = [], onClose, llm = [], validator }: SourcesProps) {
  const { copy, sourceWords: words } = useZebraCatalog();
  const text = sourceDetailCopy[useZebraLocale()];
  const records = [...sources.map(source => ({ ...source, record: source.record || source.locator || source.id, date: source.retrieved_on })),
    ...evidence.map((evidence, index) => { const source = graphSourceRecord(evidence); return { ...source, id: source.record || `evidence-${index + 1}`, date: source.retrieved_at, tier: undefined, statement: undefined, quote: undefined }; })]
    .filter((source, index, all) => all.findIndex(other => `${other.id}|${other.url || ""}` === `${source.id}|${source.url || ""}`) === index);
  const ids = sourceProvenanceIds(assertions, edgeIds, entityId, conditionId);
  return <section className={styles.panel} aria-label={copy.sources}>
    <header className={styles.header}><h3>{copy.sources}</h3>{onClose && <button type="button" className={styles.iconButton} aria-label={copy.close} onClick={onClose}><Icon name="close" /></button>}</header>
    {relation && <p className={styles.muted}>{relation}</p>}
    {records.length === 0 && <p className={styles.muted}>{copy.noSources}</p>}
    <ol className={styles.sourceList}>{records.map(source => {
      const href = safeSourceUrl(source.url);
      return <li key={`${source.id}|${source.url || ""}`}>
        {href ? <a href={href} target="_blank" rel="noopener noreferrer" className={styles.sourceLink}>{source.source}<Icon name="external" size={14} /></a> : <strong>{source.source}</strong>}
        <div className={styles.sourceMeta}><span>{source.date || words.unknownDate}</span><span>{source.tier ? `${words[source.tier]} · ${words.tier.toLowerCase()}` : words.unknownTier}</span></div>
        {source.statement && <p>{source.statement}</p>}
        {source.quote && <blockquote lang={source.quote.lang}>{source.quote.text}</blockquote>}
        <details><summary>{copy.details}</summary><dl className={styles.metadata}>
          <dt>{words.record}</dt><dd>{source.record || source.id}</dd>
          {source.assertion && <><dt>{text.assertion}</dt><dd>{source.assertion}</dd></>}
          {source.kind && <><dt>{words.edgeKind}</dt><dd>{source.kind}</dd></>}
          {source.references?.length ? <><dt>{text.references}</dt><dd>{source.references.map((reference, index) => <span className={styles.reference} key={`${reference}-${index}`}>{safeSourceUrl(reference) ? <a href={safeSourceUrl(reference)} target="_blank" rel="noopener noreferrer">{reference}</a> : reference}</span>)}</dd></> : null}
          {href && <><dt>{copy.evidenceUrl}</dt><dd><a href={href} target="_blank" rel="noopener noreferrer">{href}</a></dd></>}
          {source.version && <><dt>{words.version}</dt><dd>{source.version}</dd></>}
          {source.sha256 && <><dt>{words.hash}</dt><dd><code>{source.sha256}</code></dd></>}
          {source.sha256_scope && <><dt>{text.hashScope}</dt><dd>{source.sha256_scope}</dd></>}
          {source.generated_by && <><dt>{text.generated}</dt><dd>{source.generated_by}</dd></>}
        </dl></details>
      </li>;
    })}</ol>
    {edges.length > 0 && <details className={styles.section}><summary>{words.relation}</summary>{edges.map((edge, index) => <div className={styles.trace} key={`${edge.from}-${edge.to}-${index}`}>
      <p>{edge.from} → {edge.relation} → {edge.to}</p><small>{edge.kind}</small>{edge.why && <p>{edge.why}</p>}
      {edge.evidence.length > 0 && <><h4>{text.support}</h4><EvidenceRecords items={edge.evidence} /></>}
      {!!edge.contradicted_by?.length && <><h4>{words.counter}</h4><EvidenceRecords items={edge.contradicted_by} /></>}
    </div>)}</details>}
    {assertions.length > 0 && <details className={styles.section}><summary>{text.assertion}</summary>{assertions.map(assertion => <div className={styles.trace} key={assertion.id}>
      <code>{assertion.id}</code><p>{assertion.source} → {assertion.relation} → {assertion.target}</p>{assertion.kind && <small>{assertion.kind}</small>}{assertion.reason && <p>{assertion.reason}</p>}
      {assertion.proof !== undefined && <details><summary>{text.proof}</summary><pre className={styles.proof}>{JSON.stringify(assertion.proof, null, 2)}</pre></details>}
    </div>)}</details>}
    {(coverage.length > 0 || limits.length > 0 || conflicts.length > 0) && <details className={styles.section}><summary>{text.coverage}</summary>
      {coverage.map((item, index) => <dl className={styles.metadata} key={`${item.source}-${index}`}><dt>{words.records}</dt><dd>{item.source}</dd>{item.searched_on && <><dt>{text.checked}</dt><dd>{item.searched_on}</dd></>}<dt>{text.found}</dt><dd>{item.found}</dd>{typeof item.covered === "boolean" && <><dt>{text.covered}</dt><dd>{item.covered ? words.yes : words.no}</dd></>}{item.note && <><dt>{text.limits}</dt><dd>{item.note}</dd></>}</dl>)}
      {limits.length > 0 && <><h4>{text.limits}</h4>{limits.map((limit, index) => <p key={index}>{limit}</p>)}</>}
      {conflicts.length > 0 && <><h4>{text.conflicts}</h4>{conflicts.map((conflict, index) => <p key={index}>{conflict}</p>)}</>}
    </details>}
    {(ids.length > 0 || llm.length > 0 || validator) && <details className={styles.section}><summary>{words.technical}</summary>
      {conditionId && <a href={rdfExportHref(conditionId)} download className={styles.sourceLink}>{`${copy.export} · RDF`}<Icon name="external" size={14} /></a>}
      {validator && <><h4>{words.validator}</h4><p>{validator.ok ? words.passed : words.review} · {validator.checked}</p>{validator.notes.map((note, index) => <p key={index}>{note}</p>)}</>}
      {llm.map((call, index) => <dl key={`${call.purpose}-${index}`} className={styles.metadata}>
        <dt>{words.model}</dt><dd>{call.purpose} · {call.origin}</dd>
        {call.model && <><dt>{words.version}</dt><dd>{call.model}</dd></>}
        {call.provider && <><dt>{words.provider}</dt><dd>{call.provider}</dd></>}
        {typeof call.cached === "boolean" && <><dt>{words.cached}</dt><dd>{call.cached ? words.yes : words.no}</dd></>}
        <dt>{words.trace}</dt><dd>{call.activities.join(" · ") || copy.unknown}</dd>
      </dl>)}
      {ids.map(id => <Trace key={id} id={id} />)}
    </details>}
  </section>;
}
