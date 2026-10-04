import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
const directory = path.dirname(fileURLToPath(import.meta.url)), require = createRequire(import.meta.url);
const read = file => fs.readFileSync(path.join(directory, "..", file), "utf8");
const load = (file, resolve = require) => {
  const exports = {};
  const compiled = ts.transpileModule(read(file), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX, esModuleInterop: true } }).outputText;
  new Function("exports", "require", compiled)(exports, resolve);
  return exports;
};
const errors = load("lib/zebra/explore-error.ts"), lookup = load("lib/zebra/indexed-lookup.ts");
const typed = Object.assign(new Error("sensitive internal diagnostic"), { code: "model_rate_limited", status: 429 });
assert.deepEqual(errors.classifyExploreError(typed), { kind: "quota", retryable: false, modelSettings: true });
const legacy = new Error('Provider response 429: {"error":{"code":429,"message":"Rate limit exceeded: free-models-per-day","metadata":{"internal":"hidden-sensitive-key"}}}');
assert.equal(errors.classifyExploreError(legacy).kind, "quota", "Legacy structured daily limits remain recognized without displaying provider diagnostics");
assert.equal(errors.classifyExploreError(new Error("quota exceeded" )).kind, "unknown", "Unstructured prose must not be treated as a verified model quota");
assert.equal(errors.classifyExploreError({ code: "account_rate_limited", status: 429 }).kind, "unknown", "A different subsystem's 429 is not a model fault");
assert.equal(errors.classifyExploreError(new Error('bad JSON {"error": broken')).kind, "unknown");
assert.equal(errors.classifyExploreError({ code: "model_unavailable" }).kind, "provider");
assert.equal(errors.classifyExploreError({ status: 400 }).kind, "invalid");
assert.equal(errors.classifyExploreError({ status: 422, code: "unsupported_question", message: "private scope diagnostic" }).kind, "unsupported");
assert(lookup.isShortIndexedName("Altzheimer"));
assert(!lookup.isShortIndexedName("Which studies could help my child?"));
assert(!lookup.isShortIndexedName("Document text\n" + "private ".repeat(20)));

for (const language of ["en", "de"]) {
  const copy = JSON.parse(read(`messages/zebra/${language}.json`)).copy;
  const view = load("components/zebra/ExploreError.tsx", name => name === "@/lib/zebra/explore-error" ? errors : name === "@/lib/zebra/locale" ? { zebraHref: url => `${url}?lang=${language}` } : name === "./Locale" ? { useZebraCopy: () => copy, useZebraLocale: () => language } : name === "next/link" ? { default: ({ children, ...props }) => React.createElement("a", props, children), __esModule: true } : require(name));
  for (const cause of [typed, legacy, new Error('raw JSON {"credential":"hidden-sensitive-key"}')]) {
    const html = renderToStaticMarkup(React.createElement(view.ExploreError, { error: cause, onRetry: () => {} }));
    assert(!html.includes("hidden-sensitive-key") && !html.includes("sensitive internal diagnostic") && !html.includes("credential"), "Raw provider diagnostics never render");
    assert(!html.includes(copy.offline), "A model or unknown search error does not claim that the atlas is down");
    if (errors.classifyExploreError(cause).kind === "quota") { assert(html.includes(copy.exploreError.quotaTitle)); assert(html.includes("/zebra/account")); assert(!html.includes(copy.retry), "Quota has no automatic or repeated model request affordance"); }
  }
  const candidates = [{ id: "MONDO:0004975", label: "Alzheimer disease", kind: "disease", match: "fuzzy", score: 0.8 }];
  let selected;
  const props = { error: { code: "unsupported_question", message: "private scope diagnostic" }, correctedQuery: "Alzheimer", matches: candidates, onSelectMatch: match => { selected = match; } };
  const html = renderToStaticMarkup(React.createElement(view.ExploreError, props));
  assert(html.includes("Alzheimer") && html.includes(copy.exploreError.possibleMatch));
  assert(html.includes(copy.exploreError.unsupportedBody) && !html.includes("private scope diagnostic"));
  function clickCandidate(node) {
    if (!node || typeof node !== "object") return;
    if (node.type === "button" && String(node.props.children?.[0]) === candidates[0].label) node.props.onClick();
    React.Children.forEach(node.props?.children, clickCandidate);
  }
  clickCandidate(view.ExploreError(props));
  assert.deepEqual(selected, candidates[0], "Selection returns only the actual indexed identifier");
}
const response = { query: "example", mode: "indexed_name_lookup", model_calls: 0, engine: "indexed-atlas", matches: [{ id: "HGNC:123", label: "Example gene", kind: "gene", match: "exact", score: 1 }] };
assert.deepEqual(lookup.normalizeIndexedLookup(response), response);
const nativeMatch = { ...response.matches[0] }; delete nativeMatch.score;
assert.deepEqual(lookup.normalizeIndexedLookup({ ...response, corrected_query: "Alzheimer", matches: [nativeMatch] }).matches, [nativeMatch], "Actual backend records without a score remain selectable");
assert.equal(lookup.normalizeIndexedLookup({ ...response, corrected_query: "Alzheimer" }).corrected_query, "Alzheimer");
assert.throws(() => lookup.normalizeIndexedLookup({ ...response, corrected_query: 42 }));
assert.throws(() => lookup.normalizeIndexedLookup({ ...response, model_calls: 1 }));
assert.throws(() => lookup.normalizeIndexedLookup({ ...response, matches: [...response.matches, ...response.matches] }));
assert.throws(() => lookup.normalizeIndexedLookup({ ...response, matches: [{ ...response.matches[0], id: "" }] }));
const originalFetch = globalThis.fetch;
let requested;
globalThis.fetch = async (url, init) => { requested = { url, init }; return new Response(JSON.stringify(response), { status: 200, headers: { "content-type": "application/json" } }); };
try {
  assert.deepEqual(await lookup.lookupIndexedNames("example"), response);
  assert.equal(requested.url, "/zebra/api/lookup");
  assert.deepEqual(JSON.parse(requested.init.body), { query: "example", limit: 10 });
  assert.equal(requested.init.method, "POST");
  await assert.rejects(() => lookup.lookupIndexedNames("x".repeat(513)));
} finally { globalThis.fetch = originalFetch; }
const route = read("app/zebra/api/[operation]/route.ts");
assert(route.includes('"/api/explore/lookup", { method: "POST", body: { query, limit }, auth: false'));
console.log("Zebra provider-fault privacy, localized quota UI and model-free lookup checks passed.");
