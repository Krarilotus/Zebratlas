/* Exercise the real renderer without claiming browser FPS or device verification. */
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import ts from "typescript";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const directory = path.dirname(fileURLToPath(import.meta.url));
const requireFromHere = createRequire(import.meta.url);
const component = path.join(directory, "../components/zebra");
const compile = (name) => ts.transpileModule(fs.readFileSync(path.join(component, name), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
}).outputText;
const helper = {};
new Function("exports", compile("graph-labels.ts"))(helper);
const { paint, contextHit } = new Function("exports", "require", compile("Graph.tsx") + ";return {paint,contextHit};")({}, name =>
  name === "./graph-labels" ? helper : name.endsWith(".css") || ["./ReasoningProof", "./Sources", "./ZebraLoader", "./graph-preview"].includes(name) ? {} : requireFromHere(name));

function recordingCanvas() {
  const text = [], dots = [], strokes = [];
  let measurements = 0;
  const context = {
    font: "13px TestSans", lineWidth: 0, shadowBlur: 0,
    setTransform() {}, clearRect() {}, beginPath() {}, moveTo() {}, bezierCurveTo() {},
    setLineDash() {}, closePath() {}, fill() {}, lineTo() {}, ellipse() {}, fillRect() {},
    measureText(value) { measurements++; return { width: value.length * parseFloat(this.font) * 0.6 }; },
    fillText(value, x, y) { text.push({ value, x, y, font: this.font, color: this.fillStyle }); },
    arc(x, y, radius) { dots.push({ x, y, radius }); },
    stroke() { strokes.push({ width: this.lineWidth, glow: this.shadowBlur }); },
  };
  return { getContext: () => context, text, dots, strokes, measurementCount: () => measurements,
    clear() { text.length = 0; dots.length = 0; strokes.length = 0; } };
}
const node = (id, name, overrides = {}) => ({ id, label: name, name, kind: "gene", level: "context", x: 200, y: 200, width: 0, distance: 1, degree: 2, sharedSeeds: 0, ...overrides });
const scene = (nodes, foreground = [], edges = []) => ({ nodes, foreground, edges, totalNeighbors: 0, page: 0, pages: 1 });
const positions = data => new Map(data.nodes.map(item => [item.id, { x: item.x, y: item.y }]));
const palette = { fontFamily: "TestSans", muted: "#4c6470", active: "#102e2c", surface: "#f8fafb" };
const canvas = recordingCanvas();

for (const scale of [0.6, 1, 2.4]) {
  const data = scene([node("ordinary", "Ordinary name")]);
  const view = { scale, x: 400 - 200 * scale, y: 300 - 200 * scale };
  paint(canvas, data, [], positions(data), view, 1200, 760, 1, null, null, palette);
  assert.equal(canvas.text.length, 1);
  assert.equal(parseFloat(canvas.text[0].font), 13 * scale, "Quiet label font follows the exact scene zoom");
  assert.equal(canvas.text[0].font.slice(canvas.text[0].font.indexOf(" ") + 1), "TestSans", "Measured and drawn text use the actual supplied font family");
  assert.deepEqual(canvas.dots.map(dot => dot.radius), [2.7], "Zoom does not enlarge the existing context dots");
  const measured = canvas.measurementCount();
  canvas.clear();
  paint(canvas, data, [], positions(data), view, 1200, 760, 1, null, null, palette);
  assert.equal(canvas.measurementCount(), measured, "A stationary frame reuses measured scene labels");
  canvas.clear();
  paint(canvas, data, [], positions(data), view, 1200, 760, 1, "ordinary", null, palette);
  assert(canvas.text.length > 0);
  assert(canvas.text.every(item => parseFloat(item.font) === 14 * scale), "The stronger active name follows the same zoom with its 14px base");
  assert(canvas.text.every(item => item.color === palette.active));
  assert.deepEqual(canvas.dots.map(dot => dot.radius), [2.7], "Hover changes label contrast rather than dot size");
  canvas.clear();
}

const overlaps = scene([node("quiet", "Quiet contender"), node("bridge", "Shared bridge", { sharedSeeds: 3 }), node("far", "Distant label", { distance: 3 })]);
const snapshot = JSON.stringify(overlaps);
paint(canvas, overlaps, [], positions(overlaps), { scale: 1, x: 0, y: 0 }, 1000, 700, 1, null, null, palette);
assert.deepEqual(canvas.text.map(item => item.value), ["Shared bridge"], "A real shared-seed bridge wins overlapping lower-priority names");
assert.equal(canvas.dots.length, 3, "Suppressed names retain every actual context node");
assert.equal(JSON.stringify(overlaps), snapshot, "Collision culling never mutates the graph or its positions");
for (const id of ["quiet", "far"]) {
  canvas.clear();
  paint(canvas, overlaps, [], positions(overlaps), { scale: 1, x: 0, y: 0 }, 1000, 700, 1, id, null, palette);
  assert(canvas.text.some(item => item.value === overlaps.nodes.find(item => item.id === id).name), "Any hidden name can become readable through hover or keyboard focus");
}
const hidden = scene([node("hidden", "Name omitted by collision", { x: 150, y: 160 })]);
assert.equal(contextHit(hidden, positions(hidden), { scale: 0.6, x: 10, y: 20 }, { x: 100, y: 116 }).id, "hidden", "Canvas hit testing uses actual nodes independently of name visibility");
assert.equal(contextHit(hidden, positions(hidden), { scale: 1, x: 0, y: 0 }, { x: 167, y: 160 }).id, "hidden", "Hidden labels retain the 18px context hit target");

canvas.clear();
const longName = "Unbroken".repeat(40);
const oversized = scene([node("long", longName)]);
paint(canvas, oversized, [], positions(oversized), { scale: 2.4, x: -285, y: -170 }, 390, 620, 1, "long", null, palette);
assert.equal(canvas.text.length, 0, "An oversized full name uses the existing UI readout instead of painting across reserved labels");
assert.equal(canvas.dots.length, 1, "Readout fallback keeps its real node visible and selectable");

canvas.clear();
const a = node("A", "A", { level: "anchor", width: 120, x: 250, y: 300 });
const b = node("B", "B", { level: "anchor", width: 120, x: 750, y: 300 });
const edges = ["first", "second"].map(id => ({ id, source: "A", target: "B", relation: "same_property", kind: "observed", evidence: [] }));
const connected = { ...scene([a, b], [a, b], edges), overview: true };
const routes = edges.map((edge, index) => ({ edge, lane: index * 30, labeled: false, color: "#17675f", point: { x: 500, y: 300 }, width: 0, height: 0 }));
paint(canvas, connected, routes, positions(connected), { scale: 1, x: 0, y: 0 }, 1000, 700, 1, null, "second", palette);
assert.deepEqual(canvas.strokes, [{ width: 1.35, glow: 0 }, { width: 2.5, glow: 5 }], "Hover emphasizes only the exact assertion ID, even when predicates and endpoints coincide");
console.log("Graph label renderer: zoom, collision, hit-target, readout, and exact-assertion checks passed.");
