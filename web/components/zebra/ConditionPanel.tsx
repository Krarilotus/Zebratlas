"use client";

import { useState } from "react";
import type { ConditionDetail, ExploreResult, JobItem } from "@/lib/zebra/types";
import type { Connection, SourceRecord } from "@/lib/types";
import { useZebraCopy, useZebraKindLabel } from "./Locale";
import { Icon } from "./Icon";
import { ZebraLoader } from "./ZebraLoader";

function safeUrl(value?: string | null): string | undefined {
  if (!value) return;
  try { const url = new URL(value); if (["https:", "http:"].includes(url.protocol) && !url.username && !url.password) return url.href; } catch { /* Unsupported source route. */ }
}

function sourcesOf(sources: SourceRecord[]) { return sources.map((source) => ({ source: source.source, url: source.url, record: source.locator || source.id, retrieved_at: source.retrieved_on, sha256: source.sha256 })); }
export function resultOf(connection: Connection): ExploreResult { return { id: connection.id, label: connection.name, kind: connection.kind, reason: connection.why.text, score: 0, evidence: sourcesOf(connection.sources), url: connection.channels[0]?.url }; }
function Status({ value }: { value?: string }) { const copy = useZebraCopy(); return value ? <span className={value === "recruiting" ? "z-status z-status-open" : "z-status"}>{copy.status[value as keyof typeof copy.status] ?? value.replaceAll("_", " ")}</span> : null; }

