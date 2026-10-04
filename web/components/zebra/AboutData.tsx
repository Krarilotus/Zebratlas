"use client";

import { useEffect, useId, useMemo, useState } from "react";
import dynamic from "next/dynamic";
import { exploreSchema } from "@/lib/zebra/client";
import type { ExploreSchema } from "@/lib/zebra/types";
import { aboutMetadata, metadataPresent, NRESE_REPOSITORY_URL } from "@/lib/zebra/about-data";
import { HISTORICAL_ZEBRA_RELEASE } from "@/lib/zebra/published-release";
import { useZebraCopy, useZebraLocale, useZebraKindLabel } from "./Locale";
import styles from "./AboutData.module.css";
import { ZebraLoader } from "./ZebraLoader";

const TBoxDiagram = dynamic(() => import("./TBoxDiagram"), { ssr: false, loading: () => <ZebraLoader /> });

export function AboutData() {
  const words = useZebraCopy().aboutData;
  const locale = useZebraLocale(), kindLabel = useZebraKindLabel(), headingId = useId();
  const [schema, setSchema] = useState<ExploreSchema | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "unavailable">("loading");
  const [attempt, setAttempt] = useState(0);
  const [detailsOpen, setDetailsOpen] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    void exploreSchema(controller.signal).then(value => {
      if (!controller.signal.aborted) { setSchema(value); setStatus("ready"); }
    }).catch(() => { if (!controller.signal.aborted) setStatus("unavailable"); });
    return () => controller.abort();
  }, [attempt]);
  const meta = useMemo(() => aboutMetadata(schema), [schema]);
  const number = new Intl.NumberFormat(locale);
  const count = (value: number | undefined) => value === undefined ? <span className={styles.unknown}>{words.unknown}</span> : number.format(value);
  const ratio = (value: number | undefined, total: number | undefined) => value === undefined || total === undefined ? <span className={styles.unknown}>{words.notMeasured}</span>
    : words.of.replace("{count}", number.format(value)).replace("{total}", number.format(total));
  const measurementName = (key: string) => key === "atlas_sources" ? words.atlasSources : key === "connected_sources" ? words.researchSources : words.sourceRecords;
  const records = meta.visible.recordsProof.total === undefined ? meta.measures.find(item => item.key === "connected_records") : meta.visible.recordsProof;
  const evidenceName = (key: string) => key === "observed" ? words.observed : key === "inferred" ? words.inferred : key === "extracted" ? words.extracted : key === "hypothesis" ? words.hypothesis : key.replaceAll("_", " ");
  const evidenceKinds = [...new Set([...Object.keys(meta.loaded.evidence), ...Object.keys(meta.visible.evidence)])];
  const classKinds = [...new Set([...Object.keys(meta.loaded.nodes), ...Object.keys(meta.classes), ...Object.keys(meta.visible.nodes)])];
  const hasAdapter = Boolean(meta.adapter.rdfSha256 || meta.adapter.graphSha256 || meta.adapter.atlasSha256);

  return <section className={styles.data} aria-labelledby={headingId}>
    <h2 id={headingId}>{words.title}</h2>
    {status !== "ready" && <div className={styles.note} role="status">{status === "loading" && <ZebraLoader />}{status === "loading" ? words.loading : words.unavailable}
      {status === "unavailable" && <button type="button" onClick={() => { setStatus("loading"); setAttempt(value => value + 1); }}>{words.retry}</button>}
    </div>}
    {!schema && <p className={styles.note}><a href={NRESE_REPOSITORY_URL} target="_blank" rel="noopener noreferrer">{words.engineRepository}</a></p>}
    {schema && <>
      <dl className={styles.rows}>
        <div><dt>{words.clinicalAtlas}</dt><dd className={styles.inline}><span>{count(meta.atlas.diseases)} {words.conditions}</span><span>{count(meta.atlas.genes)} {words.genes}</span><span>{count(meta.atlas.hpo_terms)} {words.phenotypes}</span></dd></div>
        <div><dt>{words.researchSnapshot}</dt><dd className={styles.inline}><span>{count(meta.loaded.totalNodes)} {words.nodes}</span><span>{count(meta.loaded.connectedEdges)} {words.edges}</span></dd></div>
        <div><dt>{words.servingGraph}</dt><dd className={styles.inline}><span>{count(meta.visible.totalNodes)} {words.nodes}</span><span>{count(meta.visible.connectedEdges)} {words.edges}</span></dd></div>
        <div><dt>{words.availableSources}</dt><dd className={styles.inline}><span>{count(meta.visible.sources)} {words.sourceArtifacts}</span><span>{count(meta.visible.records)} {words.sourceRecords}</span></dd></div>
        <div><dt>{words.quality}</dt><dd className={styles.inline}><span>{ratio(metadataPresent(meta.visible.recordsProof.total, meta.visible.recordsProof.missingUrl), meta.visible.recordsProof.total)} {words.withSourceUrl}</span><span>{ratio(metadataPresent(meta.visible.recordsProof.total, meta.visible.recordsProof.missingHash), meta.visible.recordsProof.total)} {words.withChecksum}</span></dd></div>
        <div><dt>{words.store}</dt><dd>{meta.sparqlConfigured === undefined ? words.unknown : meta.sparqlConfigured ? words.configured : words.notConfigured}<a className={styles.storeLink} href={NRESE_REPOSITORY_URL} target="_blank" rel="noopener noreferrer">{words.engineRepository}</a><span className={styles.sourceMeta}>{words.equivalence}: {meta.equivalence === "unverified" ? words.unverified : meta.equivalence ?? words.unknown}</span></dd></div>
      </dl>
      <p className={styles.note}>{words.provenanceNote}</p>
    </>}
    <details onToggle={event => setDetailsOpen(event.currentTarget.open)}>
      <summary>{words.details}</summary>
      {detailsOpen && <TBoxDiagram />}
      {schema && <>
        <details>
        <summary>{words.sourceCoverage}</summary>
        <h3>{words.quality}{meta.visible.recordsProof.total !== undefined && <> · {words.available}</>}</h3>
        <dl className={styles.rows}>
          <div><dt>{words.urlGaps}</dt><dd>{ratio(records?.missingUrl, records?.total)}</dd></div>
          <div><dt>{words.hashGaps}</dt><dd>{ratio(records?.missingHash, records?.total)}</dd></div>
          <div><dt>{words.dateGaps}</dt><dd>{ratio(records?.missingDate, records?.total)}</dd></div>
        </dl>
        <p className={styles.note}>{words.retrievalNote}</p>
        <p className={styles.note}>{words.structureNote}</p>
        <h3>{words.scope}</h3>
        <p className={styles.note}>{words.edgeScope}</p>
        <dl className={styles.rows}>
          <div><dt>{words.geneAssociations}</dt><dd>{count(meta.loaded.geneEdges)}</dd></div>
          <div><dt>{words.phenotypeAssociations}</dt><dd>{count(meta.loaded.phenotypeEdges)}</dd></div>
          <div><dt>{words.absentPhenotypes}</dt><dd>{count(meta.loaded.absentPhenotypeEdges)}</dd></div>
        </dl>
        <div className={styles.tableWrap}><table>
          <thead><tr><th scope="col">{words.nodes}</th><th scope="col">{words.loaded}</th><th scope="col">{words.available}</th></tr></thead>
          <tbody>{classKinds.map(kind => <tr key={kind}><th scope="row">{kindLabel(kind)}</th><td>{count(meta.loaded.nodes[kind] ?? meta.classes[kind])}</td><td>{count(meta.visible.nodes[kind])}</td></tr>)}</tbody>
        </table></div>
        {!!evidenceKinds.length && <><h3>{words.evidenceKinds}</h3><div className={styles.tableWrap}><table>
          <thead><tr><th scope="col">{words.evidenceKinds}</th><th scope="col">{words.loaded}</th><th scope="col">{words.available}</th></tr></thead>
          <tbody>{evidenceKinds.map(kind => <tr key={kind}><th scope="row">{evidenceName(kind)}</th><td>{count(meta.loaded.evidence[kind])}</td><td>{count(meta.visible.evidence[kind])}</td></tr>)}</tbody>
        </table></div></>}
        <h3>{words.sourceArtifacts}</h3>
        <div className={styles.tableWrap}><table>
          <thead><tr><th scope="col">{words.source}</th><th scope="col">{words.records}</th><th scope="col">{words.urlGaps}</th><th scope="col">{words.hashGaps}</th><th scope="col">{words.dateGaps}</th></tr></thead>
          <tbody>{meta.measures.map(item => <tr key={item.key}><th scope="row">{measurementName(item.key)}</th><td>{count(item.total)}</td><td>{ratio(item.missingUrl, item.total)}</td><td>{ratio(item.missingHash, item.total)}</td><td>{ratio(item.missingDate, item.total)}</td></tr>)}</tbody>
        </table></div>
        {!!meta.sources.length && <ul className={styles.sources}>{meta.sources.map((source, index) => <li key={source.id ?? `${source.url}-${index}`}>
          {source.url ? <a href={source.url} target="_blank" rel="noopener noreferrer">{source.name ?? source.url}</a> : source.name}
          <div className={styles.sourceMeta}>{source.version && <span>{words.version}: {source.version}</span>}{source.retrievedAt && <span>{words.retrieved}: <time dateTime={source.retrievedAt}>{source.retrievedAt.slice(0, 10)}</time></span>}</div>
        </li>)}</ul>}
        </details>
        <details>
        <summary>{words.engineDetails}</summary>
        <dl className={styles.rows}>
          <div><dt>{words.configuration}</dt><dd>{meta.sparqlConfigured === undefined ? words.unknown : meta.sparqlConfigured ? words.configured : words.notConfigured}<a className={styles.storeLink} href={NRESE_REPOSITORY_URL} target="_blank" rel="noopener noreferrer">{words.engineRepository}</a></dd></div>
          <div><dt>{words.equivalence}</dt><dd>{meta.equivalence === "unverified" ? words.unverified : meta.equivalence ?? words.unknown}</dd></div>
          <div><dt>{words.coverage}</dt><dd>{words.notMeasured}</dd></div>
          <div><dt>{words.withheld}</dt><dd className={styles.inline}><span>{count(meta.withheld.nodes)} {words.nodes}</span><span>{count(meta.withheld.edges)} {words.edges}</span><span>{count(meta.withheld.records)} {words.sourceRecords}</span></dd></div>
          <div><dt>{words.limits}</dt><dd className={styles.inline}>{[["nodes", words.limitNodes], ["edges", words.limitEdges], ["query_bytes", words.limitQuery]].map(([key, label]) => <span key={key}>{meta.limits[key] === undefined ? words.unknown : label.replace("{count}", number.format(meta.limits[key]))}</span>)}</dd></div>
        </dl>
        {hasAdapter && <dl className={styles.rows}>
          {meta.adapter.runtimeReasoning !== undefined && <div><dt>{words.adapterReasoning}</dt><dd>{meta.adapter.runtimeReasoning ? words.enabled : words.disabled}</dd></div>}
        </dl>}
        {!!meta.limitations.length && <><h3>{words.limitsTitle}</h3><ul className={styles.sources}>{meta.limitations.map((limit, index) => <li key={index}>{limit}</li>)}</ul></>}
        </details>
      </>}
      <details>
      <summary>{words.releaseDownloads}</summary>
      <dl className={styles.rows}>
        <div><dt>{words.published}</dt><dd><span>{HISTORICAL_ZEBRA_RELEASE.version} · {HISTORICAL_ZEBRA_RELEASE.artifact}</span></dd></div>
      </dl>
      <p className={styles.note}>{words.publishedNote}</p>
      <div className={styles.actions}><a href="/zebra/api/schema" target="_blank" rel="noopener noreferrer">{words.schema}</a></div>
      <details><summary>{words.verificationDetails}</summary>
        <dl className={styles.rows}>
          <div><dt>{words.published} · {words.version}</dt><dd><code className={styles.hash}>{HISTORICAL_ZEBRA_RELEASE.revision}</code></dd></div>
          <div><dt>{HISTORICAL_ZEBRA_RELEASE.artifact} · {words.checksum}</dt><dd><code className={styles.hash}>{HISTORICAL_ZEBRA_RELEASE.sha256}</code></dd></div>
        </dl>
        {hasAdapter && <><h3>{words.adapter}</h3><dl className={styles.rows}>
          {[[words.rdfChecksum, meta.adapter.rdfSha256], [words.graphChecksum, meta.adapter.graphSha256], [words.atlasChecksum, meta.adapter.atlasSha256]].filter(([, hash]) => hash).map(([label, hash]) => <div key={label}><dt>{label}</dt><dd><code className={styles.hash}>{hash}</code></dd></div>)}
        </dl></>}
      </details>
      </details>
    </details>
  </section>;
}
