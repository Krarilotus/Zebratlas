"use client";

import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ATLAS_VOCAB, compactTBoxId, expressionText, fullTBoxDiagram, loadTBox, tboxEdgeRoutes, TBOX_PIN, type TBoxData } from "@/lib/zebra/tbox";
import { rankGraphLabels, visibleGraphLabels } from "./graph-labels";
import { fitTBoxBounds, zoomTBoxView, type TBoxView } from "./tbox-viewport";
import { useZebraCopy } from "./Locale";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./TBoxDiagram.module.css";

const COLORS: Record<string, string> = { subclass: "#527b99", bfo: "#816595", dul: "#bc8152", domain: "#17675f", range: "#946b86", property: "#17675f", equivalent: "#467f8c", expression: "#788b96", union: "#788b96", inverse: "#a37950", disjoint: "#996278" };
const shortName = (label: string) => label.length > 35 ? `${label.slice(0, 32)}…` : label;
export default function TBoxDiagram() {
  const copy = useZebraCopy(), words = copy.tbox;
  const [data, setData] = useState<TBoxData | null>(null), [failed, setFailed] = useState(false);
  const [selected, setSelected] = useState(""), [filter, setFilter] = useState(""), [active, setActive] = useState("");
  const [activeEdge, setActiveEdge] = useState("");
  const [expanded, setExpanded] = useState(false), [width, setWidth] = useState(640);
  const [camera, setCamera] = useState<{ key: string; view: TBoxView } | null>(null);
  const host = useRef<HTMLDivElement>(null), svg = useRef<SVGSVGElement>(null), markerId = useId().replaceAll(":", ""), listId = useId();
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const gesture = useRef<{ view: TBoxView; center: { x: number; y: number }; distance: number } | null>(null);
  const pointerNode = useRef(""), focusNode = useRef("");
  const pointerEdge = useRef(""), focusEdge = useRef("");
  useEffect(() => {
    const controller = new AbortController();
    void loadTBox(controller.signal).then(value => { if (!controller.signal.aborted) setData(value); }).catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, []);
  useEffect(() => {
    const element = host.current; if (!element) return;
    const measure = () => setWidth(Math.max(280, Math.round(element.clientWidth)));
    const observer = new ResizeObserver(measure); observer.observe(element); measure();
    return () => observer.disconnect();
  }, []);
  const height = expanded ? 680 : width < 600 ? 440 : 460;
  const scene = useMemo(() => data ? fullTBoxDiagram(data, width, selected) : null, [data, selected, width]);
  const routes = useMemo(() => scene ? tboxEdgeRoutes({ ...scene, nodes: scene.nodes.map(node => ({ ...node, width: 6, height: 6 })) }) : [], [scene]);
  const key = `${selected}:${width}:${height}`;
  const fitted = scene ? fitTBoxBounds(scene.focusBounds, { width, height }) : { x: 0, y: 0, scale: 1 };
  const initial = scene && fitted.scale < .85 ? zoomTBoxView(fitted, .85 / fitted.scale, { x: width / 2, y: height / 2 }) : fitted;
  const view = camera?.key === key ? camera.view : initial;
  const latest = useRef({ key, view });
  useLayoutEffect(() => { latest.current = { key, view }; }, [key, view]);
  const matched = useMemo(() => new Set(scene?.nodes.filter(node => filter.trim() && `${node.label} ${node.id}`.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase())).map(node => node.id)), [scene, filter]);
  const caption = (kind: string) => kind === "subclass" ? words.subclass : kind === "domain" ? words.domain : kind === "range" ? words.range : kind === "equivalent" ? words.equivalent : kind === "expression" ? words.definition : kind === "property" ? words.property : kind === "inverse" ? "owl:inverseOf" : kind === "disjoint" ? "owl:disjointWith" : words.union;
  const color = (kind: string, module?: string) => kind === "subclass" && (module === "bfo" || module === "dul") ? COLORS[module] : COLORS[kind] ?? COLORS.expression;
  const edgeCaption = (edge: (typeof routes)[number]["edge"]) => edge.kind === "property" ? edge.label ?? compactTBoxId(edge.propertyId ?? "") : edge.kind === "expression" && edge.role ? edge.role : caption(edge.kind);
  const named = useMemo(() => {
    if (!scene) return [];
    const candidates = scene.nodes.filter(node => node.prominent || matched.has(node.id) || node.id === active).map(node => ({ id: node.id, x: node.x, y: node.y - 17, width: shortName(node.label).length * 7.5 + 12, height: 20, focus: node.id === active, seed: node.id === selected || matched.has(node.id), distance: node.id === selected ? 0 : 1 }));
    const prominent = new Set(scene.nodes.filter(node => node.prominent).map(node => node.id));
    for (const route of routes) if (route.edge.id === activeEdge || route.edge.source === selected || route.edge.target === selected || (route.edge.kind === "property" && prominent.has(route.edge.source) && prominent.has(route.edge.target))) candidates.push({ id: `edge:${route.edge.id}`, x: route.label.x, y: route.label.y - 4, width: shortName(edgeCaption(route.edge)).length * 7 + 10, height: 20, focus: route.edge.id === activeEdge, seed: false, distance: 2 });
    return rankGraphLabels(candidates);
  // Caption depends only on the current language, not camera movement.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, routes, matched, active, activeEdge, selected, words]);
  const visible = visibleGraphLabels(named, view, { width, height }, { activeId: active || (activeEdge ? `edge:${activeEdge}` : ""), maxLabels: width < 600 ? 16 : 32, obstacles: [{ left: width - 156, top: 0, right: width, bottom: 58, width: 156, height: 58 }] });
  const selectedNode = scene?.nodes.find(node => node.id === selected), activeNode = scene?.nodes.find(node => node.id === active), activeConnection = scene?.edges.find(edge => edge.id === activeEdge);
  const selectedClass = data?.classes.find(item => item.id === selected), selectedProperty = data?.properties.find(item => item.id === selected);
  const equations = data?.equivalences.filter(item => item.source === selected) ?? [];
  const complexParents = data?.subclasses.filter(item => item.source === selected && item.target.type !== "named") ?? [];
  const highlight = (id: string, owner: "pointer" | "focus") => { (owner === "pointer" ? pointerNode : focusNode).current = id; setActive(pointerNode.current || focusNode.current); };
  const highlightEdge = (id: string, owner: "pointer" | "focus") => { (owner === "pointer" ? pointerEdge : focusEdge).current = id; setActiveEdge(pointerEdge.current || focusEdge.current); };
  const choose = (id: string) => { if (id !== selected) { setSelected(id); setFilter(""); } };
  const local = (event: { clientX: number; clientY: number }) => { const box = svg.current!.getBoundingClientRect(); return { x: event.clientX - box.left, y: event.clientY - box.top }; };
  const startGesture = () => { const points = [...pointers.current.values()]; if (!points.length) { gesture.current = null; return; } const a = points[0], b = points[1] ?? a; gesture.current = { view: latest.current.view, center: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, distance: points.length > 1 ? Math.hypot(a.x - b.x, a.y - b.y) : 0 }; };
  useEffect(() => {
    const element = svg.current; if (!element) return;
    const wheel = (event: WheelEvent) => { event.preventDefault(); const state = latest.current; setCamera({ key: state.key, view: zoomTBoxView(state.view, Math.exp(-Math.max(-80, Math.min(80, event.deltaY)) * .006), local(event)) }); };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [data]);
  const basePath = `/zebra/tbox/${TBOX_PIN.version}`;
  return <section ref={host} className={styles.tbox} aria-label={words.title}>
    <header><h3>{words.title}</h3><span>{TBOX_PIN.version}</span></header><p className={styles.scope}>{words.scope}</p>
    {!data && <div className={styles.pending} role="status">{!failed && <ZebraLoader />}{failed ? words.unavailable : words.loading}</div>}
    {data && scene && <>
      <div className={styles.toolbar}><label className={styles.find}><span className={styles.srOnly}>{words.filter}</span><input type="search" placeholder={words.filter} value={filter} list={listId} onChange={event => { const value = event.target.value; const term = scene.nodes.find(node => node.label === value || node.id === value); setFilter(value); if (term) choose(term.id); }} onKeyDown={event => { if (event.key === "Enter") { const id = [...matched][0]; if (id) choose(id); } }} /></label><datalist id={listId}>{[...data.classes, ...data.properties].map(term => <option key={term.id} value={term.label}>{compactTBoxId(term.id)}</option>)}</datalist>{selected && <button type="button" onClick={() => { setSelected(""); setFilter(""); }}>{words.overview}</button>}<button type="button" onClick={() => setExpanded(value => !value)} aria-expanded={expanded}>{expanded ? words.collapse : words.expand}</button></div>
      <div className={styles.counts}><span>{data.counts.allNamedClasses} {words.classes}</span><span>{data.counts.properties} {words.properties}</span><span>{data.counts.equivalences} {words.expressions}</span><span>{words.bothBridges}</span></div>
      <div className={styles.viewport} style={{ height }}>
        <svg ref={svg} className={styles.diagram} viewBox={`0 0 ${width} ${height}`} width={width} height={height} role="group" aria-label={words.title} tabIndex={0}
          onKeyDown={event => { const state = latest.current; if (event.key === "+" || event.key === "-") { event.preventDefault(); setCamera({ key, view: zoomTBoxView(view, event.key === "+" ? 1.2 : 1 / 1.2, { x: width / 2, y: height / 2 }) }); } else if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key) && event.target === event.currentTarget) { event.preventDefault(); setCamera({ key: state.key, view: { ...state.view, x: state.view.x + (event.key === "ArrowLeft" ? 40 : event.key === "ArrowRight" ? -40 : 0), y: state.view.y + (event.key === "ArrowUp" ? 40 : event.key === "ArrowDown" ? -40 : 0) } }); } }}
          onPointerDown={event => { if ((event.target as Element).closest('[data-tbox-node],[data-tbox-edge]')) return; pointers.current.set(event.pointerId, local(event)); event.currentTarget.setPointerCapture(event.pointerId); startGesture(); }}
          onPointerMove={event => { if (!pointers.current.has(event.pointerId) || !gesture.current) return; pointers.current.set(event.pointerId, local(event)); const points = [...pointers.current.values()], a = points[0], b = points[1] ?? a, center = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, start = gesture.current; const zoomed = start.distance > 0 && points.length > 1 ? zoomTBoxView(start.view, Math.hypot(a.x - b.x, a.y - b.y) / start.distance, start.center) : start.view; setCamera({ key, view: { ...zoomed, x: zoomed.x + center.x - start.center.x, y: zoomed.y + center.y - start.center.y } }); }}
          onPointerUp={event => { pointers.current.delete(event.pointerId); startGesture(); }} onPointerCancel={event => { pointers.current.delete(event.pointerId); startGesture(); }}>
          <defs>{Object.entries(COLORS).map(([kind, shade]) => <marker key={kind} id={`${markerId}-${kind}`} markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto" markerUnits="userSpaceOnUse"><path d="M0 0 L7 3.5 L0 7" fill="none" stroke={shade} strokeWidth="1.3" /></marker>)}</defs>
          <g transform={`translate(${view.x} ${view.y}) scale(${view.scale})`}>
            {routes.map(({ edge, path }) => {
              const incident = edge.id === activeEdge || edge.propertyId === selected || edge.source === selected || edge.target === selected || edge.source === active || edge.target === active;
              const shade = color(edge.kind, edge.module), marker = edge.kind === "subclass" && ["bfo", "dul"].includes(edge.module ?? "") ? edge.module : edge.kind;
              const box = visible.get(`edge:${edge.id}`), target = edge.propertyId ?? edge.target;
              return <g key={edge.id} className={styles.edge} data-active={incident} data-tbox-edge="" role="button" tabIndex={0} aria-label={`${edgeCaption(edge)} · ${compactTBoxId(edge.source)} → ${compactTBoxId(edge.target)}`} onClick={() => choose(target)} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); choose(target); } }} onPointerEnter={() => highlightEdge(edge.id, "pointer")} onPointerLeave={() => highlightEdge("", "pointer")} onFocus={() => highlightEdge(edge.id, "focus")} onBlur={() => highlightEdge("", "focus")}>
                <title>{`${compactTBoxId(edge.source)} → ${edgeCaption(edge)} → ${compactTBoxId(edge.target)} (${edge.module})`}</title><path className={styles.edgeHit} d={path} fill="none" stroke="transparent" strokeWidth={18 / view.scale} /><path d={path} fill="none" stroke={shade} strokeWidth={incident ? 1.8 : 1} strokeDasharray={edge.wildcard ? "5 4" : undefined} markerEnd={`url(#${markerId}-${marker})`} />{box && !box.needsReadout && <text x={(box.left + box.width / 2 - view.x) / view.scale} y={(box.top + box.height / 2 - view.y) / view.scale + 4} textAnchor="middle" className={styles.edgeText} fill={shade}>{shortName(edgeCaption(edge))}</text>}
              </g>;
            })}
            {scene.nodes.map(node => { const box = visible.get(node.id); return <g key={node.id} transform={`translate(${node.x} ${node.y})`} data-tbox-node="" role="button" tabIndex={0} aria-label={`${node.label} · ${node.kind === "property" ? words.property : node.kind === "expression" ? words.definition : words.class}`} aria-pressed={node.id === selected} className={styles.node} onClick={() => choose(node.id)} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); choose(node.id); } }} onPointerEnter={() => highlight(node.id, "pointer")} onPointerLeave={() => highlight("", "pointer")} onFocus={() => highlight(node.id, "focus")} onBlur={() => highlight("", "focus")}>
              <title>{`${node.label}\n${node.id}`}</title><circle className={styles.hit} r={22 / view.scale} /><circle className={styles.dot} r={3} fill={node.kind === "property" ? COLORS.domain : node.external && node.id.includes("DUL.owl") ? COLORS.dul : node.external ? COLORS.bfo : COLORS.subclass} />{box && !box.needsReadout && <text x={(box.left + box.width / 2 - view.x) / view.scale - node.x} y={(box.top + box.height / 2 - view.y) / view.scale - node.y + 4} textAnchor="middle" className={styles.nodeText} data-active={node.id === active}>{shortName(node.label)}</text>}
            </g>; })}
          </g>
        </svg>
        <div className={styles.controls}><button type="button" onClick={() => setCamera({ key, view: zoomTBoxView(view, 1.2, { x: width / 2, y: height / 2 }) })} aria-label={copy.zoomIn}>+</button><button type="button" onClick={() => setCamera({ key, view: zoomTBoxView(view, 1 / 1.2, { x: width / 2, y: height / 2 }) })} aria-label={copy.zoomOut}>−</button><button type="button" onClick={() => setCamera({ key, view: fitTBoxBounds({ left: 0, top: 0, width: scene.width, height: scene.height }, { width, height }) })}>{copy.fit}</button></div>
        {(activeNode || activeConnection) && <div className={styles.readout} aria-hidden="true"><strong>{activeNode?.label ?? (activeConnection ? edgeCaption(activeConnection) : "")}</strong><span>{activeNode ? compactTBoxId(activeNode.id) : activeConnection ? `${compactTBoxId(activeConnection.source)} → ${compactTBoxId(activeConnection.target)}` : ""}</span></div>}
      </div>
      <div className={styles.legend}>{["subclass", "bfo", "dul", "property", "equivalent", "disjoint"].map(kind => <span key={kind}><i style={{ background: COLORS[kind] }} />{kind === "bfo" || kind === "dul" ? `${kind.toUpperCase()} · ${words.subclass}` : caption(kind)}</span>)}<span>{words.allRelationships.replace("{shown}", String(scene.edges.length))}</span></div>
      {selected && <div className={styles.selection}><strong>{selectedNode?.label ?? compactTBoxId(selected)}</strong><code>{selected}</code>{(selectedClass?.definition || selectedProperty?.definition) && <p>{selectedClass?.definition ?? selectedProperty?.definition}</p>}{selectedProperty && <><p>{selectedProperty.kind === "datatype" ? words.datatype : words.property}</p><dl><div><dt>{words.domain}</dt><dd>{selectedProperty.domains.map(expressionText).join(" AND ")}</dd></div><div><dt>{words.range}</dt><dd>{selectedProperty.ranges.map(expressionText).join(" AND ")}</dd></div></dl></>}{selectedNode?.expression && <p><span>{words.definition}: </span><code>{expressionText(selectedNode.expression)}</code></p>}{equations.map((item, index) => <p key={`${item.module}-${index}`}><span>{words.equivalent} ({item.module}): </span><code>{expressionText(item.expression)}</code></p>)}{complexParents.map((item, index) => <p key={`${item.module}-${index}`}><span>{words.subclass}: </span><code>{expressionText(item.target)}</code></p>)}{!selected.startsWith(ATLAS_VOCAB) && selectedNode?.external && <p className={styles.scope}>{words.referenceScope}</p>}<div className={styles.adjacent}>{scene.edges.filter(edge => edge.source === selected || edge.target === selected).map(edge => { const id = edge.source === selected ? edge.target : edge.source; const node = scene.nodes.find(item => item.id === id); return <button key={edge.id} type="button" onClick={() => choose(id)}>{edgeCaption(edge)} · {node?.label ?? compactTBoxId(id)}</button>; })}</div></div>}
      <details className={styles.artifacts}><summary>{words.artifacts}</summary><p>{words.version} {data.version} · {data.validation.regressions} {words.checks}</p><p className={styles.scope}>{words.validationScope}</p><div className={styles.downloads}>{data.modules.map(item => <a key={item.id} href={`${basePath}/${item.file}`} download>{item.file}</a>)}<a href={`${basePath}/manifest.json`} download>{words.manifest}</a><a href={`${basePath}/validation.json`} download>{words.validation}</a><a href={`${basePath}/GOVERNANCE.md`} download>{words.governance}</a></div><details className={styles.verification}><summary>{copy.aboutData.verificationDetails}</summary><ul>{data.modules.map(item => <li key={item.id}>{item.file}<code>{item.sha256}</code></li>)}<li><a href={`${basePath}/view.json`} download>{words.diagramData}</a><code>{TBOX_PIN.sha256}</code></li></ul>{data.sourcePins.map(source => <p key={source.file} className={styles.sourcePin}><a href={source.source_url} target="_blank" rel="noopener noreferrer">{source.version}</a><span>{source.license}</span><code>{source.sha256}</code></p>)}</details></details>
    </>}
  </section>;
}
