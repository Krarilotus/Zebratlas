// Deterministic fake-clock lifecycle checks; no browser, server or model calls.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const compiled = ts.transpileModule(readFileSync(new URL("../lib/zebra/search-suggestions.ts", import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const api = {};
new Function("exports", compiled)(api);
const hit = (id, label, matched = label) => ({ node: { id, label, kind: "gene" }, matched });
const stxbp1 = { results: [hit("HGNC:11444", "STXBP1")] };
const scn2a = { results: [hit("HGNC:10588", "SCN2A")] };
assert.deepEqual(api.buildSuggestions("STXBP1", stxbp1).map((choice) => choice.query), ["STXBP1 AND ", "STXBP1 OR "]);
assert.deepEqual(api.buildSuggestions("STXBP1 SCN2A", scn2a, stxbp1).map((choice) => choice.query), ["STXBP1 AND SCN2A", "STXBP1 OR SCN2A"]);
assert.deepEqual(api.buildSuggestions("STXBP1 AND SCN2A", scn2a, stxbp1).map((choice) => choice.query), ["STXBP1 OR SCN2A"]);
assert.deepEqual(api.buildSuggestions("Unknown AND SCN2A", scn2a, { results: [] }), [], "syntax never assumes an unresolved left term");
assert.deepEqual(api.buildSuggestions("unresolved", { results: [] }), [], "no fabricated entities or operators");
const realEntities = { results: [hit("HGNC:11444", "STXBP1"), hit("HGNC:11444", "STXBP1"), hit("HGNC:11445", "STXBP2"), hit("bad", "bad\nlabel")] };
assert.deepEqual(api.buildSuggestions("Find studies for STX", realEntities).map((choice) => choice.query), ["Find studies for STXBP1", "Find studies for STXBP2"], "applying an entity keeps the original question prefix");
assert.equal(api.suggestionContext("STXBP1 AND "), null, "incomplete syntax does not guess the next entity");
assert.equal(api.suggestionContext('"STXBP1 AND SCN2A"'), null, "quoted literals are not rewritten");
assert.equal(api.suggestionContext("x".repeat(513)), null);

let now = 0; let timerId = 0;
const timers = new Map();
const timer = { set: (callback, delay) => { const id = ++timerId; timers.set(id, { callback, at: now + delay }); return id; }, clear: (id) => timers.delete(id) };
function advance(ms) {
  const end = now + ms;
  for (;;) {
    const due = [...timers].filter(([, value]) => value.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
    if (!due) break;
    timers.delete(due[0]); now = due[1].at; due[1].callback();
  }
  now = end;
}
const pending = [];
let visible = [];
const scheduler = api.createSuggestionScheduler({ timer, publish: (value) => { visible = value; }, lookup: (term, signal) => new Promise((resolve) => pending.push({ term, signal, resolve })) });
scheduler.edit("STXBP1");
advance(1999); assert.equal(pending.length, 0, "no lookup before two seconds idle");
advance(1); assert.equal(pending.length, 1);
pending[0].resolve(stxbp1); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
assert.equal(visible.length, 2);
scheduler.edit("SCN2A"); assert.deepEqual(visible, [], "typing immediately clears suggestions");
assert.equal(pending[0].signal.aborted, true);
advance(2000); assert.equal(pending.length, 2);
scheduler.cancel(); assert.equal(pending[1].signal.aborted, true, "blur/outside click abort the indexed lookup");
pending[1].resolve(scn2a); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
assert.deepEqual(visible, [], "stale results cannot reappear after blur");
scheduler.edit("STX", false); advance(4000); assert.equal(pending.length, 2, "IME composition/busy fields never schedule a lookup");
scheduler.edit("STX"); advance(1500); scheduler.edit("SCN"); advance(1999); assert.equal(pending.length, 2, "every keystroke restarts the idle clock");
advance(1); assert.equal(pending.length, 3); assert.equal(pending[2].term, "SCN");
scheduler.edit("STXBP1 SCN2A"); advance(2000); assert.equal(pending.length, 5, "at most two indexed terms are resolved for a combination");
assert.deepEqual(pending.slice(-2).map((item) => item.term), ["SCN2A", "STXBP1"]);
scheduler.dispose(); pending.at(-1).resolve(stxbp1); pending.at(-2).resolve(scn2a);
await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
assert.deepEqual(visible, [], "unmount prevents late publication");
const count = pending.length;
scheduler.edit("STXBP1"); advance(3000); assert.equal(pending.length, count, "disposed components remain inactive");
console.log("PASS: indexed entities, resolved AND/OR previews, two-second idle, typing/IME/blur/outside cancellation, stale responses and unmount (fake clock; no model calls)");
