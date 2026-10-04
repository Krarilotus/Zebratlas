"use client";

import { ZebraLoader } from "./ZebraLoader";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import type { Connection, MessageResponse, Sentence, SourceRecord } from "@/lib/types";
import type { ConditionDetail, ConditionSection, ExploreResult, GraphEvidence } from "@/lib/zebra/types";
import { account, draftMessage, saveItem, ZebraApiError } from "@/lib/zebra/client";
import { LOCALES, LOCALE_META } from "@/lib/i18n/config";
import { zebraHref } from "@/lib/zebra/locale";
import { useZebraCatalog, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import Sources, { safeSourceUrl } from "./Sources";
import type { BriefTaskId } from "./briefTasks";
import styles from "./ResearchPanels.module.css";
import { briefEmailWords, emailComposeHref, scriptedInquiry, wrapInquiry, type EmailProvider } from "./brief-email";
import { sourceDetailCopy } from "./sourceDetailCopy";
import { graphSourceRecord } from "./source-provenance";
import { findingKind } from "./result-overview";

export type ResearchBriefProps = { query: string; selected: ExploreResult | Connection | null; detail: ConditionDetail | null; onClose?: () => void; onLoadSections?: (sections: ConditionSection[]) => Promise<void> };
const taskSections: Record<BriefTaskId, ConditionSection[]> = {
  F01: ["jobs"], F02: ["jobs"], F03: ["connections"], F04: ["connections"], F05: ["jobs"], F06: [],
  F07: ["related"], F08: ["jobs"], F09: ["jobs"], F10: ["related"], F11: ["related", "questions"], F12: ["gaps", "connections"],
};
const languages = LOCALES.map(id => ({ id, label: LOCALE_META[id].name }));
const isConnection = (selected: ResearchBriefProps["selected"]): selected is Connection => !!selected && "name" in selected;
const labelOf = (selected: ResearchBriefProps["selected"]) => selected ? isConnection(selected) ? selected.name : selected.label : "";
const initialTask = (selected: ResearchBriefProps["selected"]): BriefTaskId => {
  const kind = selected ? isConnection(selected) ? selected.kind : findingKind(selected) : undefined;
  if (["asset", "model", "biobank", "cell_line", "dataset"].includes(kind || "")) return "F01";
  if (["therapy", "programme", "drug", "designation"].includes(kind || "")) return "F02";
  if (kind === "outcome_measure") return "F05";
  if (kind === "patient_group") return "F03";
  if (["study", "registry", "natural_history", "trial", "observational", "expanded_access"].includes(kind || "")) return "F04";
  if (["person", "researcher"].includes(kind || "")) return "F07";
  if (["grant", "funder", "funding_call"].includes(kind || "")) return "F08";
  if (kind === "paper") return "F09";
  return "F11";
};

function collectSources(selected: ResearchBriefProps["selected"], detail: ConditionDetail | null): SourceRecord[] {
  const records = [
    ...(isConnection(selected) ? selected.sources : []), ...(detail?.summary?.sources || []),
    ...(detail?.related?.communities.flatMap(item => item.assets.filter(asset => asset.id === selected?.id).flatMap(asset => asset.sources)) || []),
  ];
  return records.filter((source, index) => records.findIndex(other => other.id === source.id && other.url === source.url) === index);
}
function collectFacts(selected: ResearchBriefProps["selected"], detail: ConditionDetail | null) {
  const sentences: Sentence[] = [
    ...(isConnection(selected) ? [selected.why] : []), ...(detail?.summary?.what || []),
  ];
  return sentences.filter(sentence => sentence.cites.length > 0);
}
function graphSources(selected: ResearchBriefProps["selected"]): GraphEvidence[] { return selected && !isConnection(selected) ? [...selected.evidence, ...(selected.metadata_evidence || [])] : []; }
function officialRoute(url: string, kind: string): string | undefined {
  const web = safeSourceUrl(url);
  if (web) return web;
  if (kind === "email" && /^mailto:[^\s?<>]+@[^\s?<>]+$/i.test(url)) return url;
  return undefined;
}

/** A new scope gets fresh editor state; in-place typing is never replaced by an effect. */
export default function ResearchBrief(props: ResearchBriefProps) {
  const connections = [...(props.detail?.connections?.exact || []), ...(props.detail?.connections?.related || []),
    ...(props.detail?.related?.communities.flatMap(item => [...(item.group ? [item.group] : []), ...(item.partner ? [item.partner] : []), ...(item.cards || [])]) || []),
    ...(props.detail?.questions.flatMap(question => question.partner ? [question.partner] : []) || [])];
  const selected = connections.find(connection => connection.id === props.selected?.id) || props.selected;
  return <BriefEditor key={`${props.query}|${selected?.id || ""}`} {...props} selected={selected} />;
}

function BriefEditor({ query, selected, detail, onClose, onLoadSections }: ResearchBriefProps) {
  const { copy, briefTasks, briefWords: words, sourceWords } = useZebraCatalog();
  const locale = useZebraLocale();
  const emailWords = briefEmailWords[locale];
  const sourceDetails = sourceDetailCopy[locale];
  const [profile, setProfile] = useState<{ display_name: string | null; email: string } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void account(controller.signal).then(value => { if (!controller.signal.aborted && value.state === "signed_in") setProfile(value.account.user); }).catch(() => {});
    return () => controller.abort();
  }, []);
  const coreSources = useMemo(() => collectSources(selected, detail), [selected, detail]);
  const facts = useMemo(() => collectFacts(selected, detail), [selected, detail]);
  const jobItem = detail?.jobs?.jobs.flatMap(job => job.items).find(item => item.id === selected?.id);
  const bridge = detail?.related?.people.find(person => person.person.id === selected?.id);
  const evidence = [...graphSources(selected), ...(jobItem?.source ? [{ source: jobItem.source.name, url: jobItem.source.url, record: jobItem.source.record, retrieved_at: jobItem.source.retrieved_at, sha256: jobItem.source.sha256 }] : []),
    ...(bridge?.works.map(work => ({ source: work.title, record: work.id, url: work.url })) || [])].map(graphSourceRecord);
  const graphSelection = selected && !isConnection(selected) ? selected : undefined;
  const access = graphSelection?.official_action || (graphSelection?.how_to_get?.url ? { action: graphSelection.how_to_get.route, url: graphSelection.how_to_get.url } : undefined);
  const contactRoutes = (isConnection(selected) ? selected.channels.map(channel => ({ label: channel.kind.replaceAll("_", " "), url: officialRoute(channel.url, channel.kind) })) : [
    ...(access ? [{ label: access.action.replaceAll("_", " "), url: officialRoute(access.url, access.url.startsWith("mailto:") ? "email" : access.action) }] : []),
    ...(jobItem?.how_to_get.url ? [{ label: jobItem.how_to_get.route.replaceAll("_", " "), url: officialRoute(jobItem.how_to_get.url, jobItem.how_to_get.url.startsWith("mailto:") ? "email" : jobItem.how_to_get.route) }] : []),
  ]).filter((route, index, routes) => !!route.url && routes.findIndex(other => other.url === route.url) === index);
  const conditionLabel = detail?.summary?.condition.label || detail?.connections?.condition.label || query;
  const scopeLabel = labelOf(selected) || conditionLabel;
  const [taskId, setTaskId] = useState<BriefTaskId>(() => initialTask(selected));
  const task = briefTasks.find(item => item.id === taskId)!;
  const requestedTasks = useRef(new Set<string>());
  const [sectionStatus, setSectionStatus] = useState<Record<string, "loading" | "ready" | "failed">>({});
  const sectionKey = `${detail?.id || ""}|${locale}|${taskId}`;
  const loadSections = useCallback(async (retry = false) => {
    if (!detail || !onLoadSections) return;
    const loaded = detail.loadedSections || [];
    const needed = taskSections[taskId].filter(section => !loaded.includes(section));
    if (!needed.length || (!retry && requestedTasks.current.has(sectionKey))) return;
    requestedTasks.current.add(sectionKey);
    setSectionStatus(current => ({ ...current, [sectionKey]: "loading" }));
    try {
      await onLoadSections(needed);
      setSectionStatus(current => ({ ...current, [sectionKey]: "ready" }));
    } catch {
      setSectionStatus(current => ({ ...current, [sectionKey]: "failed" }));
    }
  }, [detail, onLoadSections, sectionKey, taskId]);
  // The Request panel and purpose selector are explicit requests for these sections.
  useEffect(() => { void Promise.resolve().then(() => loadSections()); }, [loadSections]);
  const relatedScope = (id: BriefTaskId) => {
    if (id !== "F10" && id !== "F11") return { community: undefined, question: undefined, counter: undefined };
    const rootCondition = !!selected && !!detail && selected.id === detail.id;
    const community = detail?.related?.communities.find(item => item.condition.id === selected?.id || item.group?.id === selected?.id || item.partner?.id === selected?.id || item.cards?.some(card => card.id === selected?.id))
      || (rootCondition ? detail?.related?.communities[0] : undefined);
    const question = detail?.questions.find(item => item.id === selected?.id || item.partner?.id === selected?.id)
      || (rootCondition ? detail?.questions.find(item => item.communities.some(community => community.node.id === selected?.id)) : undefined);
    const counter = rootCondition || community ? detail?.related?.counterexamples[0] : undefined;
    return { community, question, counter };
  };
  const scopeSources = (id: BriefTaskId) => {
    const scope = relatedScope(id);
    const gapSources = id === "F12" ? detail?.connections?.exact.filter(connection => ["patient_group", "trial", "registry", "natural_history", "observational", "expanded_access"].includes(connection.kind)).flatMap(connection => connection.sources) || [] : [];
    return [...coreSources, ...(scope.community?.sources || []), ...(scope.question?.sources || []), ...(scope.counter?.sources || []), ...gapSources]
      .filter((source, index, list) => list.findIndex(other => other.id === source.id && other.url === source.url) === index);
  };
  const sources = scopeSources(taskId);
  const recordedContext = (id: BriefTaskId) => {
    const scopedSources = scopeSources(id);
    const cited = (sentence: Sentence) => `${sentence.text}${sentence.cites.length ? ` [${sentence.cites.join(", ")}]` : ""}`;
    const supported = (sentence: Sentence) => sentence.cites.length > 0 && sentence.cites.every(cite => scopedSources.some(source => source.id === cite));
    const lines = [`${scopeLabel}${selected ? ` (${selected.id})` : ""}`];
    const selectedFacts = isConnection(selected) ? [selected.why] : facts;
    lines.push(...selectedFacts.filter(supported).slice(0, 2).map(cited));
    if (isConnection(selected) && selected.status) lines.push(`${words.status}: ${copy.status[selected.status]}`);
    if (jobItem) {
      const cite = jobItem.source?.record ? ` [${jobItem.source.record}]` : "";
      if (jobItem.holder) lines.push(`${words.holder}: ${jobItem.holder.name}${cite}`);
      lines.push(...jobItem.facts.slice(0, 3).map(fact => `${fact.key.replaceAll("_", " ")}: ${fact.value}${cite}`));
    } else if (graphSelection) {
      const recorded = graphSelection.metadata_evidence || [];
      const cite = recorded[0]?.record ? ` [${recorded[0].record}]` : "";
      if (graphSelection.holder) lines.push(`${words.holder}: ${graphSelection.holder.name}${cite}`);
      if (graphSelection.status) lines.push(`${words.status}: ${copy.status[graphSelection.status as keyof typeof copy.status] || graphSelection.status.replaceAll("_", " ")}${cite}`);
      lines.push(...(graphSelection.facts || []).slice(0, 3).map(fact => `${fact.key.replaceAll("_", " ")}: ${fact.value}${cite}`));
    }
    if (id === "F07" && bridge) {
      lines.push(`${copy.related}: ${bridge.communities.map(item => item.label).join(" · ")}`);
      lines.push(...bridge.works.slice(0, 2).map(work => `${work.title}${work.year ? ` (${work.year})` : ""} [${work.id}]`));
    }
    if (id === "F10" || id === "F11") {
      const scope = relatedScope(id);
      if (scope.community) lines.push(...scope.community.why.filter(supported).slice(0, 1).map(cited));
      if (scope.counter && supported(scope.counter.differs)) lines.push(`${copy.limitation}: ${cited(scope.counter.differs)}`);
      if (id === "F11" && scope.question) {
        lines.push(`${words.hypothesis}: ${cited(scope.question.hypothesis)}`);
        lines.push(...scope.question.for.filter(supported).slice(0, 2).map(cited));
        if (supported(scope.question.experiment)) lines.push(`${copy.experiment}: ${cited(scope.question.experiment)}`);
      }
    }
    if (id === "F12") {
      const exactGroups = detail?.connections?.exact.filter(connection => connection.kind === "patient_group") || [];
      const exactStudies = detail?.connections?.exact.filter(connection => ["trial", "registry", "natural_history", "observational", "expanded_access"].includes(connection.kind)) || [];
      for (const section of [{ label: copy.groups, items: exactGroups, missing: copy.noneExactGroup }, { label: copy.studies, items: exactStudies, missing: copy.noneExactStudy }]) {
        lines.push(section.items.length ? `${section.label}: ${section.items.slice(0, 2).map(connection => `${connection.name} (${connection.id})`).join("; ")}` : detail?.connections ? section.missing : copy.noCoverage);
      }
    }
    return lines.join("\n");
  };
  // A pristine template follows newly loaded evidence. Once edited, its wording belongs to the user.
  const [editedSubject, setSubject] = useState<string | null>(null);
  const [editedBody, setBody] = useState<string | null>(null);
  const subject = editedSubject ?? `${task.title}: ${scopeLabel}`;
  const researcher = selected && ["person", "researcher"].includes(selected.kind) ? scopeLabel : undefined;
  const inquiryAsk = taskId === "F11" && !relatedScope(taskId).question
    ? emailWords.proposeQuestion
    : taskId === "F07" && !(bridge?.communities.length && bridge.communities.length > 1)
      ? emailWords.researchStep
      : task.ask.replace(locale === "en" ? "Would the researcher review" : "Könnte die forschende Person prüfen", locale === "en" ? "Would you review" : "Könnten Sie prüfen").replace(locale === "en" ? "Can the partner review" : "Kann die Partnerstelle", locale === "en" ? "Could you review" : "Könnten Sie");
  const body = editedBody ?? scriptedInquiry({ query, topic: detail?.summary?.condition.label || detail?.connections?.condition.label, record: labelOf(selected), task: taskId, ask: inquiryAsk, researcher, sender: profile?.display_name, locale: locale });
  const [provider, setProvider] = useState<EmailProvider>("mailto");
  const [editedRecipient, setRecipient] = useState<string | null>(null);
  const [toSelf, setToSelf] = useState(false);
  const officialEmail = contactRoutes.find(route => route.url?.startsWith("mailto:"))?.url?.slice(7) || "";
  const recipient = toSelf ? profile?.email || "" : editedRecipient ?? officialEmail;
  const [lang, setLang] = useState<string>(locale);
  const [sender, setSender] = useState<"family" | "group">(() => ["F03", "F04"].includes(initialTask(selected)) ? "family" : "group");
  const [draft, setDraft] = useState<MessageResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const [signin, setSignin] = useState(false);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => () => pending.current?.abort(), []);
  const allSources = [...sources, ...(draft?.sources || [])].filter((source, index, list) => list.findIndex(other => other.id === source.id && other.url === source.url) === index);
  const appendix = () => [words.sourceAppendix,
    `${emailWords.facts}\n${recordedContext(taskId)}`,
    ...(contactRoutes.length ? [words.contacts, ...contactRoutes.map(route => `- ${route.label}: ${route.url}`)] : []),
    ...allSources.map(source => `- [${source.id}] ${source.source}\n  ${words.url}: ${safeSourceUrl(source.url) || words.unknown}\n  ${words.retrieved}: ${source.retrieved_on || words.unknown}\n  ${words.locator}: ${source.locator || words.unknown}\n  ${words.tier}: ${source.tier}${source.record ? `\n  ${words.records}: ${source.record}` : ""}${source.assertion ? `\n  ${sourceDetails.assertion}: ${source.assertion}` : ""}${source.kind ? `\n  ${sourceWords.edgeKind}: ${source.kind}` : ""}${source.version ? `\n  ${words.version}: ${source.version}` : ""}${source.sha256 ? `\n  ${words.checksum}: ${source.sha256}` : ""}${source.sha256_scope ? `\n  ${sourceDetails.hashScope}: ${source.sha256_scope}` : ""}${source.generated_by ? `\n  ${sourceDetails.generated}: ${source.generated_by}` : ""}${source.references?.length ? `\n  ${sourceDetails.references}: ${source.references.join(" · ")}` : ""}`),
    ...evidence.map((source, index) => `- [${source.record || `evidence-${index + 1}`}] ${source.source}\n  ${words.url}: ${safeSourceUrl(source.url) || words.unknown}\n  ${words.retrieved}: ${source.retrieved_at || words.unknown}${source.version ? `\n  ${words.version}: ${source.version}` : ""}${source.sha256 ? `\n  ${words.checksum}: ${source.sha256}` : ""}${source.sha256_scope ? `\n  ${sourceDetails.hashScope}: ${source.sha256_scope}` : ""}${source.generated_by ? `\n  ${sourceDetails.generated}: ${source.generated_by}` : ""}${source.references?.length ? `\n  ${sourceDetails.references}: ${source.references.join(" · ")}` : ""}`),
    ...(allSources.length || evidence.length ? [] : [words.noSources]),
    ...(isConnection(selected) ? [...(selected.limits || []).map(limit => `${sourceDetails.limits}: ${limit}`), ...(selected.conflicts || []).map(conflict => `${sourceDetails.conflicts}: ${conflict}`), ...selected.edges.flatMap(edge => [
      `${sourceDetails.assertion}: ${edge.id || `${edge.from}|${edge.relation}|${edge.to}`}${edge.kind ? ` (${edge.kind})` : ""}${edge.why ? ` · ${edge.why}` : ""}`,
      ...[...edge.evidence.map(record => ({ record, label: sourceDetails.support })), ...(edge.contradicted_by || []).map(record => ({ record, label: sourceWords.counter }))].map(({ record, label }) => `${label}: ${record.source}${record.record ? ` [${record.record}]` : ""}${record.quote ? ` · ${record.quote}` : ""}${record.date ? ` · ${record.date}` : ""}${record.url ? `\n  ${words.url}: ${safeSourceUrl(record.url) || words.unknown}` : ""}${record.references.length ? `\n  ${sourceDetails.references}: ${record.references.join(" · ")}` : ""}${record.sha256 ? `\n  ${words.checksum}: ${record.sha256}` : ""}${record.sha256_scope ? `\n  ${sourceDetails.hashScope}: ${record.sha256_scope}` : ""}${record.version ? `\n  ${words.version}: ${record.version}` : ""}${record.generated_by ? `\n  ${sourceDetails.generated}: ${record.generated_by}` : ""}${record.evidence_code ? `\n  ${sourceDetails.code}: ${record.evidence_code}` : ""}${record.frequency ? `\n  ${sourceDetails.frequency}: ${record.frequency}` : ""}${typeof record.confidence === "number" ? `\n  ${sourceDetails.confidence}: ${record.confidence}` : ""}`),
    ])] : []),
  ].join("\n");
  const exportText = () => `${subject}\n\n${words.scope}: ${query}\n${words.selected}: ${scopeLabel}${selected ? ` (${selected.id})` : ""}\n${words.purpose}: ${task.title}\n${words.created}: ${new Date().toISOString()}\n\n${body}\n\n${appendix()}`;
  function changeTask(id: BriefTaskId) {
    pending.current?.abort(); setBusy(false); setTaskId(id); setDraft(null); setStatus("");
    setSubject(null); setBody(null);
  }
  async function prepare() {
    if (!isConnection(selected) || !detail) return;
    pending.current?.abort(); const controller = new AbortController(); pending.current = controller;
    setBusy(true); setStatus("");
    try {
      const result = await draftMessage({ condition: detail.id, connection: selected.id, lang, sender, kind: taskId === "F11" ? "proposal" : "message" }, controller.signal);
      if (controller.signal.aborted) return;
      setDraft(result); setSubject(result.subject); setBody(wrapInquiry(result.body, { researcher, sender: profile?.display_name, locale: locale }));
      setStatus(result.validator.ok ? words.validationPassed : words.validationFailed);
    } catch { if (!controller.signal.aborted) setStatus(words.failed); }
    finally { if (!controller.signal.aborted) setBusy(false); }
  }
  async function copyBrief() { try { await navigator.clipboard.writeText(body); setStatus(words.copied); } catch { setStatus(words.copyFailed); } }
  function openEmail() {
    const compose = emailComposeHref(provider, recipient, subject, body);
    if (!compose.href) { setStatus(compose.error === "recipient" ? emailWords.invalid : emailWords.tooLong); return; }
    if (provider === "mailto") window.location.assign(compose.href);
    else window.open(compose.href, "_blank", "noopener,noreferrer");
  }
  function download() {
    const url = URL.createObjectURL(new Blob([exportText()], { type: "text/markdown;charset=utf-8" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = `zebratlas-${task.id.toLowerCase()}-brief.md`; document.body.appendChild(anchor); anchor.click(); anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000); setStatus(words.downloaded);
  }
  async function save() {
    setSaving(true); setSignin(false);
    try {
      const scope = detail?.id || selected?.id || (/^[\w:.-]{1,80}$/.test(query) ? query : "");
      const href = scope ? `/zebra?q=${encodeURIComponent(scope)}${selected ? `&node=${encodeURIComponent(selected.id)}` : ""}` : "/zebra";
      const input = { kind: "message_draft" as const, title: subject, refs: [selected?.id, detail?.id].filter((id): id is string => !!id), payload: { href: zebraHref(href, locale), subject, body: exportText(), query, task: taskId } };
      if (new TextEncoder().encode(JSON.stringify(input)).length > 65536) { setStatus(emailWords.saveTooLarge); return; }
      await saveItem(input);
      setStatus(words.saved);
    } catch (error) {
      if (error instanceof ZebraApiError && error.status === 401) { setSignin(true); setStatus(words.signInNeeded); }
      else setStatus(words.saveFailed);
    } finally { setSaving(false); }
  }
  return <section className={styles.panel} aria-label={words.title}>
    <header className={styles.header}><h3>{words.title}</h3>{onClose && <button type="button" className={styles.iconButton} aria-label={copy.close} onClick={onClose}><Icon name="close" /></button>}</header>
    <label className={styles.field}><span>{words.purpose}</span><select value={taskId} onChange={event => changeTask(event.target.value as BriefTaskId)}>{briefTasks.map(item => <option key={item.id} value={item.id}>{item.title}</option>)}</select></label>
    {sectionStatus[sectionKey] === "loading" && <p className={styles.status} role="status"><ZebraLoader /> {copy.loading}</p>}
    {sectionStatus[sectionKey] !== "loading" && (sectionStatus[sectionKey] === "failed" || taskSections[taskId].some(section => detail?.unavailable.includes(section))) && <div className={styles.status} role="status"><p>{copy.noCoverage}</p><button type="button" onClick={() => void loadSections(true)}>{copy.retry}</button></div>}
    <p className={styles.status}>{draft ? words.generated : words.template}</p>
    <label className={styles.field}><span>{words.subject}</span><input value={subject} onChange={event => setSubject(event.target.value)} /></label>
    <label className={styles.field}><span>{words.body}</span><textarea value={body} onChange={event => setBody(event.target.value)} spellCheck aria-describedby="zebra-brief-edit-note" /></label>
    <p id="zebra-brief-edit-note" className={styles.status}>{words.editable}</p>
    <div className={styles.row}>
      <label className={styles.field}><span>{emailWords.provider}</span><select value={provider} onChange={event => setProvider(event.target.value as EmailProvider)}><option value="mailto">{emailWords.default}</option><option value="gmail">{emailWords.gmail}</option><option value="outlook">{emailWords.outlook}</option></select></label>
      <label className={styles.field}><span>{emailWords.recipient}</span><input type="email" value={recipient} disabled={toSelf} onChange={event => setRecipient(event.target.value)} /></label>
    </div>
    {profile?.email && <label className={styles.self}><input type="checkbox" checked={toSelf} onChange={event => setToSelf(event.target.checked)} /><span>{emailWords.self}</span></label>}
    <div className={styles.actions}>
      <button type="button" className={styles.primary} onClick={openEmail}>{emailWords.open}</button>
      <button type="button" onClick={download}>{words.download}</button>
      <button type="button" onClick={copyBrief}>{words.copy}</button>
      <button type="button" onClick={save} disabled={saving}>{saving ? <><ZebraLoader /> {words.saving}</> : words.save}</button>
      <button type="button" onClick={() => window.print()}>{words.print}</button>
    </div>
    {status && <p className={styles.status} role="status">{status}</p>}
    {signin && <Link href={zebraHref("/zebra/account", locale)}>{words.signin}</Link>}
    <details className={styles.section}><summary>{words.prepare}</summary>
      {isConnection(selected) && detail ? <>
        <div className={styles.row}>
          <label className={styles.field}><span>{words.sender}</span><select value={sender} onChange={event => setSender(event.target.value as "family" | "group")}><option value="family">{words.family}</option><option value="group">{words.group}</option></select></label>
          <label className={styles.field}><span>{words.language}</span><select value={lang} onChange={event => setLang(event.target.value)}>{languages.map(language => <option key={language.id} value={language.id}>{language.label}</option>)}</select></label>
        </div>
        <button type="button" onClick={prepare} disabled={busy}>{busy ? <><ZebraLoader /> {words.drafting}</> : words.prepare}</button>
        {lang !== locale && !draft && <p className={styles.status}>{words.unsupportedLocale}</p>}
      </> : <p className={styles.muted}>{words.noRecipient}</p>}
    </details>
    <details className={styles.section}><summary>{copy.nextQuestions}</summary><ul className={styles.facts}>{task.questions.map(question => <li key={question}>{question}</li>)}</ul></details>
    <details className={styles.section}><summary>{words.sourceFacts}</summary>
      {draft?.validator.notes.length ? <ul className={styles.facts}>{draft.validator.notes.map((note, index) => <li key={index}>{note}</li>)}</ul> : null}
      <Sources sources={allSources} evidence={evidence} edges={isConnection(selected) ? selected.edges : []} edgeIds={isConnection(selected) ? selected.edge_ids : []} entityId={selected?.id} llm={draft?.llm ? [draft.llm] : isConnection(selected) ? selected.llm : []} validator={draft?.validator || (isConnection(selected) ? selected.validator : undefined)} coverage={detail?.connections?.coverage} limits={isConnection(selected) ? selected.limits : undefined} conflicts={isConnection(selected) ? selected.conflicts : undefined} />
    </details>
    <div className={styles.printArea}><h1>{subject}</h1><pre>{`${words.scope}: ${query}\n${words.selected}: ${scopeLabel}\n${words.purpose}: ${task.title}\n\n${body}\n\n${appendix()}`}</pre></div>
  </section>;
}
