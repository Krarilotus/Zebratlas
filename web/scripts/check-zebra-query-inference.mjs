/* Exercise the actual QueryPlan handlers with deterministic hook state; no provider calls. */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import ts from "typescript";
import { fixtureModule } from "./zebra-fixture-module.mjs";
const requireFromHere = createRequire(import.meta.url);
const compile = name => ts.transpileModule(readFileSync(new URL(`../components/zebra/${name}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020, esModuleInterop: true } }).outputText;
const copy = {};
new Function("exports", compile("queryWorkspaceCopy.ts"))(copy);
const text = copy.queryWorkspaceCopy.en;
const states = []; let cursor = 0;
const hooks = {
  useMemo: factory => factory(), useEffect() {},
  useRef(initial) { const slot = cursor++; if (!(slot in states)) states[slot] = { current: initial }; return states[slot]; },
  useState(initial) { const slot = cursor++; if (!(slot in states)) states[slot] = initial; return [states[slot], value => { states[slot] = typeof value === "function" ? value(states[slot]) : value; }]; },
};
const componentExports = {};
new Function("exports", "require", compile("QueryPlan.tsx"))(componentExports, name => {
  if (name === "react") return hooks;
  if (name === "next/dynamic") return { __esModule: true, default: () => () => null };
  if (name === "./Locale") return { useZebraLocale: () => "en" };
  if (name === "@/lib/zebra/locale") return fixtureModule(name);
  if (name === "./queryWorkspaceCopy") return copy;
  if (name === "./query-workspace") return { queryReceipts: data => data?.receipts || [], canMutateQuery: () => false, queryFitsBudget: () => true };
  if (name.endsWith(".css")) return { __esModule: true, default: new Proxy({}, { get: (_, key) => key }) };
  if (["./Icon", "./ZebraLoader"].includes(name)) return { Icon: () => null, ZebraLoader: () => null };
  return requireFromHere(name);
});
const receipt = (key, reasoning, limit) => ({ key, text: `SELECT ?result WHERE { ?result <urn:actual> ?value } LIMIT ${limit}`, stage: "canonical", backend: "nrese", linked: [{ id: `ID:${key}`, label: key, kind: "gene" }], settings: { reasoning, limit, focus: [`ID:${key}`], semantic_focus: [`DISEASE:${key}`] } });
const receipts = [receipt("off-receipt", false, 20), receipt("on-receipt", true, 40)];
const original = JSON.stringify(receipts), calls = [];
const data = receipts => ({ receipts, graph: {}, execution: {}, interpretation: {} });
let props = { data: data(receipts), query: "Original public caption", busy: false, onRunSparql: (sparql, settings) => calls.push({ sparql, settings }) };
function render() { cursor = 0; return componentExports.default(props); }
function walk(node) { if (!node || typeof node !== "object") return []; return [node, ...[node.props?.children].flat(Infinity).flatMap(walk)]; }
function inference(tree) { return walk(tree).find(node => node.type === "select" && node.props["aria-label"] === text.infer); }
function selection(tree) { return walk(tree).find(node => node.type === "select" && node.props["aria-label"] === text.query); }
function run(tree) { walk(tree).find(node => node.type === "button" && node.props.className === "run").props.onClick(); }
function technicalMode(tree) { const technical = walk(tree).find(node => node.type === "details" && node.props.className === "technical"); return walk(technical).filter(node => node.type === "dd").map(node => node.props.children); }

let tree = render();
assert.equal(inference(tree).props.value, "false", "The untouched receipt preserves its actual inference-off setting");
inference(tree).props.onChange({ target: { value: "true" } });
assert.equal(calls.length, 0, "Changing a mode never automatically executes a query");
tree = render(); assert.equal(inference(tree).props.value, "true");
assert(technicalMode(tree).includes(text.no), "The original technical receipt still records inference off");
run(tree); assert.equal(calls[0].settings.reasoning, true); assert.equal(calls[0].sparql, receipts[0].text);
assert.deepEqual(calls[0].settings.focus, receipts[0].settings.focus);
assert.deepEqual(calls[0].settings.semantic_focus, receipts[0].settings.semantic_focus);
assert.equal(calls[0].settings.limit, 20); assert.deepEqual(calls[0].settings.linked, receipts[0].linked);
selection(tree).props.onChange({ target: { value: "1" } });
tree = render(); assert.equal(inference(tree).props.value, "true", "A second receipt starts from its actual inference-on setting");
inference(tree).props.onChange({ target: { value: "false" } });
tree = render(); run(tree); assert.equal(calls[1].settings.reasoning, false); assert.equal(calls[1].settings.limit, 40);
assert(technicalMode(tree).includes(text.yes), "An explicit off override does not rewrite an executed on receipt");
selection(tree).props.onChange({ target: { value: "0" } }); tree = render();
assert.equal(inference(tree).props.value, "true", "Explicit overrides persist only for their exact receipt key");
assert.equal(JSON.stringify(receipts), original, "Query text, receipt modes, scope and provenance inputs are immutable");
props = { ...props, busy: true }; tree = render(); assert.equal(inference(tree).props.disabled, true);
props = { ...props, busy: false, data: data([receipt("new-execution", false, 20)]) };
tree = render(); assert.equal(inference(tree).props.value, "false", "A returned new execution uses its own actual mode rather than inheriting an old override");
props = { ...props, data: data([{ ...receipt("unknown", false, 20), settings: undefined }]) };
tree = render(); assert.equal(inference(tree), undefined, "Unknown execution settings cannot invent a mode switch");
assert.equal(props.query, "Original public caption");
const css = readFileSync(new URL("../components/zebra/QueryPlan.module.css", import.meta.url), "utf8");
assert.match(css, /\.capControl select\s*\{[^}]*min-height:44px;[^}]*var\(--z-line\)[^}]*var\(--z-paper\)[^}]*var\(--z-ink\)/, "The native mode control has a phone touch target and theme colors");
console.log("Query inference: exact receipt defaults, explicit off/on handlers, immutable executed modes, per-receipt state and unchanged rerun scope/cap/text passed.");