export function ConditionPanel({ detail, selected, busy, sectionLoading = false, onLoadSection, onSelect, onEvidence, onRequest }: { detail: ConditionDetail | null; selected: ExploreResult | null; busy: boolean; sectionLoading?: boolean; onLoadSection?: (section: string) => void; onSelect: (id: string, result?: ExploreResult) => void; onEvidence: () => void; onRequest: () => void }) {
  const copy = useZebraCopy();
  const kindLabel = useZebraKindLabel();
  const [section, setSection] = useState("overview");
  const all = [...detail?.connections?.exact ?? [], ...detail?.connections?.related ?? []];
  const connection = all.find((item) => item.id === selected?.id);
  const nativeRoute = safeUrl(selected?.official_action?.url ?? selected?.how_to_get?.url);
  const sourceEvidence = [...selected?.evidence ?? [], ...selected?.metadata_evidence ?? []];
  const isCondition = selected?.kind === "disease";
  const groups = all.filter((item) => ["patient_group", "organisation", "expert_centre"].includes(item.kind));
  const studies = all.filter((item) => ["trial", "registry", "natural_history", "observational", "expanded_access"].includes(item.kind));
  const people = all.filter((item) => item.kind === "researcher");
  const assets = all.filter((item) => ["grant", "registry", "natural_history"].includes(item.kind));
  const hasConnections = !!detail?.connections;
  const exactGroups = groups.filter((item) => item.kind === "patient_group" && item.match === "exact");
  const overviewGroups = [...exactGroups, ...groups.filter((item) => !exactGroups.includes(item))];
  const loaded = (name: string) => detail?.loadedSections?.some((item) => item === name) === true;
  const jobs = detail?.jobs?.jobs ?? [];
  const jobItem = jobs.flatMap((job) => job.items).find((item) => item.id === selected?.id);
  const jobTabs = [
    { key: "therapy_programmes", label: copy.therapies },
    { key: "free_papers", label: copy.papers },
    { key: "funding", label: copy.funders },
    { key: "outcome_measures", label: copy.outcomes },
  ];
  const sectionAvailable = section === "related" ? loaded("related") : section === "questions" ? loaded("questions") : jobTabs.some((tab) => tab.key === section) ? loaded("jobs") : section === "assets" ? loaded("jobs") || hasConnections : section === "gaps" ? !!detail?.gaps || hasConnections : section === "overview" ? !!detail?.summary || hasConnections : hasConnections;
  function chooseSection(next: string) { setSection(next); onLoadSection?.(next); }
  function jobResult(item: JobItem): ExploreResult {
    return { id: item.id, label: item.what.label, kind: item.what.kind, reason: item.via.reason ?? item.via.relation.replaceAll("_", " "), score: 0,
      url: safeUrl(item.how_to_get.url), facts: item.facts, holder: item.holder ?? undefined, how_to_get: item.how_to_get, evidence: item.source ? [{ source: item.source.name, url: safeUrl(item.source.url), record: item.source.record, retrieved_at: item.source.retrieved_at, sha256: item.source.sha256 }] : [] };
  }
  function jobFacts(item: JobItem) {
    return <>
      <dl className="z-dl"><dt>{copy.holder}</dt><dd>{item.holder?.name || copy.unknown}</dd>
        {item.facts.map((fact, index) => <div key={`${fact.key}-${index}`}><dt>{fact.key.replaceAll("_", " ")}</dt><dd>{fact.value || copy.unknown}</dd></div>)}
        <dt>{copy.checked}</dt><dd>{item.source?.retrieved_at?.slice(0, 10) || copy.unknown}</dd>
        <dt>{copy.licence}</dt><dd>{item.licence?.id || item.licence?.class || copy.unknown}</dd></dl>
      {item.how_to_get.note && <p className="z-muted">{item.how_to_get.note}</p>}
      <div className="z-inline-actions">{safeUrl(item.how_to_get.url) && <a className="z-text-button" href={safeUrl(item.how_to_get.url)} target="_blank" rel="noopener noreferrer">{copy.requestRoute}<Icon name="external" size={14} /></a>}
        {safeUrl(item.source?.url) && <a className="z-text-button" href={safeUrl(item.source?.url)} target="_blank" rel="noopener noreferrer">{copy.source}<Icon name="external" size={14} /></a>}</div>
    </>;
  }
  function renderJob(name: string) {
    const job = jobs.find((item) => item.job === name);
    if (!job?.items.length) return <p className="z-empty-note">{copy.noData}</p>;
    return job.items.map((item) => <section className="z-section" key={item.id}>
      <button className="z-related-title" onClick={() => onSelect(item.id, jobResult(item))}>{item.what.label}<Icon name="chevron" size={15} /></button>
      <p className="z-row-meta">{kindLabel(item.what.kind)}</p>
      <p className="z-muted">{item.via.reason || item.via.relation.replaceAll("_", " ")}</p>
      {jobFacts(item)}
    </section>);
  }
  const initiatives = detail?.initiatives ?? [];
  function renderInitiatives() {
    return initiatives.length > 0 && <section className="z-section"><h3>{copy.alreadyWorking}</h3>{initiatives.map((item) => <div className="z-section" key={item.id}>
      <strong>{item.initiative}</strong>{item.description && <p className="z-muted">{item.description}</p>}
      <p className="z-match-note">{item.scope.replaceAll("_", " ")}</p>
      {item.official_action && <><p>{item.official_action.outcome}</p>{safeUrl(item.official_action.url) && <a className="z-text-button" href={safeUrl(item.official_action.url)} target="_blank" rel="noopener noreferrer">{item.official_action.action}<Icon name="external" size={14} /></a>}
        <details className="z-coverage"><summary>{copy.sourceDetails}</summary><p>{item.official_action.availability || copy.unknown}</p><p>{item.official_action.retrieved_at?.slice(0, 10) || item.official_action.date || copy.unknown}</p>{safeUrl(item.official_action.source?.url) && <a className="z-text-button" href={safeUrl(item.official_action.source?.url)} target="_blank" rel="noopener noreferrer">{copy.source}<Icon name="external" size={14} /></a>}</details></>}
    </div>)}</section>;
  }
  function renderConnections(items: Connection[], compact = false) { return items.length ? <ul className="z-simple-list">{items.map((item) => <li key={item.id}><button className="z-connection-row" onClick={() => onSelect(item.id, resultOf(item))}><span>{!compact && <span className="z-eyebrow">{kindLabel(item.kind)}</span>}<strong>{item.name}</strong>{!compact && <span>{item.why.text}</span>}<span className="z-row-meta">{[item.affiliation || item.sponsor, item.countries.join(" · ")].filter(Boolean).join(" · ")}</span>{["patient_group", "organisation", "expert_centre"].includes(item.kind) && <span className="z-match-note">{item.match === "exact" ? copy.exact : item.match === "broad" ? copy.broad : copy.relatedMatch}</span>}<Status value={item.status} /></span><Icon name="chevron" size={15} /></button></li>)}</ul> : <p className="z-empty-note">{copy.noData}</p>; }
  if (busy) return <div className="z-inline-loading" role="status"><ZebraLoader />{copy.loading}</div>;
  return <>
    {!isCondition && <div className="z-entity-overview">
      {nativeRoute ? <a className="z-button z-button-primary" href={nativeRoute} target="_blank" rel="noopener noreferrer">{selected?.official_action?.action || copy.requestRoute}<Icon name="external" size={15} /></a> : safeUrl(selected?.url) && <a className="z-button z-button-primary" href={safeUrl(selected?.url)} target="_blank" rel="noopener noreferrer">{connection ? copy.contact : copy.viewDetails}<Icon name="external" size={15} /></a>}
      {selected?.official_action?.outcome && <p>{selected.official_action.outcome}</p>}
      {selected?.how_to_get?.note && <p className="z-muted">{selected.how_to_get.note}</p>}
      {selected?.status && !connection && <Status value={selected.status} />}
      {(selected?.holder || selected?.facts?.length) && <dl className="z-dl">{selected.holder && <><dt>{copy.holder}</dt><dd>{selected.holder.name}</dd></>}{selected.facts?.map((fact, index) => <div key={`${fact.key}-${index}`}><dt>{fact.key.replaceAll("_", " ")}</dt><dd>{fact.value || copy.unknown}</dd></div>)}</dl>}
      {connection && <><p className="z-row-meta">{[connection.affiliation || connection.sponsor, connection.countries.join(" · ")].filter(Boolean).join(" · ")}</p><Status value={connection.status} /><p className="z-match-note">{connection.match === "exact" ? copy.exact : connection.match === "broad" ? copy.broad : copy.relatedMatch}</p></>}
      <dl className="z-dl"><dt>{copy.source}</dt><dd>{[...new Set(sourceEvidence.map((item) => item.source))].join(" · ") || copy.unknown}</dd><dt>{copy.checked}</dt><dd>{sourceEvidence.find((item) => item.retrieved_at)?.retrieved_at?.slice(0, 10) || copy.unknown}</dd></dl>
      <div className="z-inline-actions"><button className="z-text-button" onClick={onEvidence}>{copy.evidence}<Icon name="arrow" size={14} /></button><button className="z-text-button" onClick={onRequest}>{copy.draft}<Icon name="arrow" size={14} /></button></div>
      {jobItem && jobFacts(jobItem)}
      {["asset", "model", "biobank"].includes(selected?.kind ?? "") && !selected?.facts?.length && !jobItem && <dl className="z-dl"><dt>{copy.suitability}</dt><dd>{copy.notAssessed}</dd><dt>{copy.availability}</dt><dd>{copy.unknown}</dd><dt>{copy.access}</dt><dd>{copy.notAssessed}</dd></dl>}
      {detail?.summary && <p className="z-context-label">{detail.summary.condition.label}</p>}
    </div>}
    {detail && <>
      <div className="z-section-select"><select aria-label={copy.details} value={section} onChange={(event) => chooseSection(event.target.value)}>{[{ key: "overview", label: copy.summary }, { key: "groups", label: copy.groups }, { key: "studies", label: copy.studies }, { key: "people", label: copy.people }, { key: "related", label: copy.related }, { key: "assets", label: copy.assets }, { key: "questions", label: copy.questions }, { key: "gaps", label: copy.gaps }, ...jobTabs].map((tab) => <option key={tab.key} value={tab.key}>{tab.label}</option>)}</select></div>
      {sectionLoading ? <div className="z-inline-loading" role="status"><ZebraLoader />{copy.loading}</div> : !sectionAvailable ? <p className="z-empty-note">{copy.noCoverage}</p> : <>
      {section === "overview" && <div className="z-condition-overview">
        <section><h3>{copy.what}</h3>{detail.summary?.what.length ? detail.summary.what.map((sentence, index) => <p key={index}>{sentence.text}</p>) : detail.summary?.medical.definition ? <p lang={detail.summary.lang}>{detail.summary.medical.definition.split(/(?<=[.!?])\s+/).slice(0, 2).join(" ")}</p> : <p className="z-muted">{copy.noCoverage}</p>}</section>
        <section><h3>{copy.notAlone}</h3>{detail.summary?.not_alone && <p>{detail.summary.not_alone.text}</p>}{overviewGroups.length ? renderConnections(overviewGroups.slice(0, 1), true) : <p className="z-muted">{hasConnections ? copy.noneExactGroup : copy.noCoverage}</p>}</section>
        <section><h3>{copy.thisWeek}</h3>{studies.length ? renderConnections(studies.slice(0, 1), true) : <p className="z-muted">{hasConnections ? copy.noneExactStudy : copy.noCoverage}</p>}<button className="z-text-button" onClick={() => chooseSection("gaps")}>{copy.startPath}<Icon name="arrow" size={14} /></button></section>
        {renderInitiatives()}
        <button className="z-text-button" onClick={onEvidence}>{copy.evidence}<Icon name="arrow" size={14} /></button>
      </div>}
      {jobTabs.some((tab) => tab.key === section) && renderJob(section)}
      {section === "groups" && <>{renderConnections(groups)}{renderInitiatives()}</>}
      {section === "studies" && <>{renderConnections(studies)}<p className="z-fine-print">{copy.siteUnknown}</p></>}
      {section === "people" && <>{renderConnections(people)}{detail.related?.people.map((bridge) => <div key={bridge.person.id} className="z-section"><h3>{bridge.person.label}</h3><p className="z-muted">{bridge.institution}</p>{bridge.facts?.map((fact, index) => <p key={index}>{fact.text}</p>)}<p>{bridge.communities.map((item) => item.label).join(" · ")}</p>{safeUrl(bridge.channel?.url) && <a className="z-text-button" href={safeUrl(bridge.channel?.url)} target="_blank" rel="noopener noreferrer">{copy.contact}<Icon name="external" size={14} /></a>}</div>)}</>}
      {section === "assets" && <>{renderJob("models_samples")}{renderConnections(assets)}{detail.related?.communities.flatMap((item) => item.assets).map((asset) => <div className="z-section" key={asset.id}><p className="z-eyebrow">{kindLabel(asset.type)}</p><h3>{asset.title}</h3><Status value={asset.status} /><p className="z-muted">{asset.sponsor}</p><p>{asset.covers.map((item) => item.label).join(" · ")}</p>{safeUrl(asset.url) && <a className="z-text-button" href={safeUrl(asset.url)} target="_blank" rel="noopener noreferrer">{copy.viewDetails}<Icon name="external" size={14} /></a>}</div>)}</>}
      {section === "related" && <>
        {detail.related?.communities.length ? detail.related.communities.map((item) => <section className="z-section" key={item.condition.id}><button className="z-related-title" onClick={() => onSelect(item.condition.id, { id: item.condition.id, label: item.condition.label, kind: item.condition.kind, reason: item.why.map((sentence) => sentence.text).join(" "), evidence: sourcesOf(item.sources), score: 0 })}>{item.condition.label}<Icon name="arrow" size={16} /></button>{item.why.map((reason, index) => <p key={index}>{reason.text}</p>)}<p className="z-shared-terms">{[...item.shared.genes, ...item.shared.processes, ...item.shared.symptoms].map((term) => term.label).join(" · ")}</p>{item.limits?.map((limit, index) => <p className="z-note" key={index}>{limit.text}</p>)}{item.caveat && <p className="z-note">{item.caveat}</p>}{item.cards?.length ? renderConnections(item.cards.slice(0, 3)) : null}</section>) : <p className="z-empty-note">{copy.noData}</p>}
        {!!detail.related?.counterexamples.length && <section className="z-section"><h3>{copy.conflicting}</h3>{detail.related.counterexamples.map((item) => <div className="z-counterexample" key={item.condition.id}><strong>{item.condition.label}</strong><p>{item.looks_like.text}</p><p>{item.differs.text}</p></div>)}</section>}
        <p className="z-fine-print">{copy.relatedCaveat}</p>
      </>}
      {section === "questions" && (detail.questions.length ? detail.questions.map((question) => <section className="z-section z-question" key={question.id}><p className="z-eyebrow">{question.status}</p><h3>{question.hypothesis.text}</h3>{question.why_it_matters && <p>{question.why_it_matters.text}</p>}{[{ title: copy.supports, sentences: question.for }, { title: copy.against, sentences: question.against }, { title: copy.missing, sentences: question.unknown }].map((group) => group.sentences.length ? <details key={group.title}><summary>{group.title}</summary>{group.sentences.map((sentence, index) => <p key={index}>{sentence.text}</p>)}</details> : null)}<h4>{copy.experiment}</h4><p>{question.experiment.text}</p>{question.experiment_note && <p className="z-note">{question.experiment_note.text}</p>}<button className="z-text-button" onClick={onRequest}>{copy.request}<Icon name="arrow" size={14} /></button></section>) : <p className="z-empty-note">{copy.noData}</p>)}
      {section === "gaps" && <>
        {!hasConnections ? <p>{copy.noCoverage}</p> : <>{!exactGroups.length && <p>{copy.noneExactGroup}</p>}{!studies.some((item) => item.match === "exact") && <p>{copy.noneExactStudy}</p>}</>}
        <section className="z-section"><h3>{copy.missing}</h3>{detail.gaps?.unknown.length ? detail.gaps.unknown.map((sentence, index) => <p key={index}>{sentence.text}</p>) : <p className="z-muted">{copy.noData}</p>}</section>
        <section className="z-section"><h3>{copy.nextQuestions}</h3>{detail.gaps?.questions.map((sentence, index) => <p key={index}>{sentence.text}</p>)}</section>
        <section className="z-section"><h3>{copy.startPath}</h3><ol className="z-gap-steps">{copy.gapSteps.map((step, index) => <li key={step}><button className="z-text-button" onClick={() => chooseSection(index === 0 ? "related" : index === 1 ? "studies" : "people")}>{step}<Icon name="arrow" size={14} /></button></li>)}</ol></section>
        <details className="z-coverage"><summary>{copy.coverage}</summary>{[...(detail.connections?.coverage ?? []), ...(detail.gaps?.searched ?? [])].map((coverage, index) => <div className="z-coverage-row" key={`${coverage.source}-${index}`}><strong>{coverage.source}</strong><span>{coverage.searched_on}</span><span>{coverage.note || `${coverage.found}`}</span></div>)}</details>
      </>}
      </>}
    </>}
    {!detail && isCondition && <p className="z-empty-note">{copy.noCoverage}</p>}
    <p className="z-fine-print">{copy.medical}</p>
  </>;
}
