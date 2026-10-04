"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";

import { useEffect, useMemo, useState } from "react";
import dynamic from "next/dynamic";
import type { ConditionDetail, ExploreResponse, ExploreResult, JobsResponse } from "@/lib/zebra/types";
import type { SourceRecord } from "@/lib/types";
import { condition, jobs } from "@/lib/zebra/client";
import { useZebraCopy, useZebraKindLabel, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import { resultCopy } from "./resultCopy";
import { connectionRow, contextAnchors, evidenceOf, findingKind, findingSection, isContactAction, jobRow, safeActionUrl, safeSourceLink, sourceCaption, supplementalRows, supportedSentence, type FindingRow, type FindingSection } from "./result-overview";
import { compactResultLabel } from "./result-label";
import { ResultSnapshotCache } from "./result-cache";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./ResultOverview.module.css";
import { findingsNoteWords } from "./findings-note";

const FindingsNote = dynamic(() => import("./FindingsNote"));

type OpenFinding = (id: string, result?: ExploreResult, conditionId?: string) => void;
export type ResultOverviewProps = { data: ExploreResponse | null; results?: ExploreResult[]; query: string; busy: boolean; selectedId?: string | null; onFocus: (id: string) => void; onDetails: OpenFinding; onEvidence?: OpenFinding; onRequest: OpenFinding };
type Enrichment = { response?: ExploreResponse; key: string; details: ConditionDetail[]; geneJobs: JobsResponse | null; failed: boolean };
const enrichmentCache = new ResultSnapshotCache<Enrichment>();

export default function ResultOverview({ data, results, query, busy, onFocus, onDetails, onEvidence, onRequest }: ResultOverviewProps) {
  const locale = useZebraLocale();
  const copy = useZebraCopy();
  const words = resultCopy[zebraCopyLocale(locale)];
  const kindLabel = useZebraKindLabel();
  const anchors = useMemo(() => data ? contextAnchors(data) : { conditions: [] as string[], gene: undefined }, [data]);
  const scopeKey = `${data?.query || query}|${anchors.conditions.join(",")}|${anchors.gene || ""}|${locale}`;
  const [enrichment, setEnrichment] = useState<Enrichment>({ key: "", details: [], geneJobs: null, failed: false });
  const [showAll, setShowAll] = useState(false);
  const [noteOpen, setNoteOpen] = useState(false);
  useEffect(() => {
    if (!data || (!anchors.conditions.length && !anchors.gene)) return;
    if (enrichmentCache.get(data, scopeKey)) return;
    const controller = new AbortController();
    // At most two resolved diagnosis scopes and one resolved gene. These paths
    // read snapshot facts; questions and model-generated summary are never used.
    const requests = anchors.conditions.map(id => condition(id, locale, controller.signal, ["summary", "connections", "related", "gaps", "jobs"]));
    void Promise.allSettled([...requests, ...(anchors.gene ? [jobs(anchors.gene, "gene", controller.signal)] : [])]).then(responses => {
      if (controller.signal.aborted) return;
      const details = responses.slice(0, requests.length).flatMap(response => response.status === "fulfilled" ? [response.value as ConditionDetail] : []);
      const gene = anchors.gene ? responses[requests.length] : undefined;
      const value = { response: data, key: scopeKey, details, geneJobs: gene?.status === "fulfilled" ? gene.value as JobsResponse : null, failed: responses.some(response => response.status === "rejected") };
      enrichmentCache.set(data, scopeKey, value);
      setEnrichment(value);
    });
    return () => controller.abort();
  }, [data, anchors, locale, scopeKey]);
  const active = data ? enrichmentCache.get(data, scopeKey) || (enrichment.response === data && enrichment.key === scopeKey ? enrichment : undefined) : undefined;
  const supplements = useMemo(() => [...(active?.details.flatMap(supplementalRows) || []), ...(active?.geneJobs?.jobs.flatMap(group => group.items.map(item => jobRow(item))) || [])], [active]);
  const metadata = new Map(supplements.map(row => [row.result.id, row]));
  const returned = results || data?.results || [];
  const rows: FindingRow[] = returned.map(result => {
    const enriched = metadata.get(result.id);
    return enriched ? { ...enriched, result: { ...enriched.result, ...result, official_action: result.official_action || enriched.result.official_action, facts: result.facts?.length ? result.facts : enriched.result.facts, how_to_get: result.how_to_get || enriched.result.how_to_get, match: result.match || enriched.result.match }, sources: [...result.evidence, ...enriched.sources] } : { result, sources: result.evidence };
  });
  const visibleRows = showAll ? rows : rows.slice(0, 16);
  const officialLabel = (row: FindingRow) => {
    const action = row.result.official_action?.action || row.result.how_to_get?.route || "";
    const kind = findingKind(row.result);
    if (isContactAction(row.result)) return words.contact;
    if (["grant", "funding_call"].includes(kind)) return words.funding;
    if (["paper"].includes(kind)) return words.paper;
    if (["study", "trial", "registry", "natural_history", "observational", "expanded_access"].includes(kind)) return words.study;
    if (["asset", "model", "cell_line", "biobank", "dataset", "outcome_measure"].includes(kind)) return /request|order|apply/i.test(action) ? words.resource : words.viewResource;
    return words.official;
  };
  const matchLabel = (match?: string) => match === "exact" || match === "direct" ? words.direct : match === "related" || match === "ortholog" ? words.related : match === "broad" ? words.broad : undefined;
  const relationLabel = (kind?: string) => kind === "inferred" ? words.inferred : kind === "hypothesis" ? words.hypothesis : kind === "observed" || kind === "extracted" ? words.observed : undefined;
  const factLabel = (key: string) => ({ affiliation: words.affiliation, sponsor: words.sponsor, countries: words.country, country: words.country, year: words.year, journal: words.journal, access: words.access, licence: words.licence, mechanism: words.mechanism, biomarker: words.biomarker, symptom: words.symptom, role: words.role }[key] || key.replaceAll("_", " "));
  const sourceLinks = (sources: FindingRow["sources"], onOpen?: () => void) => {
    const unique = sources.filter((source, index) => sources.findIndex(other => other.record === source.record && other.url === source.url && other.source === source.source) === index);
    return <div className={styles.source}>{unique.slice(0, 1).map((source, index) => {
      const url = safeSourceLink(source.url);
      const caption = compactResultLabel(sourceCaption(source, words.source), 32);
      return <span key={`${source.record || source.source}-${index}`}>{url ? <a href={url} target="_blank" rel="noopener noreferrer" title={source.source}>{caption}<Icon name="external" size={10} /></a> : caption}{source.retrieved_at ? ` · ${source.retrieved_at.slice(0, 10)}` : ""}</span>;
    })}{onOpen && <button className={styles.button} onClick={onOpen}>{words.evidence}{unique.length ? ` (${unique.length})` : ""}</button>}</div>;
  };
  const renderRow = (row: FindingRow) => {
    const { result } = row;
    const action = safeActionUrl(result.official_action?.url) || safeActionUrl(result.how_to_get?.url);
    const fallback = !action ? safeSourceLink(result.url) : undefined;
    const match = result.match === "exact" || result.match === "direct" ? undefined : matchLabel(result.match);
    const relation = result.relation_kind === "observed" || result.relation_kind === "extracted" ? undefined : relationLabel(result.relation_kind);
    const open = (callback: OpenFinding) => callback(result.id, result, row.conditionId);
    return <li className={styles.row} key={`${row.conditionId || "result"}-${result.id}`}>
      <div><button className={styles.name} onClick={() => open(onDetails)} title={result.label} aria-label={result.label}>{compactResultLabel(result.label, 84)}</button>
        <div className={styles.tags}><span>{kindLabel(findingKind(result))}</span>{match && <span className={styles.match}>{match}</span>}{relation && <span>{relation}</span>}</div>
        <div className={styles.facts}>{result.holder && <span><strong>{words.holder}</strong>{compactResultLabel(result.holder.name, 80)}</span>}{result.status && <span><strong>{words.status}</strong>{copy.status[result.status as keyof typeof copy.status] || result.status.replaceAll("_", " ")}</span>}{result.facts?.filter(fact => !["id", "uri", "name", "label", "kind", "asset_kind", "organisation_kind", "study_kind", "source", "edge_id", "record_id"].includes(fact.key) && fact.value !== result.label && fact.value !== result.status).slice(0, 2).map(fact => <span key={`${fact.key}-${fact.value}`}><strong>{factLabel(fact.key)}</strong>{compactResultLabel(fact.value, 100)}</span>)}</div>
        {sourceLinks(row.sources, onEvidence ? () => open(onEvidence) : undefined)}
        {result.metadata_evidence?.length && !row.sources.length ? sourceLinks(result.metadata_evidence) : null}
      </div>
      <div className={styles.actions}>{action ? <a className={styles.action} href={action} target={action.startsWith("mailto:") ? undefined : "_blank"} rel="noopener noreferrer">{officialLabel(row)}<Icon name="external" size={12} /></a> : fallback ? <a className={styles.action} href={fallback} target="_blank" rel="noopener noreferrer">{result.kind === "paper" ? words.paper : words.source}<Icon name="external" size={12} /></a> : null}
        <button className={styles.button} onClick={() => open(onRequest)}>{words.brief}</button>
        {data?.graph.nodes.some(node => node.id === result.id) && <button className={styles.button} onClick={() => onFocus(result.id)} aria-label={words.explore} title={words.explore}><Icon name="graph" size={14} /></button>}
      </div>
    </li>;
  };
  const renderSections = (sectionRows: FindingRow[]) => <div className={styles.columns}>{(["support", "resources", "biology", "evidence"] as FindingSection[]).flatMap(section => {
    const categoryKinds = [...new Set(sectionRows.filter(row => findingSection(findingKind(row.result)) === section).map(row => findingKind(row.result)))];
    return categoryKinds.map(kind => <section key={kind}><h3 className={styles.sectionHeading}>{kindLabel(kind)}</h3><ul className={styles.rows}>{sectionRows.filter(row => findingKind(row.result) === kind).map(renderRow)}</ul></section>);
  })}</div>;
  const sourceRecords = (sources: SourceRecord[]) => sourceLinks(evidenceOf(sources));
  return <div className={styles.overview} aria-busy={busy}>
    {visibleRows.length > 0 && <><div className={styles.heading}><h2>{words.heading}</h2><span className={styles.meta}>{words.order}</span></div>{renderSections(visibleRows)}{rows.length > 16 && <button className={styles.button} onClick={() => setShowAll(value => !value)}>{showAll ? words.less : `${words.more} (${rows.length - 16})`}</button>}</>}
    {active?.details.map(detail => {
      const context = detail.summary?.condition || detail.connections?.condition || detail.related?.condition;
      const candidates = supplementalRows(detail).filter(row => !rows.some(existing => existing.result.id === row.result.id));
      const extraRows = (["support", "resources", "biology", "evidence"] as FindingSection[]).flatMap(section => candidates.filter(row => findingSection(findingKind(row.result)) === section).slice(0, 4));
      const comparisons = detail.related?.communities.slice(0, 3) || [];
      return <section className={styles.scope} key={detail.id}>
        <div className={styles.scopeHeading}><span>{words.scope}</span><h2>{context?.label || detail.id}</h2><button className={styles.button} onClick={() => onDetails(detail.id)}>{words.details}</button></div>
        {detail.summary?.medical.definition && <p className={styles.description}>{detail.summary.medical.definition}</p>}
        {extraRows.length > 0 && renderSections(extraRows)}
        {comparisons.length > 0 && <section className={styles.comparisons}><h3 className={styles.sectionHeading}>{words.comparison}</h3>{comparisons.map(item => <details className={styles.comparison} key={item.condition.id}>
          <summary><span>{item.condition.label}<span className={styles.comparisonPreview}>{[...item.shared.processes, ...item.shared.symptoms, ...item.shared.genes].slice(0, 3).map(term => term.label).join(" · ")}</span></span><span className={styles.meta}>{words.related}</span></summary>
          {item.why.filter(sentence => supportedSentence(sentence, item.sources)).slice(0, 2).map((sentence, index) => <p key={index}>{sentence.text}</p>)}
          <h4>{words.shared}</h4><p>{[...item.shared.processes, ...item.shared.symptoms, ...item.shared.genes].slice(0, 8).map(term => term.label).join(" · ")}</p>
          {item.limits?.slice(0, 1).map((sentence, index) => <p key={index}>{sentence.text}</p>)}
          {sourceRecords(item.sources)}
          <button className={styles.button} onClick={() => onRequest(item.partner?.id || detail.id, item.partner ? connectionRow(item.partner, detail.id).result : undefined, detail.id)}>{words.experiment}</button>
        </details>)}</section>}
        {detail.related?.counterexamples.length ? <section><h3 className={styles.sectionHeading}>{words.differences}</h3>{detail.related.counterexamples.slice(0, 2).map(item => <div key={item.condition.id}><p className={styles.reason}><strong>{item.condition.label}</strong>{supportedSentence(item.differs, item.sources) ? ` · ${item.differs.text}` : ""}</p>{sourceRecords(item.sources)}</div>)}</section> : null}
        {detail.gaps?.unknown.length ? <section><h3 className={styles.sectionHeading}>{words.gaps}</h3><ul className={styles.gaps}>{detail.gaps.unknown.slice(0, 3).map((sentence, index) => <li key={index}>{sentence.text}</li>)}</ul></section> : null}
      </section>;
    })}
    {active?.geneJobs && !active.details.length && <section className={styles.scope}><div className={styles.scopeHeading}><span>{words.scope}</span><h2>{active.geneJobs.subject.label}</h2></div>{renderSections(supplements.filter(row => !rows.some(existing => existing.result.id === row.result.id)).slice(0, 12))}</section>}
    {!busy && data && !active && (anchors.conditions.length > 0 || anchors.gene) && <p className={styles.status} role="status"><ZebraLoader /> {words.loading}</p>}
    {active?.failed && <p className={styles.status} role="status">{words.unavailable}</p>}
    {!busy && !rows.length && active && !supplements.length && <p className={styles.status}>{words.empty}</p>}
    {!busy && /(?:rank|ranking|prioriti|prioris|silenc|silenz)/i.test(query) && rows.length > 0 && <p className={styles.status}>{words.noRanking}</p>}
    {data && !busy && <div className={styles.note}><button className={styles.button} onClick={() => setNoteOpen(true)}>{findingsNoteWords[zebraCopyLocale(locale)].open}<Icon name="arrow" size={12} /></button>{noteOpen && <FindingsNote key={query} query={query} data={data} rows={[...rows, ...supplements.filter(row => !rows.some(existing => existing.result.id === row.result.id))]} limitations={active?.details.flatMap(detail => detail.gaps?.unknown.map(sentence => sentence.text) || [])} onClose={() => setNoteOpen(false)} />}</div>}
  </div>;
}
