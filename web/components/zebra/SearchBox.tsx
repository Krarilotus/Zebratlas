"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";

import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";


import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent, type PointerEvent } from "react";
import { extractDocument, lookupEntities } from "@/lib/zebra/client";
import type { ExtractedDocument } from "@/lib/zebra/types";
import { createSuggestionScheduler, type SearchSuggestion } from "@/lib/zebra/search-suggestions";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./SearchSuggestions.module.css";

const resizeWords = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).resizeWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["resizeWords"]>;
const suggestionWords = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).suggestionWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["suggestionWords"]>;
type SearchMetrics = { paddingX: number; paddingY: number; icon: number; gap: number; control: number; controls: number; singleHeight: number; expandedHeight: number; lineHeight: number; textPadding: number };

export function SearchBox({ initial = "", resetVersion = 0, compact = false, busy = false, onSearch }: { initial?: string; resetVersion?: number; compact?: boolean; busy?: boolean; onSearch: (query: string, display?: string) => void }) {
  const copy = useZebraCopy();
  const locale = useZebraLocale();
  const resize = resizeWords[zebraCopyLocale(locale)];
  const suggestionCopy = suggestionWords[zebraCopyLocale(locale)];
  const resizeHintId = useId();
  const [query, setQuery] = useState(initial);
  const [document, setDocument] = useState<ExtractedDocument | null>(null);
  const [extracting, setExtracting] = useState(false);
  const [error, setError] = useState("");
  const [suggestions, setSuggestions] = useState<SearchSuggestion[]>([]);
  const root = useRef<HTMLDivElement>(null);
  const focused = useRef(false);
  const composing = useRef(false);
  const suggestionScheduler = useRef<ReturnType<typeof createSuggestionScheduler> | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const measure = useRef<HTMLTextAreaElement>(null);
  const form = useRef<HTMLFormElement>(null);
  const resizeHandle = useRef<HTMLButtonElement>(null);
  const queryValue = useRef(initial);
  const source = useRef({ initial, resetVersion });
  const metrics = useRef<SearchMetrics | null>(null);
  const manual = useRef<{ width?: number; height?: number; exact: boolean }>({ exact: false });
  const drag = useRef<{ id: number; x: number; y: number; width: number; height: number } | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const extraction = useRef<AbortController | null>(null);
  useEffect(() => () => extraction.current?.abort(), []);
  useEffect(() => {
    const scheduler = createSuggestionScheduler({ lookup: lookupEntities, publish: setSuggestions });
    suggestionScheduler.current = scheduler;
    function outside(event: globalThis.PointerEvent) {
      if (!root.current?.contains(event.target as Node)) { focused.current = false; scheduler.cancel(); }
    }
    window.document.addEventListener("pointerdown", outside);
    return () => { window.document.removeEventListener("pointerdown", outside); scheduler.dispose(); suggestionScheduler.current = null; };
  }, []);
  useEffect(() => { if (busy || extracting) suggestionScheduler.current?.cancel(); }, [busy, extracting]);
  const limits = useCallback(() => ({
    width: Math.max(0, form.current?.parentElement?.clientWidth ?? 0),
    height: Math.max(120, Math.min(compact ? 340 : 460, window.innerHeight * .55)),
  }), [compact]);
  const getMetrics = useCallback(() => {
    if (metrics.current) return metrics.current;
    if (!form.current || !input.current) return null;
    const box = getComputedStyle(form.current);
    const text = getComputedStyle(input.current);
    const lineHeight = parseFloat(text.lineHeight);
    const textPadding = parseFloat(text.paddingTop) + parseFloat(text.paddingBottom);
    const paddingY = parseFloat(box.paddingTop) + parseFloat(box.paddingBottom) + parseFloat(box.borderTopWidth) + parseFloat(box.borderBottomWidth);
    const control = parseFloat(box.getPropertyValue("--z-search-control"));
    const rowControls = parseFloat(box.getPropertyValue("--z-search-row-controls"));
    metrics.current = { paddingX: parseFloat(box.paddingLeft) + parseFloat(box.paddingRight) + parseFloat(box.borderLeftWidth) + parseFloat(box.borderRightWidth), paddingY,
      icon: parseFloat(box.getPropertyValue("--z-search-icon")), gap: parseFloat(box.columnGap), control, controls: rowControls,
      singleHeight: Math.max(lineHeight + textPadding, control) + paddingY,
      expandedHeight: control * 2 + 8 + paddingY, lineHeight, textPadding };
    return metrics.current;
  }, []);
  const setHeight = useCallback((height: number, expanded: boolean) => {
    const element = form.current;
    if (!element) return;
    const width = Math.round(element.getBoundingClientRect().width);
    root.current?.style.setProperty("--z-search-box-width", `${width}px`);
    element.style.setProperty("--z-search-height", `${Math.ceil(height)}px`);
    element.dataset.multiline = String(expanded);
    resizeHandle.current?.setAttribute("aria-label", `${resize.label}: ${width} × ${Math.round(height)}`);
  }, [resize.label]);
  const fit = useCallback((contentChanged = false) => {
    if (drag.current || !form.current || !measure.current) return;
    const sizes = getMetrics(); if (!sizes) return;
    const maximum = limits();
    if (manual.current.width !== undefined) {
      manual.current.width = Math.min(manual.current.width, maximum.width);
      form.current.style.width = `${manual.current.width}px`;
    }
    if (contentChanged && !queryValue.current) { manual.current.height = undefined; manual.current.exact = false; }
    const width = form.current.getBoundingClientRect().width;
    const mirror = measure.current; mirror.value = queryValue.current;
    mirror.style.width = `${Math.max(40, width - sizes.paddingX - sizes.icon - sizes.gap * 2 - sizes.controls)}px`;
    const wraps = mirror.scrollHeight > sizes.lineHeight + sizes.textPadding + 1;
    if (manual.current.exact && !contentChanged && manual.current.height !== undefined) {
      const expanded = wraps || manual.current.height >= sizes.expandedHeight - 1;
      const height = Math.min(maximum.height, Math.max(expanded ? sizes.expandedHeight : sizes.singleHeight, manual.current.height));
      setHeight(height, expanded); return;
    }
    manual.current.exact = false;
    const expanded = wraps || (manual.current.height ?? 0) >= sizes.expandedHeight;
    if (expanded) mirror.style.width = `${Math.max(40, width - sizes.paddingX - sizes.icon - sizes.gap * 2 - sizes.control)}px`;
    const natural = mirror.scrollHeight + sizes.paddingY;
    const height = Math.min(maximum.height, Math.max(expanded ? sizes.expandedHeight : sizes.singleHeight, natural, manual.current.height ?? 0));
    setHeight(height, expanded);
  }, [getMetrics, limits, setHeight]);
  useLayoutEffect(() => { queryValue.current = query; fit(true); }, [query, fit]);
  useEffect(() => {
    const previous = source.current;
    source.current = { initial, resetVersion };
    if (initial === previous.initial && resetVersion === previous.resetVersion) return;
    const explicitReset = resetVersion !== previous.resetVersion;
    // Late saved/deep-link results preserve dirty drafts. Browser history is an explicit reset.
    void Promise.resolve().then(() => {
      if (source.current.initial !== initial || source.current.resetVersion !== resetVersion) return;
      if (explicitReset || queryValue.current === previous.initial) {
        suggestionScheduler.current?.cancel();
        setQuery(initial);
        if (explicitReset) { extraction.current?.abort(); extraction.current = null; setExtracting(false); setDocument(null); setError(""); }
      }
    });
  }, [initial, resetVersion]);
  useLayoutEffect(() => {
    let previousWidth = 0;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? 0;
      if (Math.abs(width - previousWidth) < 1) return;
      previousWidth = width; fit();
    });
    if (input.current) observer.observe(input.current);
    function viewport() { metrics.current = null; fit(); }
    window.addEventListener("resize", viewport);
    return () => { observer.disconnect(); window.removeEventListener("resize", viewport); };
  }, [fit]);
  function manualSize(width: number, height: number) {
    const sizes = getMetrics(); if (!form.current || !sizes) return;
    const maximum = limits();
    const nextWidth = Math.min(maximum.width, Math.max(Math.min(260, maximum.width), width));
    let wraps = false;
    if (measure.current) {
      measure.current.value = queryValue.current;
      measure.current.style.width = `${Math.max(40, nextWidth - sizes.paddingX - sizes.icon - sizes.gap * 2 - sizes.controls)}px`;
      wraps = measure.current.scrollHeight > sizes.lineHeight + sizes.textPadding + 1;
    }
    const expanded = wraps || height >= sizes.expandedHeight - 1;
    manual.current = { width: nextWidth, height: Math.min(maximum.height, Math.max(expanded ? sizes.expandedHeight : sizes.singleHeight, height)), exact: true };
    form.current.style.width = `${manual.current.width}px`;
    setHeight(manual.current.height ?? sizes.singleHeight, expanded);
  }
  function startResize(event: PointerEvent<HTMLButtonElement>) {
    if (!form.current || event.button !== 0) return;
    event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId);
    const rect = form.current.getBoundingClientRect(); getMetrics();
    drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, width: rect.width, height: rect.height };
  }
  function moveResize(event: PointerEvent<HTMLButtonElement>) {
    const start = drag.current; if (!start || start.id !== event.pointerId) return;
    manualSize(start.width + event.clientX - start.x, start.height + event.clientY - start.y);
  }
  function endResize(event: PointerEvent<HTMLButtonElement>) {
    if (drag.current?.id !== event.pointerId) return;
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }
  function keyboardResize(event: KeyboardEvent<HTMLButtonElement>) {
    const rect = form.current?.getBoundingClientRect(); const sizes = getMetrics(); if (!rect || !sizes) return;
    if (event.key === "Home") { event.preventDefault(); manual.current = { exact: false }; if (form.current) form.current.style.width = ""; fit(); return; }
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault(); const step = event.shiftKey ? 3 : 1;
    manualSize(rect.width + (event.key === "ArrowLeft" ? -20 * step : event.key === "ArrowRight" ? 20 * step : 0), rect.height + (event.key === "ArrowUp" ? -sizes.lineHeight * step : event.key === "ArrowDown" ? sizes.lineHeight * step : 0));
  }
  function submit(event?: FormEvent) {
    event?.preventDefault();
    suggestionScheduler.current?.cancel();
    if (busy || extracting || (!query.trim() && !document)) return;
    const prefix = document ? `${query.trim()}\n\nDocument: ${document.name}\n` : query.trim();
    const encoder = new TextEncoder();
    const remaining = 32000 - encoder.encode(prefix).length;
    if (remaining < 0) { setError(copy.queryTooLong); return; }
    let documentText = document?.text ?? "";
    if (encoder.encode(documentText).length > remaining) {
      let low = 0; let high = documentText.length;
      while (low < high) { const middle = Math.ceil((low + high) / 2); if (encoder.encode(documentText.slice(0, middle)).length <= remaining) low = middle; else high = middle - 1; }
      documentText = documentText.slice(0, low).replace(/[\uD800-\uDBFF]$/, "");
      if (document) setDocument({ ...document, text: documentText, truncated: true });
    }
    const text = document ? `${prefix}${documentText}` : prefix;
    onSearch(text, query.trim() || document?.name);
  }
  async function attach(selected?: File) {
    if (!selected) return;
    suggestionScheduler.current?.cancel();
    extraction.current?.abort();
    const controller = new AbortController(); extraction.current = controller;
    setExtracting(true); setError("");
    try { const value = await extractDocument(selected, controller.signal); if (!controller.signal.aborted) setDocument(value); }
    catch (e) { if (!controller.signal.aborted) setError(e instanceof Error ? e.message : copy.documentUnsupported); }
    finally { if (!controller.signal.aborted && extraction.current === controller) { extraction.current = null; setExtracting(false); if (file.current) file.current.value = ""; } }
  }
  return <div ref={root} className={`z-search ${styles.root} ${compact ? `z-search-compact ${styles.compact}` : ""}`} onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) { focused.current = false; suggestionScheduler.current?.cancel(); } }}>
    <form ref={form} onSubmit={submit} className="z-search-form" role="search" aria-busy={busy || extracting}>
      <Icon name="search" size={compact ? 19 : 22} />
      <textarea ref={input} className="z-search-input" value={query} rows={1} maxLength={16000} aria-label={copy.search} placeholder={copy.placeholder}
        onChange={(e) => { queryValue.current = e.target.value; setQuery(e.target.value); suggestionScheduler.current?.edit(e.target.value, focused.current && !composing.current && !busy && !extracting); }}
        onFocus={() => { focused.current = true; suggestionScheduler.current?.edit(queryValue.current, !composing.current && !busy && !extracting); }}
        onBlur={(event) => { if (!(event.relatedTarget as HTMLElement | null)?.dataset.zebraSuggestion) { focused.current = false; suggestionScheduler.current?.cancel(); } }}
        onCompositionStart={() => { composing.current = true; suggestionScheduler.current?.cancel(); }}
        onCompositionEnd={(event) => { composing.current = false; suggestionScheduler.current?.edit(event.currentTarget.value, focused.current && !busy && !extracting); }}
        onKeyDown={(e) => { if (e.key === "Escape") suggestionScheduler.current?.cancel(); if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && !composing.current) { e.preventDefault(); submit(); } }} />
      <input ref={file} type="file" accept=".txt,.md,.pdf,.docx,.csv,.json,.xml" hidden onChange={(e) => void attach(e.target.files?.[0])} />
      <div className="z-search-controls"><button type="button" className="z-icon-button z-attach" aria-label={copy.attach} title={copy.attach} disabled={extracting || busy} onClick={() => file.current?.click()}><Icon name="attach" size={20} /></button>
        <button type="submit" className="z-search-submit" aria-label={busy ? copy.loading : extracting ? copy.extracting : copy.enter} aria-busy={busy || extracting} disabled={busy || extracting || (!query.trim() && !document)}>{busy || extracting ? <ZebraLoader /> : <Icon name="arrow" size={21} />}</button></div>
      <textarea ref={measure} className="z-search-input z-search-measure" rows={1} readOnly tabIndex={-1} aria-hidden="true" />
      <button ref={resizeHandle} type="button" className="z-search-resize" aria-label={resize.label} aria-describedby={resizeHintId} title={resize.label} onPointerDown={startResize} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onLostPointerCapture={() => { drag.current = null; }} onKeyDown={keyboardResize}><span aria-hidden="true" /></button>
      <span id={resizeHintId} className="z-search-resize-hint">{resize.hint}</span>
    </form>
    {!!suggestions.length && <div className={styles.suggestions} role="group" aria-label={suggestionCopy.label}>{suggestions.map((choice) => <button key={choice.key} type="button" data-zebra-suggestion="true" className={`${styles.chip} ${choice.operator ? styles.operator : ""}`} title={choice.operator ? suggestionCopy[choice.operator] : undefined} onPointerDown={(event) => { if (event.button === 0) event.preventDefault(); }} onClick={() => { queryValue.current = choice.query; setQuery(choice.query); focused.current = window.document.activeElement === input.current; suggestionScheduler.current?.edit(choice.query, focused.current && !composing.current && !busy && !extracting); }}>{choice.label}{choice.kind && <span className={styles.kind}>{copy.kinds[choice.kind as keyof typeof copy.kinds] ?? choice.kind}</span>}</button>)}</div>}
    {document && <div className="z-document-review"><div className="z-document"><Icon name="attach" size={14} /><span>{document.name}</span><button type="button" className="z-icon-button" aria-label={copy.removeDocument} onClick={() => setDocument(null)}><Icon name="close" size={14} /></button></div><details open><summary>{copy.reviewDocument}</summary><p>{copy.documentPrivacy}</p><textarea value={document.text} rows={5} aria-label={copy.reviewDocument} onChange={(e) => setDocument({ ...document, text: e.target.value, chars: e.target.value.length })} />{document.truncated && <p>{copy.documentTruncated}</p>}</details></div>}
    {extracting && <p className="z-search-note" role="status">{copy.extracting}</p>}
    {error && <p className="z-error" role="alert">{error}</p>}
    {!compact && <>
      <p className="z-search-caption">{copy.caption}</p>
      <div className="z-examples"><span>{copy.try}</span>{copy.examples.map((gene) => <button key={gene} type="button" onClick={() => { setQuery(gene); onSearch(gene); }}>{gene}<Icon name="arrow" size={12} /></button>)}</div>
    </>}
  </div>;
}
