"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";
import { useEffect, useId, useRef, useState } from "react";
import { updateQueryCanvas, queryPackage, type QueryCanvas, type QueryDiagram, type QueryPhantom } from "./query-workspace";
import { useZebraLocale } from "./Locale";
import { queryWorkspaceCopy } from "./queryWorkspaceCopy";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./QueryGraphView.module.css";

export type QueryGraphViewProps = { graph: QueryDiagram | null; pending?: boolean; selected: string; phantoms: QueryPhantom[]; onSelect(key: string): void; onPhantom(index: number): void; onEdge(key: string): void; onEscape(): void; onPage(direction: -1 | 1): void };
const authorCredit = "Query-by-Graph · Daniel Motz";
/** One mounted editor; query revisions update its graph without resetting the canvas. */
export default function QueryGraphView(props: QueryGraphViewProps) {
  const text = queryWorkspaceCopy[zebraCopyLocale(useZebraLocale())];
  const host = useRef<HTMLDivElement>(null), api = useRef<QueryCanvas | null>(null), callbacks = useRef(props), queue = useRef(Promise.resolve()), revision = useRef(0);
  const id = useId();
  const [ready, setReady] = useState(false), [failed, setFailed] = useState(false);
  useEffect(() => { callbacks.current = props; }, [props]);
  useEffect(() => {
    const revisions = revision;
    let cancelled = false;
    const parent = host.current;
    if (!parent) return;
    const container = document.createElement("div"); parent.replaceChildren(container);
    void queryPackage().then(module => module.mount(container)).then(editor => {
      if (cancelled) { editor.destroy(); return; }
      api.current = editor; setReady(true);
    }).catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; revisions.current++; api.current?.destroy(); api.current = null; container.remove(); };
  }, []);
  useEffect(() => {
    const editor = api.current, next = ++revision.current;
    if (!editor) return;
    queue.current = queue.current.catch(() => {}).then(async () => {
      if (next !== revision.current || api.current !== editor) return;
      await updateQueryCanvas(editor, props.graph, props.selected, props.phantoms, text.optional, id, {
        onSelect: key => callbacks.current.onSelect(key), onPhantom: index => callbacks.current.onPhantom(index), onEdge: key => callbacks.current.onEdge(key),
      });
      if (next === revision.current) setFailed(false);
    }).catch(() => { if (next === revision.current) setFailed(true); });
  }, [props.graph, props.selected, props.phantoms, ready, id, text.optional]);
  return <section className={styles.view} aria-label={text.graph} onKeyDown={event => {
    if (event.key === "Escape" && props.phantoms.length) { event.preventDefault(); event.stopPropagation(); props.onEscape(); }
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-phantom-index]");
    if (!button) return;
    const delta = ["ArrowRight", "ArrowDown"].includes(event.key) ? 1 : ["ArrowLeft", "ArrowUp"].includes(event.key) ? -1 : 0;
    if (!delta) return;
    event.preventDefault(); const next = Number(button.dataset.phantomIndex) + delta;
    if (next < 0 || next >= props.phantoms.length) props.onPage(delta as -1 | 1);
    else host.current?.querySelector<HTMLButtonElement>(`[data-phantom-index="${next}"]`)?.focus();
  }}>
    <div ref={host} className={styles.canvas} />
    {failed && <p className={styles.status} role="status">{text.unsupported}</p>}
    {!failed && (!ready || !props.graph) && <p className={styles.status} role="status">{(!ready || props.pending) && <ZebraLoader />}{!ready || props.pending ? text.loading : text.unsupported}</p>}
    <div className={styles.controls}><button type="button" onClick={() => api.current?.zoom(0.8)} aria-label={`${text.graph} −`}>−</button><button type="button" onClick={() => void api.current?.fit()}>{text.fit}</button><button type="button" onClick={() => api.current?.zoom(1.25)} aria-label={`${text.graph} +`}>+</button></div>
    <a className={styles.credit} href="https://www.daniel-motz.de" target="_blank" rel="noreferrer">{authorCredit}</a>
  </section>;
}
