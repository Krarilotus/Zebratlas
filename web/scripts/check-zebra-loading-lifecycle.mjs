// Controlled requests exercise the real callers without a browser or backend.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import ts from "typescript";

const require = createRequire(import.meta.url);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const projectModules = new Map();
function projectModule(name, from = root) {
  if (!name.startsWith("@/") && !name.startsWith(".")) return require(name);
  let file = name.startsWith("@/") ? path.join(root, name.slice(2)) : path.resolve(from, name);
  if (file.endsWith(".json")) return JSON.parse(fs.readFileSync(file, "utf8"));
  if (!path.extname(file)) file += ".ts";
  if (projectModules.has(file)) return projectModules.get(file);
  const value = {}; projectModules.set(file, value);
  const compiled = ts.transpileModule(fs.readFileSync(file, "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, esModuleInterop: true } }).outputText;
  new Function("exports", "require", compiled)(value, dependency => projectModule(dependency, path.dirname(file)));
  return value;
}
const localeHelpers = projectModule("@/lib/zebra/locale");
const realReact = require("react");
const copy = new Proxy({ examples: [], kinds: {} }, { get: (obj, name) => obj[name] ?? String(name) });
const locale = { useZebraCopy: () => copy, useZebraLocale: () => "en" };
const noop = () => null;
function ZebraLoader() { return null; }
const globals = {
  window: { document: { addEventListener() {}, removeEventListener() {} }, addEventListener() {}, removeEventListener() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
};
function harness(file, imports) {
  const slots = [], effects = new Map();
  let index = 0, mounted = true, lateWrites = 0;
  const hooks = { ...realReact,
    useState(initial) {
      const slot = index++;
      if (!(slot in slots)) slots[slot] = typeof initial === "function" ? initial() : initial;
      return [slots[slot], value => { if (!mounted) { lateWrites++; return; } slots[slot] = typeof value === "function" ? value(slots[slot]) : value; }];
    },
    useRef(initial) { const slot = index++; return slots[slot] ??= { current: initial }; },
    useId() { index++; return "fixture-id"; },
    useCallback(fn) { index++; return fn; },
    useMemo(fn) { index++; return fn(); },
    useEffect(fn, deps) {
      const slot = index++, old = effects.get(slot);
      if (!old || deps.some((dep, i) => !Object.is(dep, old.deps[i]))) effects.set(slot, { fn, deps, cleanup: old?.cleanup, pending: true });
    },
  };
  hooks.useLayoutEffect = hooks.useEffect;
  const source = fs.readFileSync(path.join(root, file), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText;
  const modules = { react: hooks, "./Locale": locale, "./Icon": { Icon: noop }, "./ZebraLoader": { ZebraLoader }, "@/lib/zebra/locale": localeHelpers, ...imports };
  const exports = new Function("exports", "require", ...Object.keys(globals), js + ";return exports;")({}, name => modules[name] ?? require(name), ...Object.values(globals));
  return {
    render(props) {
      index = 0;
      const tree = Object.values(exports).find(value => typeof value === "function")(props);
      for (const effect of effects.values()) if (effect.pending) { effect.cleanup?.(); effect.pending = false; effect.cleanup = effect.fn(); }
      return tree;
    },
    unmount() { mounted = false; for (const effect of effects.values()) effect.cleanup?.(); },
    lateWrites: () => lateWrites,
  };
}
function elements(tree) {
  if (!tree || typeof tree !== "object") return [];
  if (Array.isArray(tree)) return tree.flatMap(elements);
  return [tree, ...elements(tree.props?.children)];
}
const loaders = tree => elements(tree).filter(node => node.type === ZebraLoader).length;
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = async () => { for (let i = 0; i < 4; i++) await new Promise(resolve => setImmediate(resolve)); };
const requests = [];
const suggestions = { createSuggestionScheduler: () => ({ cancel() {}, dispose() {}, edit() {} }) };
function searchFixture() {
  return harness("components/zebra/SearchBox.tsx", {
    "@/lib/zebra/client": { extractDocument: (file, signal) => { const next = { file, signal, ...deferred() }; requests.push(next); return next.promise; }, lookupEntities: noop },
    "@/lib/zebra/search-suggestions": suggestions,
    "./SearchSuggestions.module.css": { default: {} },
  });
}
const props = { onSearch() {} };
function attach(tree, name) { elements(tree).find(node => node.type === "input" && node.props.type === "file").props.onChange({ target: { files: [{ name }] } }); }
let search = searchFixture(), tree = search.render(props);
assert.equal(loaders(tree), 0);
attach(tree, "first.txt"); tree = search.render(props);
assert.equal(loaders(tree), 1); assert.equal(elements(tree).find(node => node.type === "form").props["aria-busy"], true);
requests.at(-1).resolve({ name: "first.txt", text: "STXBP1", chars: 6 }); await flush();
tree = search.render(props); assert.equal(loaders(tree), 0, "successful extraction clears pending");
attach(tree, "invalid.pdf"); search.render(props); requests.at(-1).reject(new Error("Unsupported PDF")); await flush();
tree = search.render(props); assert.equal(loaders(tree), 0); assert(elements(tree).some(node => node.props?.role === "alert"), "failed extraction ends pending and retains an error");
attach(tree, "old.txt"); search.render(props); const old = requests.at(-1);
attach(tree, "new.txt"); search.render(props); const latest = requests.at(-1);
assert.equal(old.signal.aborted, true);
old.resolve({ name: "old.txt", text: "OLD" }); await flush();
tree = search.render(props); assert.equal(loaders(tree), 1, "stale completion cannot stop the newer request");
latest.resolve({ name: "new.txt", text: "NEW" }); await flush();
tree = search.render(props); assert.equal(loaders(tree), 0); assert(elements(tree).some(node => node.props?.value === "NEW"));
attach(tree, "reset.txt"); search.render(props); const reset = requests.at(-1);
search.render({ ...props, resetVersion: 1 }); await flush(); tree = search.render({ ...props, resetVersion: 1 });
assert.equal(reset.signal.aborted, true); assert.equal(loaders(tree), 0, "history/reset cancels extraction and pending state");
reset.resolve({ name: "reset.txt", text: "STALE" }); await flush(); assert.equal(loaders(search.render({ ...props, resetVersion: 1 })), 0);
attach(tree, "unmount.txt"); search.render({ ...props, resetVersion: 1 }); const abandoned = requests.at(-1);
search.unmount(); assert.equal(abandoned.signal.aborted, true); abandoned.resolve({ name: "unmount.txt", text: "LATE" }); await flush();
assert.equal(search.lateWrites(), 0, "unmounted SearchBox does not publish late extraction results");

function utilityFixture() {
  const account = deferred(), saved = deferred(); let signal;
  const children = Object.fromEntries(["Icon", "ModelSettings", "SavedDraft", "AccountProfile", "AboutData", "ContributionForm", "AccountAuth", "Brand"].map(name => [`./${name}`, { [name]: noop, BrandText: noop }]));
  const view = harness("components/zebra/Utility.tsx", {
    ...children, "./Privacy": { PrivacyInformation: noop, PrivacyNotice: noop, PrivacyRequestForm: noop }, "next/link": { default: noop },
    "@/lib/zebra/client": { account: received => { signal = received; return account.promise; }, savedItems: () => saved.promise },
    "@/lib/zebra/account-link": { consumeAccountLink: noop },
  });
  return { view, account, saved, signal: () => signal };
}
const signedIn = { state: "signed_in", account: { user: { id: "fixture-user" } } };
let utility = utilityFixture(); assert.equal(loaders(utility.view.render({ view: "saved" })), 1);
utility.account.resolve(signedIn); await flush();
assert.equal(loaders(utility.view.render({ view: "saved" })), 1, "account success does not end loading before saved items arrive");
utility.saved.resolve([]); await flush(); assert.equal(loaders(utility.view.render({ view: "saved" })), 0); utility.view.unmount();
utility = utilityFixture(); utility.view.render({ view: "saved" }); utility.account.resolve(signedIn); await flush();
utility.saved.reject(new Error("Saved service unavailable")); await flush(); tree = utility.view.render({ view: "saved" });
assert.equal(loaders(tree), 0); assert(elements(tree).some(node => node.props?.children === "accountUnavailable")); utility.view.unmount();
utility = utilityFixture(); utility.view.render({ view: "saved" }); utility.account.resolve(signedIn); await flush(); utility.view.unmount();
assert.equal(utility.signal().aborted, true); utility.saved.resolve([]); await flush(); assert.equal(utility.view.lateWrites(), 0);
console.log("PASS: real SearchBox/Utility delayed success, failure, reset, replaced request, stale response and unmount; no network/browser/model calls");
