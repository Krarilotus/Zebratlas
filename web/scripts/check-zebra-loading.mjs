import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
import ts from "typescript";

const require = createRequire(import.meta.url);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
function compile(relative, imports = {}, globals = {}) {
  const source = fs.readFileSync(path.join(root, relative), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  return new Function("exports", "require", ...Object.keys(globals), js + ";return exports;")({}, name => imports[name] ?? require(name), ...Object.values(globals));
}
const geometry = compile("lib/zebra/brand-geometry.ts");
const imports = { "@/lib/zebra/brand-geometry": geometry, "./ZebraLoader.module.css": { default: { loader: "loader", turn: "turn" } } };
const { ZebraLoader, STRIPES, zebraFrame } = compile("components/zebra/ZebraLoader.tsx", imports);
const native = geometry.markRectangles(geometry.NATIVE_MARKS.find(grid => grid.name === "display"));
assert.deepEqual(STRIPES.slice(0, 5).map(row => row.z), native, "The Z must use the existing brand's exact stripes");
assert(STRIPES.slice(5).every(row => row.z.width === 0), "Extra profile segments disappear in the Z");
assert.deepEqual(zebraFrame(0), { morph: 0, turn: 0 });
assert.deepEqual(zebraFrame(3000), { morph: 1, turn: 180 });
assert.deepEqual(zebraFrame(6000), zebraFrame(0));
for (let ms = 0; ms < 6000; ms += 37) {
  const { morph, turn } = zebraFrame(ms);
  assert(morph >= 0 && morph <= 1 && turn >= 0 && turn < 360);
  for (const { z, head } of STRIPES) {
    assert.equal(z.y, head.y); assert.equal(z.height, head.height);
    const x = z.x + (head.x - z.x) * morph;
    const width = z.width + (head.width - z.width) * morph;
    assert(x >= 0 && width >= 0 && x + width <= 24);
  }
}
const html = renderToStaticMarkup(createElement(ZebraLoader));
assert(html.includes('aria-hidden="true"') && html.includes('width="24" height="24"'));
assert.equal((html.match(/<rect /g) ?? []).length, 8);
assert(!html.includes("<path") && !html.includes("<mask") && !html.includes("<animate"));

// Exercise active RAF cancellation, changing preferences and StrictMode remount.
const bars = STRIPES.map(() => ({ attrs: {}, setAttribute(name, value) { this.attrs[name] = Number(value); } }));
const svg = { querySelectorAll: () => bars, style: {} };
const frames = new Map(), listeners = new Set();
let nextFrame = 0, now = 0, cleanup;
const media = { matches: false, addEventListener(_, fn) { listeners.add(fn); }, removeEventListener(_, fn) { assert(listeners.delete(fn)); } };
const react = { ...require("react"), useRef: () => ({ current: svg }), useEffect: fn => { cleanup = fn(); } };
const mock = compile("components/zebra/ZebraLoader.tsx", { ...imports, react }, {
  matchMedia: () => media, performance: { now: () => now },
  requestAnimationFrame: fn => { const id = ++nextFrame; frames.set(id, fn); return id; },
  cancelAnimationFrame: id => frames.delete(id),
});
function advance(to) {
  now = to;
  const scheduled = [...frames]; frames.clear();
  for (const [, callback] of scheduled) callback(to);
}
function preference(matches) { media.matches = matches; [...listeners].forEach(fn => fn()); }
mock.ZebraLoader();
assert.equal(frames.size, 1); assert.equal(listeners.size, 1);
assert.deepEqual(bars.slice(0, 5).map(bar => bar.attrs.x), native.map(row => row.x), "active animation starts at the corrected native diagonal");
advance(3000); assert(svg.style.transform.includes("rotateY(180deg)")); assert.equal(frames.size, 1);
cleanup(); assert.equal(frames.size, 0, "unmount cancels the current frame, not an earlier frame id"); assert.equal(listeners.size, 0);
mock.ZebraLoader(); assert.equal(frames.size, 1); assert.equal(listeners.size, 1);
preference(true); assert.equal(frames.size, 0); assert.equal(svg.style.transform, "none");
assert.deepEqual(bars.map(bar => bar.attrs.width), STRIPES.map(row => row.head.width));
preference(false); assert.equal(frames.size, 1); assert.equal(listeners.size, 1);
assert.deepEqual(bars.slice(0, 5).map(bar => bar.attrs.width), native.map(row => row.width));
advance(4500); assert.equal(frames.size, 1);
cleanup(); assert.equal(frames.size, 0); assert.equal(listeners.size, 0);
preference(true); mock.ZebraLoader(); assert.equal(frames.size, 0); assert.equal(listeners.size, 1);
cleanup(); assert.equal(frames.size, 0); assert.equal(listeners.size, 0);
console.log("PASS: exact native Z, bounded profile/yaw, active frame cancellation, reduced-motion preference changes and StrictMode remount cleanup");

// A static profile SVG for inspection; live motion is computed by the component.
if (process.argv[2]) {
  const svg = html.slice(html.indexOf("<svg"), html.lastIndexOf("</svg>") + 6)
    .replace("<svg ", '<svg xmlns="http://www.w3.org/2000/svg" ')
    .replace('width="24" height="24"', 'width="192" height="192"');
  fs.writeFileSync(process.argv[2], svg);
  console.log(`Exported static stripe profile: ${Buffer.byteLength(svg)} bytes`);
}
