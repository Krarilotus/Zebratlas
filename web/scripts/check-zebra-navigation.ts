import assert from "node:assert/strict";
import { acceptNavigationResponse, decodeNavigation, heroProgress, infoNavigation, isCurrentInfo, restoreNavigation } from "../components/zebra/navigation.ts";
import { compactResultLabel } from "../components/zebra/result-label.ts";

const original = { query: "Which people and groups study SPG47?", results: [{ id: "MONDO:0012461" }, { id: "HGNC:18615" }], graph: { nodes: [{ id: "HGNC:18615" }] }, scroll: 480 };
const snapshots = new Map([["search-a", original]]);
const results = { search: "search-a", page: "results" as const };
const first = infoNavigation(results, "search-a", "MONDO:0012461", "overview");
assert.equal(first.method, "pushState");
assert.equal(first.view.returnsToResults, true);
const switched = infoNavigation(first.view, "search-a", "HGNC:18615", "evidence");
assert.equal(switched.method, "replaceState");
assert.equal(switched.view.returnsToResults, true);
assert.equal(isCurrentInfo(switched.view, "search-a", "HGNC:18615"), true);
assert.equal(isCurrentInfo(switched.view, "new-submission", "HGNC:18615"), false);
assert.equal(isCurrentInfo(switched.view, "search-a", "HGNC:11132"), false);
assert.equal(isCurrentInfo({ ...switched.view, context: "condition-a" }, "search-a", "HGNC:18615"), true);
assert.equal(isCurrentInfo({ ...switched.view, context: "condition-a" }, "search-a", "HGNC:18615", "condition-b"), false);
assert.equal(isCurrentInfo(results, "search-a", "HGNC:18615"), false);
const back = restoreNavigation({ zebra: results }, snapshots);
assert.equal(back?.view.page, "results");
assert.equal(back?.snapshot, original); // Same cached original result set, query, evidence and scroll.
assert.equal(restoreNavigation({ zebra: { search: "expired", page: "info" } }, snapshots), null);
assert.equal(decodeNavigation({ zebra: { search: "search-a", page: "other" } }), null);
const deep = infoNavigation({ search: "search-a", page: "info", node: "a" }, "search-a", "b", "overview");
assert.equal(deep.method, "replaceState");
assert.equal(deep.view.returnsToResults, undefined); // An external deep link creates no artificial Back trap.

const controller = new AbortController();
assert.equal(acceptNavigationResponse(controller.signal, "search-a", "search-a", "a", [{ id: "a" }]), true);
assert.equal(acceptNavigationResponse(controller.signal, "search-a", "new-submission", "a", [{ id: "a" }]), false);
assert.equal(acceptNavigationResponse(controller.signal, "search-a", "search-a", "a", [{ id: "wrong" }]), false);
controller.abort();
assert.equal(acceptNavigationResponse(controller.signal, "search-a", "search-a", "a", [{ id: "a" }]), false);
snapshots.clear();
assert.equal(restoreNavigation({ zebra: results }, snapshots), null); // New submit discards prior search caches.

for (const [expanded, compact] of [[860, 172], [620, 138], [380, 138]]) {
  const initial = heroProgress(expanded - compact, expanded, compact);
  assert.equal(initial.expanded, false);
  assert.equal(expanded / 2 + initial.offset - initial.distance, compact / 2); // Center stays visible while clipped.
  assert.equal(heroProgress(initial.distance + 1000, expanded, compact).offset, initial.offset);
  assert.equal(heroProgress(0, expanded, compact).expanded, true);
}
assert.equal(heroProgress(0, 0, 172).distance, 0);
assert.equal(compactResultLabel("  KIF1A-associated\nneurological disorder  "), "KIF1A-associated neurological disorder");
assert.equal(compactResultLabel("SCN2A"), "SCN2A");
const longTitle = "A natural history study of complex inherited neurological conditions across multiple international research communities";
const short = compactResultLabel(longTitle, 72);
assert.ok(Array.from(short).length <= 72);
assert.ok(short.endsWith("\u2026"));
assert.ok(longTitle.startsWith(short.slice(0, -1))); // Source wording stays intact; no generated paper/assay title.
assert.equal(compactResultLabel("\ud83e\uddec".repeat(100), 72).includes("\ufffd"), false);
console.log("Zebra navigation: cache/Back/replace/cancellation and scroll geometry checks passed");
