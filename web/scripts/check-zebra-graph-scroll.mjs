import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = name => fs.readFileSync(path.join(root, name), "utf8");
const source = read("components/zebra/Graph.tsx");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
const start = compiled.indexOf("const wheel = (event) => {");
const end = compiled.indexOf('host.addEventListener("wheel"', start);
assert(start > 0 && end > start);
const wheelCode = compiled.slice(start, end);
let painted = 0, canceled = 0;
const viewRef = { current: { scale: 1, x: 0, y: 0 } };
const layerRef = { current: { style: {}, querySelectorAll: () => [] } };
const host = { dataset: {}, clientHeight: 700 };
const canvas = { getBoundingClientRect: () => ({ left: 20, top: 30 }) };
const wheel = new Function("host", "canvas", "transitionRef", "positionsRef", "targetPositions", "sceneRef", "routesRef", "layerRef", "wheelView", "viewRef", "requestPaintRef", wheelCode + "return wheel;")(
  host, canvas, { current: {} }, { current: new Map() }, () => new Map(), { current: {} }, { current: [] }, layerRef,
  (view, delta) => ({ ...view, scale: view.scale * (delta < 0 ? 1.1 : 1 / 1.1) }), viewRef, { current: () => painted++ });
const event = ctrlKey => ({ ctrlKey, deltaY: -100, deltaMode: 0, clientX: 220, clientY: 130, preventDefault: () => canceled++, stopPropagation: () => canceled++ });
wheel(event(false));
assert.equal(canceled, 0, "Ordinary wheel reaches native page scrolling");
assert.equal(painted, 0, "Ordinary wheel does not mutate or repaint the graph");
assert.equal(viewRef.current.scale, 1);
wheel(event(true));
assert.equal(canceled, 2, "Ctrl+wheel cancels browser zoom and propagation");
assert.equal(painted, 1);
assert(viewRef.current.scale > 1);
const css = read("components/zebra/Graph.module.css");
assert(css.includes("transform:translateY(calc(-1 * var(--z-hero-offset,0px)))"));
assert(css.includes(":global(.z-hero-scene) .graph { min-height:0; }"), "Phone minimum heights cannot overflow the stable hero");
// The scene moves by scroll/2; only its caption/pager counteracts that movement.
for (const [height, compact, bottom] of [[380, 138, 12], [680, 172, 21]]) {
  for (const scroll of [0, (height - compact) / 2, height - compact]) {
    const offset = scroll / 2;
    const captionBottom = -scroll + offset + height - bottom - offset;
    assert.equal(captionBottom, height - scroll - bottom, "Navigation stays above the visible hero bottom throughout scroll");
    assert(captionBottom > 44);
  }
}
const workspace = read("components/zebra/Workspace.tsx");
assert(workspace.includes('top: expand ? 0 : heroDistance.current'), "Explicit graph entry scrolls to the top");
assert(workspace.includes('if (!plan && readingScroll.current) { readingScroll.current.scrollTop = 0; }'), "A fresh search enters at graph top");
assert(workspace.includes('scrollTop = pendingScroll.current'), "History restores its recorded scroll");
assert(workspace.includes('scrollTo({ top: snapshot.scroll, behavior: "auto" })'), "Back to results preserves prior scroll");
assert(!wheelCode.includes(".focus("), "Wheel navigation never steals typing focus");
console.log("Graph scroll: Ctrl-only zoom, native wheel, pinned caption/pager and graph-entry/history checks passed.");
