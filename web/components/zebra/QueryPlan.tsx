"use client";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import dynamic from "next/dynamic";
import type { ExploreResponse } from "@/lib/zebra/types";
import { useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import { ZebraLoader } from "./ZebraLoader";
import { canMutateQuery, inspectQuery, queryFitsBudget, queryParser, queryReceipts, querySuggestions, type QueryLink, type QueryModel, type QueryRunSettings, type QuerySuggestion, type QuerySuggestionPage } from "./query-workspace";
import { getQueryWorkspaceCopy } from "./queryWorkspaceCopy";
import styles from "./QueryPlan.module.css";
const QueryGraphView = dynamic(() => import("./QueryGraphView"), { ssr: false });
const ReasoningProof = dynamic(() => import("./ReasoningProof"));
const blank = { active: false, property: null as QuerySuggestion | null, filter: "", offset: 0, page: null as QuerySuggestionPage | null };

/** The host keeps ownership of the sourced results; this page edits one actual execution. */
export default function QueryPlan({ data, query, busy, onRunSparql, onClose }: { data: ExploreResponse | null; query: string; busy: boolean; onRunSparql?: (sparql: string, settings: QueryRunSettings) => void; onClose?(): void }) {
  const locale = useZebraLocale(), text = getQueryWorkspaceCopy(locale);
  const receipts = useMemo(() => queryReceipts(data), [data]);
  const [queryIndex, setQueryIndex] = useState(0), [edits, setEdits] = useState<Record<string, { text: string; applied: string }>>({});
  const receipt = receipts[Math.min(queryIndex, Math.max(0, receipts.length - 1))];
  const draft = receipt ? edits[receipt.key]?.text ?? receipt.text : "", visualText = receipt ? edits[receipt.key]?.applied ?? receipt.text : "";
  const [loadedModel, setLoadedModel] = useState<{ scope: string; model: QueryModel } | null>(null), [selected, setSelected] = useState(""), [edge, setEdge] = useState<string | null>(null), [explore, setExplore] = useState(blank), [error, setError] = useState(""), [loading, setLoading] = useState(false), [code, setCode] = useState(false), [undo, setUndo] = useState<string | null>(null);
  const revision = useRef(0);
  const parseRevision = useRef(0);
  const undoCap = useRef<number | null>(null);
  const [capEdits, setCapEdits] = useState<Record<string, string>>({}), [appliedCaps, setAppliedCaps] = useState<Record<string, number>>({}), [capBusy, setCapBusy] = useState(false);
  const [reasoningEdits, setReasoningEdits] = useState<Record<string, boolean>>({});
  const receiptKey = receipt?.key;
  const [addedLinks, setAddedLinks] = useState<Record<string, QueryLink[]>>({});
  const links = useMemo(() => receipt ? [...receipt.linked, ...(addedLinks[receipt.key] || []).filter(link => !receipt.linked.some(known => known.id === link.id))] : undefined, [receipt, addedLinks]);
  const modelScope = `${receiptKey || ""}|${visualText}`;
  const model = loadedModel?.scope === modelScope ? loadedModel.model : null;
  const [failedScope, setFailedScope] = useState<string | null>(null);
  const parseFailed = failedScope === modelScope;
  const canMutate = canMutateQuery(model, draft, visualText);
  const [suggestionSeed, setSuggestionSeed] = useState("");
  const neighborhood = model?.neighborhood;
  useEffect(() => {
    const revisions = parseRevision;
    const current = ++parseRevision.current;
    if (!links) return;
    void inspectQuery(visualText, links).then(next => {
      if (current !== parseRevision.current) return;
      setLoadedModel({ scope: modelScope, model: next }); setFailedScope(null); setError(""); setExplore(blank); setEdge(null);
      setSelected(old => next.graph.nodes.some(node => node.key === old) ? old : next.graph.nodes[0]?.key || "");
    }).catch(() => { if (current === parseRevision.current) { setLoadedModel(null); setFailedScope(modelScope); setExplore(blank); setError(""); } });
    return () => { revisions.current++; };
  }, [visualText, links, receiptKey, modelScope, text.unsupported]);
  const active = model?.graph.nodes.find(node => node.key === selected);
  const discoverySeed = neighborhood?.seed_ids.includes(suggestionSeed) ? suggestionSeed : neighborhood?.seed_ids[0];
  const discoveryByNode = !!neighborhood || active?.ids.length === 1;
  const scope = neighborhood ? discoverySeed : discoveryByNode ? active?.ids[0] : active?.kind;
  useEffect(() => {
    if (!explore.active || !scope || !canMutate) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      setLoading(true);
      void querySuggestions({ ...(discoveryByNode ? { node: scope } : { class: scope }), relation: explore.property?.relation, direction: explore.property?.direction, target_class: explore.property?.target_class, q: explore.filter, offset: explore.offset }, controller.signal).then(page => {
        if (!controller.signal.aborted) { setExplore(current => ({ ...current, page })); setError(""); }
      }).catch(() => { if (!controller.signal.aborted) setError(text.unavailable); }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    }, 150);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [explore.active, explore.filter, explore.offset, explore.property, scope, discoveryByNode, canMutate, text.unavailable]);
  const phantoms = useMemo(() => explore.page?.items.map(item => ({ key: `${item.direction}:${item.relation}:${item.target_class}:${item.node?.id || ""}`, label: item.node?.label || item.relation.replaceAll("_", " "), detail: `${item.count} ${item.target_class.replaceAll("_", " ")}`, accessibleName: `${item.node?.label || item.relation.replaceAll("_", " ")}, ${item.direction === "incoming" ? text.incoming : text.outgoing}, ${item.count} ${text.count}` })) || [], [explore.page, text]);
  function edit(value: string, applied = false) { revision.current++; setLoading(false); if (applied) { setExplore(blank); setSelected(""); setEdge(null); } else setExplore(blank); if (receipt) setEdits(current => ({ ...current, [receipt.key]: { text: value, applied: applied ? value : current[receipt.key]?.applied ?? receipt.text } })); }
  function select(key: string) { revision.current++; setSelected(key); setEdge(null); setExplore({ ...blank, active: canMutate }); setError(""); }
  async function choose(index: number) {
    const suggestion = explore.page?.items[index]; if (!suggestion || !model || !receipt || !canMutate) return;
    if (neighborhood) { if (!neighborhood.relations.includes(suggestion.relation)) await editRelationships([...neighborhood.relations, suggestion.relation]); return; }
    if (!explore.property) { setExplore({ ...blank, active: true, property: suggestion }); return; }
    const current = ++revision.current;
    try {
      const next = (await queryParser()).addConnection(model, selected, suggestion, links);
      if (current !== revision.current) return;
      if (next.graph.nodes.length > 12 || next.graph.edges.length > 16) throw new Error("visual_limit");
      if (suggestion.node) { const node = suggestion.node; setAddedLinks(values => ({ ...values, [receipt.key]: [...(values[receipt.key] || []).filter(link => link.id !== node.id), node] })); }
      undoCap.current = requestedCap; setUndo(visualText); edit(next.text, true); setExplore(blank); setError("");
    } catch { if (current === revision.current) setError(text.editFailed); }
  }
  async function remove() {
    if (!model || !receipt || !canMutate || neighborhood) return;
    const current = ++revision.current;
    try { const next = (await queryParser()).removeElement(model, edge || selected, !!edge, links); if (current !== revision.current) return; undoCap.current = requestedCap; setUndo(visualText); edit(next.text, true); setExplore(blank); setError(""); }
    catch { if (current === revision.current) setError(text.editFailed); }
  }
  async function editRelationships(relations: string[]) {
    if (!receipt || !neighborhood || !canMutate) return;
    const current = ++revision.current;
    try { const next = (await queryParser()).setNeighborhoodRelations(visualText, relations); if (current !== revision.current) return; undoCap.current = requestedCap; setUndo(visualText); edit(next, true); setError(""); }
    catch { if (current === revision.current) setError(text.editFailed); }
  }
  async function branch(index: number) {
    if (!receipt) return;
    const current = ++revision.current;
    try { const next = await inspectQuery(visualText, receipt.linked, index); if (current !== revision.current) return; setLoadedModel({ scope: modelScope, model: next }); setSelected(next.graph.nodes[0]?.key || ""); setEdge(null); setExplore(blank); }
    catch { if (current === revision.current) setError(text.unsupported); }
  }
  function page(direction: -1 | 1) { const offset = explore.offset + direction * 5; if (offset >= 0 && offset < (explore.page?.total || 0)) setExplore(current => ({ ...current, offset, page: null })); }
  const changed = !!receipt && draft !== receipt.text;
  const requestedCap = receipt ? Number(capEdits[receipt.key] ?? receipt.settings?.limit) : NaN;
  const validCap = Number.isInteger(requestedCap) && requestedCap > 0 && requestedCap <= 160;
  const capPending = !!receipt && capEdits[receipt.key] !== undefined && appliedCaps[receipt.key] !== requestedCap;
  const reasoning = receipt ? reasoningEdits[receipt.key] ?? receipt.settings?.reasoning : undefined;
  const runSettings = receipt?.settings && validCap ? { ...receipt.settings, linked: links || receipt.linked, limit: requestedCap, reasoning: reasoning ?? receipt.settings.reasoning } : undefined;
  const editedCapBlocked = changed && requestedCap > 100 && !(neighborhood && draft === visualText);
  async function applyCap() {
    if (!receipt || !validCap) return;
    const current = ++revision.current;
    setCapBusy(true);
    try { const next = (await queryParser()).setQueryLimit(draft, requestedCap); if (current !== revision.current) return; undoCap.current = appliedCaps[receipt.key] ?? receipt.settings?.limit ?? null; setUndo(visualText); edit(next, true); setAppliedCaps(values => ({ ...values, [receipt.key]: requestedCap })); setError(""); }
    catch { if (current === revision.current) setError(text.editFailed); }
    finally { setCapBusy(false); }
  }
  const title = (index: number) => {
    const item = receipts[index];
    const stage = item.stage === "neighborhood" ? text.direct : item.stage === "frontier" ? text.further : item.stage === "shared_bridges" ? text.shared : item.stage;
    const names = item.settings?.focus.slice(0, 2).map(id => item.linked.find(link => link.id === id)?.label || id).join(", ");
    return `${index + 1}. ${names ? `${names} · ` : ""}${stage || item.backend}`;
  };
  return <section className={styles.workspace} aria-label={text.title}>
    <header className={styles.header}>{onClose && <button type="button" className={styles.back} onClick={onClose}><Icon name="back" size={17} />{text.back}</button>}<h1>{text.title}</h1>
      {receipts.length > 1 && <select aria-label={text.query} value={Math.min(queryIndex, receipts.length - 1)} onChange={event => { revision.current++; setQueryIndex(Number(event.target.value)); setUndo(null); setExplore(blank); }}>{receipts.map((item, index) => <option key={item.key} value={index}>{title(index)}</option>)}</select>}
      {onRunSparql && receipt && <button type="button" className={styles.run} disabled={busy || capBusy || capPending || !runSettings || !queryFitsBudget(draft) || editedCapBlocked} onClick={() => runSettings && onRunSparql(draft, runSettings)}>{busy && <ZebraLoader />}{busy ? text.working : text.run}</button>}
    </header>
    {!receipt ? <p className={styles.notice}>{text.empty}</p> : <>
      <div className={styles.toolbar}><span>{changed ? text.edited : title(Math.min(queryIndex, receipts.length - 1))}</span><div>
        <button type="button" disabled={!canMutate || !scope} onClick={() => { setEdge(null); setExplore({ ...blank, active: true }); }}>{neighborhood ? text.addRelationship : text.explore}</button>
        {!neighborhood && <button type="button" disabled={!canMutate || (!selected && !edge)} onClick={() => void remove()}>{text.remove}</button>}
        {undo && <button type="button" onClick={() => { edit(undo, true); if (receipt && undoCap.current !== null) { const cap = undoCap.current; setCapEdits(values => ({ ...values, [receipt.key]: String(cap) })); setAppliedCaps(values => ({ ...values, [receipt.key]: cap })); } setUndo(null); }}>{text.undo}</button>}
        <button type="button" aria-expanded={code} onClick={() => setCode(value => !value)}>{text.code}</button>
      </div></div>
      {receipt.settings && <div className={styles.capControl}><label>{text.cap}<input type="number" min={1} max={160} value={capEdits[receipt.key] ?? receipt.settings.limit} onChange={event => { revision.current++; setCapEdits(values => ({ ...values, [receipt.key]: event.target.value })); }} /></label>{capPending && <button type="button" disabled={!validCap || capBusy} onClick={() => void applyCap()}>{capBusy && <ZebraLoader />}{text.apply}</button>}<label>{text.infer}<select aria-label={text.infer} value={String(reasoning)} disabled={busy} onChange={event => setReasoningEdits(values => ({ ...values, [receipt.key]: event.target.value === "true" }))}><option value="false">{text.no}</option><option value="true">{text.yes}</option></select></label></div>}
      {editedCapBlocked && <p className={styles.notice}>{text.capGuard}</p>}
      {draft !== visualText && <p className={styles.notice}>{text.pendingEdits}</p>}
      {parseFailed && <p className={styles.notice} role="status">{text.unsupported}</p>}
      <div className={styles.body} hidden={parseFailed}>
        <div className={styles.stage}>
          {model?.editable === false && <div className={styles.projection}><strong>{neighborhood ? text.neighborhood : text.limited}</strong><span>{neighborhood ? text.bothDirections : text.projection}</span></div>}
          <QueryGraphView graph={model?.graph || null} pending={!parseFailed} selected={model ? selected : ""} phantoms={canMutate ? phantoms : []} onSelect={select} onEdge={key => { setEdge(key); setExplore(blank); }} onPhantom={index => void choose(index)} onEscape={() => setExplore(blank)} onPage={page} />
          {error && <p role="status" className={styles.notice}>{error}</p>}
        </div>
        <aside className={styles.inspector} aria-label={text.constraints}>
          {neighborhood && <section className={styles.operator}><h2>{text.neighborhood}</h2><dl><dt>{text.seeds}</dt><dd>{neighborhood.seed_ids.map(id => <span key={id} title={id}>{links?.find(link => link.id === id)?.label || id}</span>)}</dd><dt>{text.direction}</dt><dd>{text.bothDirections}</dd><dt>{text.queryLimit}</dt><dd>{neighborhood.limit}</dd></dl><h3>{text.relationships}</h3><ul>{neighborhood.relations.map(relation => <li key={relation}><span title={`https://w3id.org/rare-disease-atlas/vocab#${relation}`}>{relation.replaceAll("_", " ")}</span><button type="button" disabled={!canMutate || neighborhood.relations.length < 2} aria-label={`${text.removeRelationship}: ${relation.replaceAll("_", " ")}`} onClick={() => void editRelationships(neighborhood.relations.filter(item => item !== relation))}><Icon name="close" size={12} /></button></li>)}</ul><p className={styles.hint}>{text.fixedSeeds}</p></section>}
          {!neighborhood && model?.inspection.branches.length && model.inspection.branches.length > 1 ? <label className={styles.field}>{text.scope}<select value={model.branch || 0} onChange={event => void branch(Number(event.target.value))}>{model.inspection.branches.map((item, index) => <option key={item.key} value={index}>{index + 1}. {item.label} ({item.triples.length})</option>)}</select></label> : null}
          {explore.active && <section className={styles.suggestions}><h2>{explore.property ? text.targets : text.properties}</h2>{explore.property && <button type="button" onClick={() => setExplore({ ...blank, active: true })}>{explore.property.relation.replaceAll("_", " ")}</button>}
            {neighborhood && neighborhood.seed_ids.length > 1 && <label className={styles.field}>{text.suggestionSeed}<select value={discoverySeed} onChange={event => { setSuggestionSeed(event.target.value); setExplore({ ...blank, active: true }); }}>{neighborhood.seed_ids.map(id => <option key={id} value={id}>{links?.find(link => link.id === id)?.label || id}</option>)}</select></label>}
            <label className={styles.field}>{text.filter}<input value={explore.filter} onChange={event => setExplore(current => ({ ...current, filter: event.target.value, offset: 0, page: null }))} maxLength={128} /></label>
            {loading ? <p role="status"><ZebraLoader />{text.loading}</p> : explore.page && !explore.page.items.length ? <p>{text.noSuggestions}</p> : null}
            <ol>{explore.page?.items.map((item, index) => <li key={`${item.relation}:${item.direction}:${item.node?.id || item.target_class}`}><button type="button" disabled={!canMutate || neighborhood?.relations.includes(item.relation)} onClick={() => void choose(index)}>{item.node?.label || item.relation.replaceAll("_", " ")}<small>{item.direction === "incoming" ? text.incoming : text.outgoing} · {item.count} {item.target_class.replaceAll("_", " ")}</small></button></li>)}</ol>
            {explore.page && explore.page.total > 5 && <div className={styles.pages}><button type="button" disabled={explore.offset === 0} onClick={() => page(-1)}>{text.previous}</button><span>{explore.offset + 1}–{explore.offset + explore.page.items.length}/{explore.page.total}</span><button type="button" disabled={explore.offset + 5 >= explore.page.total} onClick={() => page(1)}>{text.next}</button></div>}
            {explore.page && <details><summary>{text.source}</summary><p>{text.index}: {explore.page.index_sha256}</p>{explore.page.items.map((item, index) => <pre key={index}>{JSON.stringify({ relation: item.relation, direction: item.direction, node: item.node, witness_edge: item.witness_edge, witness_status: item.witness_status, evidence_sample: item.evidence_sample }, null, 2)}</pre>)}</details>}
          </section>}
          {!neighborhood && <h2>{text.constraints}</h2>}{model?.editable === false && !neighborhood && <p className={styles.hint}>{text.readOnly}</p>}
          {neighborhood ? <details><summary>{text.constraints}</summary>{model?.inspection.constraints.map(item => <section key={item.key} className={styles.constraint}><h3>{item.label}</h3><pre>{item.text}</pre></section>)}</details> : model?.inspection.constraints.map(item => <section key={item.key} className={styles.constraint}><h3>{item.label}</h3><pre>{item.text}</pre></section>)}
          {model?.graph.edges.length ? <details><summary>{text.graph}</summary><ol>{model.graph.edges.map(item => <li key={item.key}><button type="button" onClick={() => { setEdge(item.key); setExplore(blank); }}>{model.graph.nodes.find(node => node.key === item.from)?.label} → {item.relation} → {model.graph.nodes.find(node => node.key === item.to)?.label}</button></li>)}</ol></details> : null}
          {model?.graph.nodes.length ? <details><summary>{text.selection}</summary>{model.graph.nodes.map(node => <button type="button" key={node.key} aria-pressed={selected === node.key && !edge} onClick={() => select(node.key)}>{node.label}</button>)}</details> : null}
        </aside>
      </div>
      {(code || parseFailed) && <section className={styles.code}><label className={styles.field}>{text.code}<textarea value={draft} onChange={event => edit(event.target.value)} spellCheck={false} autoCapitalize="off" autoCorrect="off" rows={12} /></label><div className={styles.pages}><button type="button" disabled={draft === visualText} onClick={() => { undoCap.current = requestedCap; setUndo(visualText); edit(draft, true); }}>{text.apply}</button>{changed && <button type="button" onClick={() => { edit(receipt.text, true); if (receipt.settings) { const cap = receipt.settings.limit; setCapEdits(values => ({ ...values, [receipt.key]: String(cap) })); setAppliedCaps(values => ({ ...values, [receipt.key]: cap })); } setUndo(null); }}>{text.discard}</button>}</div></section>}
      {!queryFitsBudget(draft) && <p className={styles.notice}>{text.overBudget}</p>}{!receipt.settings && <p className={styles.notice}>{text.unknownSettings}</p>}
      <details className={styles.technical}><summary>{text.technical}</summary><dl><dt>{text.store}</dt><dd>{receipt.backend}</dd>{receipt.settings && <><dt>{text.infer}</dt><dd>{receipt.settings.reasoning ? text.yes : text.no}</dd><dt>{text.cap}</dt><dd>{receipt.settings.limit}</dd></>}</dl>
        {data?.graph.community && <details><summary>{text.coverage}</summary><dl>{data.graph.community.seeds.map(seed => <Fragment key={seed.id}><dt>{data.graph.nodes.find(node => node.id === seed.id)?.label || seed.id}</dt><dd>{seed.direct_edges} {text.direct}{seed.truncated ? ` · ${text.limitedCount}` : ""}</dd></Fragment>)}</dl></details>}
        {query.trim() && <details><summary>{text.prepared}</summary><p>{query}</p></details>}
        {data?.query_execution?.answer.plan && <details><summary>{text.raw}</summary><pre>{JSON.stringify(data.query_execution.answer.plan, null, 2)}</pre></details>}
        {data?.execution.reasoning_proofs?.map((proof, index) => <ReasoningProof key={index} proof={proof} labels={new Map(data.graph.nodes.map(node => [node.id, node.label]))} />)}
        {data?.execution.reasoning_warning && <p>{data.execution.reasoning_warning}</p>}{data?.interpretation.warning && <p>{data.interpretation.warning}</p>}
      </details>
    </>}
  </section>;
}
