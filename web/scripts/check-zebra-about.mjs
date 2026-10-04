import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import ts from "typescript";
import { fileURLToPath } from "node:url";
const root = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(root, "../lib/zebra/about-data.ts"), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
const api = new Function("exports", compiled + ";return exports;")({});

const absent = api.aboutMetadata(null);
assert.equal(absent.loaded.totalNodes, undefined);
assert.equal(absent.visible.totalNodes, undefined);
assert.equal(absent.sparqlConfigured, undefined);
assert.equal(absent.adapter.runtimeReasoning, undefined);
assert(absent.measures.every(row => row.total === undefined && row.missingHash === undefined), "Unmeasured provenance must not become zero failures");
const older = api.aboutMetadata({ atlas: { diseases: 20, genes: 5 }, classes: [{ kind: "person", count: 7 }], execution: { sparql_configured: true, store_snapshot_equivalence: "unverified" } });
assert.equal(older.classes.person, 7);
assert.equal(older.loaded.totalNodes, undefined, "Partial class counts must not become a global total");
assert.equal(older.equivalence, "unverified", "Configuration never verifies store equivalence");
const raw = { data: { scope: "loaded_snapshots", counts: { total_nodes: 20, connected_edges: 14, nodes_by_kind: { person: 10 }, edge_kinds: { observed: 8, inferred: 6, hypothesis: 0 } },
  serving_visible: { connected_nodes_by_kind: { person: 4, organisation: 2 }, connected_edges: 3, referenced_records: 7, referenced_sources: 2, edge_kinds: { observed: 2, inferred: 1 } },
  withheld: { connected_nodes: 14, connected_edges: 11, connected_records: 5 },
  provenance: { connected_records: { total: 12, missing_effective_url: 2, zero_sha256: 3, missing_effective_retrieved_at: 4 }, connected_sources: { total: 0, missing_url: 0, missing_sha256: 0, missing_retrieved_at: 0 } },
  sources: [{ name: "Example source", url: "https://example.org/source", retrieved_at: "2026-10-03T17:22:00Z", sha256: "a".repeat(64) }, { name: "Undated source", url: "javascript:alert(1)", retrieved_at: "not a date", sha256: "invalid" }],
}, execution: { sparql_configured: true, store_snapshot_equivalence: "unverified", dataset: { public_release: false, graph_snapshot_sha256: "b".repeat(64), rdf_sha256: "c".repeat(64), runtime_reasoning: false } } };
const meta = api.aboutMetadata(raw);
assert.equal(meta.loaded.totalNodes, 20);
assert.equal(meta.visible.totalNodes, 6);
assert.equal(meta.loaded.connectedEdges, 14);
assert.equal(meta.visible.connectedEdges, 3);
assert.equal(meta.withheld.edges, 11);
assert.equal(meta.loaded.evidence.hypothesis, 0, "A measured zero remains zero");
assert.deepEqual(meta.measures.find(row => row.key === "connected_records"), { key: "connected_records", total: 12, missingUrl: 2, missingHash: 3, missingDate: 4 }, "Missing-proof measures retain individual denominators; overlapping gaps are not a fabricated score");
assert.equal(meta.sources[0].retrievedAt, "2026-10-03T17:22:00Z");
assert.equal(meta.sources[1].retrievedAt, undefined, "No fabricated date fallback");
assert.equal(meta.sources[1].url, undefined);
assert.equal(meta.sources[1].sha256, undefined);
assert.equal(meta.adapter.runtimeReasoning, false);
assert.equal(meta.adapter.graphSha256, "b".repeat(64));
assert.equal(meta.equivalence, "unverified");
assert.equal(api.metadataCount("100"), undefined);
assert.equal(api.metadataCount(-1), undefined);
assert.equal(api.metadataPresent(10, 2), 8, "One independently measured field presence is total minus missing");
assert.equal(api.metadataPresent(10, undefined), undefined, "Missing metadata measurements cannot become complete coverage");
assert.equal(api.metadataPresent(undefined, 0), undefined);
assert.equal(api.metadataPresent(0, 0), 0, "Measured empty scope remains zero");
assert.equal(api.metadataPresent(2, 3), undefined, "Impossible count relationships remain unknown");
assert.equal(api.metadataPresent(10, -1), undefined);
const visible = api.aboutMetadata({ data: { serving_visible: { referenced_records: { total: 10, missing_effective_url: 2, zero_sha256: 3, missing_effective_retrieved_at: 4 } } } });
assert.equal(api.metadataPresent(visible.visible.recordsProof.total, visible.visible.recordsProof.missingUrl), 8);
assert.equal(api.metadataPresent(visible.visible.recordsProof.total, visible.visible.recordsProof.missingHash), 7);
assert.equal(visible.visible.recordsProof.missingDate, 4, "A new UI presentation never invents record or inherited dates");
assert.equal(api.NRESE_REPOSITORY_URL, "https://github.com/Krarilotus/OWL-RS");
const component = fs.readFileSync(path.join(root, "../components/zebra/AboutData.tsx"), "utf8");
assert(component.indexOf("words.dateGaps") > component.indexOf("<details onToggle"), "Detailed missing dates stay in progressive metadata disclosure");
assert.equal((component.match(/href=\{NRESE_REPOSITORY_URL\}/g) ?? []).length, 3, "Official engine link appears in overview, detailed store and schema-unavailable states");
assert.equal(api.metadataUrl("https://user:password@example.org/source"), undefined);
assert.equal(api.metadataUrl("data:text/plain,source"), undefined);
if (process.argv[2]) {
  const live = api.aboutMetadata(JSON.parse(fs.readFileSync(process.argv[2], "utf8")));
  assert(Object.keys(live.atlas).length > 0, "Actual schema contains clinical atlas counts");
  console.log(JSON.stringify({ actualSchema: true, reportedGraphNodes: live.loaded.totalNodes ?? null, reportedServingNodes: live.visible.totalNodes ?? null, repositoryStoreEquivalence: live.equivalence ?? null }));
}
console.log(JSON.stringify({ passed: true, unknownIsNotZero: true, loadedServingAndPrivateAdapterSeparated: true, noFabricatedDatesOrTrustScore: true }));
