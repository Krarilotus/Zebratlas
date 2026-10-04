// Actual candidate UI, Workspace handlers and proxy/client; all requests controlled.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import ts from "typescript";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { fixtureModule } from "./zebra-fixture-module.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), ".."), requireHere = createRequire(import.meta.url);
const read = file => readFileSync(resolve(root, file), "utf8");
const compile = file => ts.transpileModule(read(file), { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText;
const loadComponent = (file, imports) => { const value = {}; new Function("exports", "require", compile(file))(value, name => imports(name)); return value; };
const candidates = fixtureModule("@/components/zebra/possible-matches"), locale = fixtureModule("@/lib/zebra/locale");
const match = { id: "MONDO:0004975", label: "Alzheimer disease", kind: "disease", match: "fuzzy", method: "lexical", score: .8 };
const fallback = { query: "Question about possible research contacts", interpretation: { mode: "indexed", entities: [], intent: "all" }, results: [], graph: { nodes: [], edges: [] }, execution: { engine: "indexed-atlas", sparql: null, queries: [], elapsed_ms: 1, truncated: false }, possible_matches: [match], retrieval: { status: "candidates", scope: "name_candidates_only", index_sha256: "fixture-index", truncated: false } };
assert.deepEqual(candidates.possibleMatchState(fallback), { matches: [match], invalid: false });
assert.deepEqual(candidates.possibleMatchPlan(fallback, match.id), { focus: [match.id], intent: "all", filters: {} });
assert.equal(candidates.possibleMatchPlan(fallback, "unreturned-id"), null);
assert.equal(candidates.possibleMatchState({ ...fallback, retrieval: undefined }), null);
assert.equal(candidates.possibleMatchState({ ...fallback, results: [{ id: match.id }] }), null, "Actual anchored results cannot be replaced by a candidate view");
const empty = { ...fallback, possible_matches: [], retrieval: { ...fallback.retrieval, status: "empty" } };
assert.deepEqual(candidates.possibleMatchState(empty), { matches: [], invalid: false });
assert(candidates.possibleMatchState({ ...empty, possible_matches: [match] }).invalid);
assert.deepEqual(candidates.possibleMatchState({ ...fallback, possible_matches: [match, match] }), { matches: [match], invalid: false }, "A duplicate cannot hide the valid source record");
assert(candidates.possibleMatchState({ ...fallback, possible_matches: [{ ...match, score: NaN }] }).invalid);
assert(candidates.possibleMatchState({ ...fallback, possible_matches: Array.from({ length: 11 }, (_, index) => ({ ...match, id: `fixture:${index}` })) }).invalid);
const longPaper = { ...match, id: "PMID:fixture-long", kind: "paper", label: "Actual long paper title ".repeat(80) };
const gene = { ...match, id: "HGNC:11444", label: "STXBP1", kind: "gene" };
const longResponse = { ...fallback, possible_matches: [gene, longPaper] };
assert.deepEqual(candidates.possibleMatchState(longResponse), { matches: [gene, longPaper], invalid: false }, "Long actual titles must preserve other matches and original source text");
assert.deepEqual(candidates.possibleMatchPlan(longResponse, longPaper.id), { focus: [longPaper.id], intent: "all", filters: {} });
for (const bad of [{ ...longPaper, label: "x".repeat(32769) }, { ...longPaper, label: "é".repeat(16385) }, { ...longPaper, score: NaN }, null]) {
  const mixed = { ...fallback, possible_matches: [gene, bad, longPaper] };
  assert.deepEqual(candidates.possibleMatchState(mixed), { matches: [gene, longPaper], invalid: false }, "Oversize or malformed records are isolated, not a whole-list failure");
}
assert(candidates.possibleMatchState({ ...fallback, possible_matches: [{ ...longPaper, label: "é".repeat(16385) }] }).invalid, "All-invalid candidates are unavailable, not evidence of no indexed name");
assert.equal(candidates.possibleMatchState({ ...fallback, possible_matches: [{ ...longPaper, label: "é".repeat(16384) }] }).matches[0].label.length, 16384, "The ceiling is exactly32KiB UTF8, not500characters");
assert.equal(candidates.possibleMatchPlan(empty, match.id), null);
assert.equal(candidates.publicSearchCaption("Which organisations could help?"), "Which organisations could help?");
assert.equal(candidates.publicSearchCaption("x".repeat(511) + "😀").length, 511, "Caption bounds cannot split a Unicode surrogate pair");
assert.equal(candidates.publicSearchCaption("é".repeat(255) + "😀"), "é".repeat(255), "Caption bounds use UTF-8 bytes and preserve whole characters");

const noop = () => null;
for (const lang of ["en", "de", "fr"]) {
  const copy = locale.getZebraCopy(lang);
  const Panel = loadComponent("components/zebra/PossibleMatches.tsx", name => name === "./Locale" ? { useZebraCopy: () => copy, useZebraKindLabel: () => kind => copy.kinds[kind] ?? kind } : name.endsWith(".css") ? {} : requireHere(name)).default;
  let chosen;
  const props = { matches: [match], onSelect: value => { chosen = value; } };
  const html = renderToStaticMarkup(React.createElement(Panel, props));
  assert(html.includes(copy.possibleMatches.title) && html.includes(copy.possibleMatches.scope));
  assert(!html.includes(".8") && !html.includes("SPARQL") && !html.includes('role="alert"'), "Lexical scores are not scientific evidence or a model-error box");
  const walk = node => !node || typeof node !== "object" ? [] : [node, ...[node.props?.children].flat(Infinity).flatMap(walk)];
  walk(Panel(props)).find(node => node.type === "button").props.onClick();
  assert.equal(chosen, match, "Selection returns only the actual recorded candidate");
  const emptyHtml = renderToStaticMarkup(React.createElement(Panel, { matches: [], onSelect: noop }));
  assert(emptyHtml.includes(copy.possibleMatches.empty) && !emptyHtml.includes(copy.emptyHint));
  assert(walk(Panel({ ...props, busy: true })).find(node => node.type === "button").props.disabled);
  const longHtml = renderToStaticMarkup(React.createElement(Panel, { matches: [longPaper], onSelect: noop }));
  assert(longHtml.includes(`title="${longPaper.label}"`) && longHtml.includes(`aria-label="${longPaper.label} · ${copy.possibleMatches.select}"`), "Clipped titles retain exact source labels for pointer and assistive access");
}
assert(read("components/zebra/PossibleMatches.module.css").includes("-webkit-line-clamp:2"), "Only presentation clips long titles to two lines");

// Exercise the actual Workspace callback, including canceled responses and URL privacy.
const prepared = "Private extracted document text\nWhich patient organisations can we contact?", caption = "étude-🧬.txt";
const states = [], refs = []; let stateCursor = 0, refCursor = 0;
const hooks = { ...React, useMemo: callback => callback(), useCallback: callback => callback, useEffect() {}, useLayoutEffect() {}, useState(initial) { const slot = stateCursor++; if (!(slot in states)) states[slot] = slot === 2 || slot === 3 ? fallback : typeof initial === "function" ? initial() : initial; return [states[slot], value => { states[slot] = typeof value === "function" ? value(states[slot]) : value; }]; }, useRef(initial) { const slot = refCursor++; if (!(slot in refs)) refs[slot] = { current: initial === caption ? prepared : initial }; return refs[slot]; } };
const requests = [], history = [], components = new Map();
function stub(name) { if (!components.has(name)) components.set(name, noop.bind(null)); return components.get(name); }
const SearchBox = stub("SearchBox"), CandidateView = stub("PossibleMatches");
const originalWindow = globalThis.window, originalDocument = globalThis.document;
globalThis.window = { location: { href: "http://web.fixture.invalid/zebra?lang=en" }, history: { state: {}, replaceState(state, _title, url) { this.state = state; history.push({ state, url }); } } };
globalThis.document = { documentElement: { lang: "en" }, activeElement: null };
const body = loadComponent("components/zebra/Workspace.tsx", name => {
  if (name === "react") return hooks;
  if (name === "next/dynamic") return { __esModule: true, default: loader => stub(loader.toString().match(/\.\/([A-Za-z]+)/)?.[1] ?? "Dynamic") };
  if (name === "next/link") return { __esModule: true, default: stub("Link") };
  if (name.endsWith(".css")) return { __esModule: true, default: {} };
  if (name === "./Locale") return { useZebraCopy: () => locale.getZebraCopy("en"), useZebraLocale: () => "en", useZebraKindLabel: () => kind => kind };
  if (name === "@/lib/zebra/client") return { explore(query, options) { let complete; const promise = new Promise(resolve => { complete = resolve; }); requests.push({ query, options, complete }); return promise; } };
  if (name === "./SearchBox") return { SearchBox };
  if (["./Icon", "./ZebraLoader", "./Brand", "./AccountControl", "./LanguagePicker", "./NetworkDirectory", "./Privacy"].includes(name)) return new Proxy({}, { get: (_, key) => stub(String(key)) });
  if (name === "./ContactFilters") return { __esModule: true, default: stub("ContactFilters") };
  if (name.startsWith("./")) return fixtureModule(`@/components/zebra/${name.slice(2)}`);
  return fixtureModule(name);
}).Workspace;
const walk = node => !node || typeof node !== "object" ? [] : [node, ...[node.props?.children].flat(Infinity).flatMap(walk)];
function renderWorkspace() { stateCursor = 0; refCursor = 0; return body({ initialQuery: caption }); }
const flush = async () => { for (let i = 0; i < 4; i++) await new Promise(resolve => setImmediate(resolve)); };
try {
  let tree = renderWorkspace(), elements = walk(tree);
  const shown = elements.find(node => node.type === CandidateView);
  assert(shown && !elements.some(node => node.props?.className === "z-query-info"), "The normal results page exposes candidates without suggesting an executed query");
  assert(!elements.some(node => node.type === "h2" && node.props.children === locale.getZebraCopy("en").empty), "No generic no-results claim competes with indexed candidates");
  assert.equal(requests.length, 0, "Rendering candidates chooses no identity and runs no query");
  shown.props.onSelect(match);
  assert.equal(requests[0].query, prepared); assert.equal(requests[0].options.caption, caption);
  assert.deepEqual(requests[0].options.plan, { focus: [match.id], intent: "all", filters: {} });
  assert.equal(requests[0].options.mode, "knowledge"); assert.equal(states[0], caption, "The candidate name cannot replace the original question/public caption");
  tree = renderWorkspace();
  walk(tree).find(node => node.type === SearchBox).props.onSearch("KIF1A");
  assert(requests[0].options.signal.aborted, "A new submission cancels the candidate lookup");
  requests[0].complete(fallback); await flush();
  assert.equal(states[2], null, "An aborted candidate answer cannot replace the newer results");
  requests[1].complete({ ...fallback, query: "KIF1A", retrieval: undefined }); await flush();
  assert.equal(states[0], "KIF1A");
  assert(!JSON.stringify(history).includes(prepared) && !JSON.stringify(history).includes(caption), "History retains navigation IDs and safe short queries, never extracted text");
} finally { globalThis.window = originalWindow; globalThis.document = originalDocument; }

// Actual client and same-origin proxy keep prepared text separate from public response captions.
const modules = new Map(), httpCalls = [], originalFetch = globalThis.fetch;
const mocks = { "server-only": {}, "next/headers": { cookies: async () => ({ get() {}, set() {}, delete() {} }), headers: async () => new Headers({ origin: "http://web.fixture.invalid" }) }, "@/lib/adapt": {} };
function httpModule(name, from = root) {
  if (Object.hasOwn(mocks, name)) return mocks[name];
  if (!name.startsWith("@/") && !name.startsWith(".")) return requireHere(name);
  let file = name.startsWith("@/") ? resolve(root, name.slice(2)) : resolve(from, name);
  if (extname(file) === ".json") return JSON.parse(readFileSync(file, "utf8"));
  if (!extname(file)) file += ".ts";
  if (modules.has(file)) return modules.get(file);
  const value = {}; modules.set(file, value);
  new Function("exports", "require", ts.transpileModule(readFileSync(file, "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText)(value, dependency => httpModule(dependency, dirname(file)));
  return value;
}
process.env.ZEBRA_BACKEND_URL = "http://atlas.fixture.invalid";
globalThis.fetch = async (url, init) => { httpCalls.push({ url: String(url), body: JSON.parse(init.body) }); return new Response(JSON.stringify({ ...fallback, query: prepared }), { headers: { "content-type": "application/json" } }); };
try {
  const client = httpModule("@/lib/zebra/client"), route = httpModule("@/app/zebra/api/[operation]/route");
  await client.explore(prepared, { caption, plan: candidates.possibleMatchPlan(fallback, match.id) });
  assert.equal(httpCalls.at(-1).body.query, prepared); assert.equal(httpCalls.at(-1).body.caption, caption);
  async function post(body) { return route.POST(new Request("http://web.fixture.invalid/zebra/api/search", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }), { params: Promise.resolve({ operation: "search" }) }); }
  const reply = await post({ query: prepared, caption }); assert.equal(reply.status, 200);
  const response = await reply.json(); assert.equal(response.query, caption); assert(!JSON.stringify(response).includes(prepared), "Candidate response never echoes prepared text even from an older upstream");
  const question = "Which organisations could help?";
  assert.equal((await (await post({ query: question, caption: question })).json()).query, question);
  assert.equal((await (await post({ query: prepared })).json()).query, "", "Missing public captions never fall back to private prepared text");
  const beforeInvalid = httpCalls.length;
  assert.equal((await post({ query: prepared, caption: "x".repeat(513) })).status, 400);
  assert.equal((await post({ query: prepared, caption: "é".repeat(257) })).status, 400, "Character-valid but UTF-8-oversized captions are rejected");
  assert.equal(httpCalls.length, beforeInvalid, "Invalid caption bounds cannot call upstream");
} finally { globalThis.fetch = originalFetch; }
console.log("PASS: actual lexical candidate/empty UI, explicit zero-model plan selection, prepared caption/history privacy, stale-request rejection and real client/proxy caption bounds.");
