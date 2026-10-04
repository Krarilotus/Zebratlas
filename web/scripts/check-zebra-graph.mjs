/* Geometry and bounded CPU checks; this deliberately does not claim browser FPS. */
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import ts from "typescript";
import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const requireFromHere = createRequire(import.meta.url);
const source = fs.readFileSync(path.join(__dirname, "../components/zebra/Graph.tsx"), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020, esModuleInterop: true } }).outputText;
const helperSource = fs.readFileSync(path.join(__dirname, "../components/zebra/graph-labels.ts"), "utf8");
const helper = {};
new Function("exports", ts.transpileModule(helperSource, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText)(helper);
const api = new Function("exports", "require", compiled + ";return {indexGraph,makeScene,makeEdgeRoutes,displayName,relationName,paint,wheelView,laneNormal,relationCaptions,seedMetrics,fairSeedNodes,contextHit,patientGroup,nodeKind,nodeColor,contextLabels};")({}, name => name === "./graph-labels" ? helper : name.endsWith(".css") || ["./ReasoningProof", "./Sources", "./ZebraLoader"].includes(name) ? {} : requireFromHere(name));

const initialView = { scale: 1, x: 0, y: 0 }, cursor = { x: 200, y: 100 };
assert(api.wheelView(initialView, -100, 0, cursor, 700).scale > 1, "Normal wheel up zooms in without Ctrl");
assert(api.wheelView(initialView, 100, 0, cursor, 700).scale < 1, "Normal wheel down zooms out");
assert.deepEqual(api.wheelView(initialView, 1, 1, cursor, 700), api.wheelView(initialView, 16, 0, cursor, 700), "Line-mode wheel is normalized");
const zoomed = api.wheelView(initialView, -100, 0, cursor, 700);
assert.equal((cursor.x - zoomed.x) / zoomed.scale, cursor.x, "Zoom keeps the cursor's world point fixed");
assert(source.includes('host.addEventListener("wheel", wheel, { passive: false })'), "Wheel applies to node/edge overlays as well as canvas");
assert(!source.includes('if (!event.ctrlKey && !event.metaKey) return'), "Ordinary wheel is enabled");
assert(source.includes('host.removeEventListener("wheel", wheel)'), "Wheel listener is removed on unmount");
const css = fs.readFileSync(path.join(__dirname, "../components/zebra/Graph.module.css"), "utf8");
assert(!css.includes("text-overflow: ellipsis"), "Properties have no ellipsis");
assert(css.includes("white-space: normal; overflow-wrap: anywhere"), "Expanded property names wrap");
assert(source.includes("routes.filter(route => foregroundIds.has(route.edge.source) && foregroundIds.has(route.edge.target))"), "Every foreground predicate remains accessible in the legend even when its text cannot fit without a collision");
assert(!/font-size: (?:10|12|16|17)px;[^}]*\}[^\n]*\.focusNode strong/.test(css));

const node = (id, kind, context = false) => ({ id, label: id.toUpperCase(), kind, matched: id === "condition", context });
const edge = (a, b, relation, context = false, kind = "observed") => ({ id: `${a}-${b}-${relation}`, source: a, target: b, relation, label: relation, context, kind, evidence: [] });
const fixture = { nodes: [node("condition", "disease"), node("gene", "gene"), node("gene2", "gene"), node("study", "study"), node("paper", "paper"), node("group", "patient_group"), node("alice", "person"), node("bob", "person"), node("lab", "organisation"), node("context", "asset", true), node("path", "pathway", true)], edges: [edge("condition", "gene", "has_associated_gene"), edge("condition", "gene2", "has_associated_gene"), edge("study", "condition", "studies_condition"), edge("paper", "condition", "about_condition"), edge("group", "condition", "supports_condition"), edge("alice", "gene", "studies_gene"), edge("bob", "gene", "studies_gene"), edge("alice", "lab", "member_of"), edge("gene", "context", "has_asset", true), edge("context", "path", "in_pathway", true)] };
const index = api.indexGraph(fixture);
const assertions = { nodes: [{ ...node("A", "gene"), matched: true }, node("B", "disease")], edges: [
  { ...edge("A", "B", "p"), id: "A-p-B", evidence: [{ source: "Study one", record: "record1", sha256: "hash1" }, { source: "Study two", record: "record2", sha256: "hash2" }] },
  { ...edge("A", "B", "q"), id: "A-q-B" },
  { ...edge("B", "A", "p"), id: "B-p-A" },
  { ...edge("A", "B", "p"), id: "independent-assertion", evidence: [{ source: "Study three", record: "record3" }] },
] };
const assertionIndex = api.indexGraph(assertions);
for (const [selected, mode, results] of [["A", "search"], [null, "search", ["B"]], [null, "community"]]) {
  const scene = api.makeScene(assertionIndex, selected, 0, 1080, 720, mode, undefined, results);
  assert.deepEqual(new Set(scene.edges.map(assertion => assertion.id)), new Set(assertions.edges.map(assertion => assertion.id)), "Focus, ranked overview and community must retain distinct predicates, reverse directions and independent assertion IDs");
  assert.deepEqual(scene.edges.find(assertion => assertion.id === "A-p-B").evidence, assertions.edges[0].evidence, "Canonical merged triples preserve each independent study record without synthesizing extra edges");
  const routes = api.makeEdgeRoutes(scene, 1080, 720);
  assert.equal(new Set(routes.map(route => route.lane)).size, assertions.edges.length, "Parallel and reverse assertions have distinct visual lanes");
  assert(routes.every(route => route.meaning.includes(`${route.edge.source} → ${route.short} → ${route.edge.target}`)), "Accessible relationship meanings preserve source, predicate and target direction");
  const byId = new Map(scene.nodes.map(n => [n.id,n]));
  const forward = api.laneNormal(byId.get("A"),byId.get("B"),assertions.edges[0]);
  const reverse = api.laneNormal(byId.get("B"),byId.get("A"),assertions.edges[2]);
  assert(Math.abs(forward.x-reverse.x)+Math.abs(forward.y-reverse.y)<1e-9,"Reverse edges use the same physical lane coordinate and retain their original arrow direction");
}
assert(!source.includes("pairs.has(pair)"),"Scene construction must not collapse assertions by endpoints");
assert(!source.includes("evidence.slice(0, 3)"),"Inspector must not silently discard source records");
const namespaced = [edge("A", "B", "https://ontology.example/a/p"), edge("A", "B", "https://ontology.example/b/p")];
const captions = api.relationCaptions(namespaced);
assert.notEqual(captions.get(namespaced[0].relation), captions.get(namespaced[1].relation), "Different namespaces with an identical local property name remain visibly distinct");
assert(captions.get(namespaced[0].relation).includes(namespaced[0].relation), "Ambiguous relation captions retain the exact predicate identifier");
const overview = api.makeScene(index, null, 0, 1080, 720, "search", undefined, ["study", "paper", "condition"]);
assert(overview.overview && !overview.rootId, "An unselected search must never implicitly focus the first indexed node");
assert.deepEqual(overview.foreground.filter(n => n.level === "anchor").map(n => n.id), ["study", "paper", "condition"], "Overview follows actual ranked result IDs, not the query gene");
assert(overview.nodes.some(n => n.id === "gene"), "The actual query gene remains in overview context");
const first = api.makeScene(index, "condition", 0, 1080, 720);
const memory = { points: new Map(first.nodes.map(n => [n.id, { x: n.x, y: n.y }])), width: 1080, height: 720, rootId: first.rootId };
const second = api.makeScene(index, "gene", 0, 1080, 720, "search", memory);
assert.equal(second.rootId, "gene");
assert.equal(second.foreground[0].x, 540);
assert.equal(second.foreground[0].y, 352);
assert(second.foreground.some(n => n.id === "condition"), "Navigation retains the real previous root path");
assert.notDeepEqual(first.clusters.map(c => c.key), second.clusters.map(c => c.key), "Different focus must change actual property/type clusters");
assert(second.foreground.some(n => n.id === "alice"), "Gene navigation reveals its actual research neighbor");
assert(second.nodes.find(n => n.id === "path").distance === 2, "Fade uses actual graph distance");
const remembered = first.nodes.find(n => n.id === "path"), oldRoot = first.nodes.find(n => n.id === "gene");
const stable = second.nodes.find(n => n.id === "path");
assert(Math.abs(stable.x - remembered.x - (540 - oldRoot.x)) < 0.001 && Math.abs(stable.y - remembered.y - (352 - oldRoot.y)) < 0.001, "Existing peripheral map translates without reshuffling");

const added = { nodes: [...fixture.nodes, node("newlab", "organisation"), node("newperson", "person")], edges: [...fixture.edges, edge("newlab", "gene", "studies_gene"), edge("newperson", "newlab", "member_of")] };
const expanded = api.makeScene(api.indexGraph(added), "gene", 0, 1080, 720, "search", memory);
assert(expanded.nodes.some(n => n.id === "newlab") && expanded.nodes.some(n => n.id === "newperson"), "Fetched actual neighbors expand the scene");
assert.deepEqual({ x: expanded.nodes.find(n => n.id === "path").x, y: expanded.nodes.find(n => n.id === "path").y }, { x: stable.x, y: stable.y }, "Expansion preserves the existing peripheral map");
assert(expanded.edges.every(e => added.edges.some(actual => actual.id === e.id)), "Every drawn relationship comes from the response");
assert.equal(api.relationName(edge("a", "b", "has_associated_gene")), "associated gene");
assert(api.displayName({ id: "urn:record:1", label: "CVCL:A3KE", kind: "asset" }).includes("CVCL:A3KE"));
assert.equal(api.displayName({ id: "urn:record:1", label: "urn:record:123", kind: "asset" }), null);
const priorityFixture = { nodes: [node("root", "gene"), ...Array.from({ length: 12 }, (_, i) => node(`asset${i}`, "asset", true)), node("exact", "disease")], edges: [...Array.from({ length: 12 }, (_, i) => edge("root", `asset${i}`, "has_asset", true)), edge("exact", "root", "has_associated_gene")] };
assert(api.makeScene(api.indexGraph(priorityFixture), "root", 0, 390, 650).foreground.some(n => n.id === "exact"), "Background context cannot displace exact scientific evidence");
assert(source.indexOf('if (id === selectedNodeId) return;') < source.indexOf('rememberMap();', source.indexOf('function select(id')), "Repeated focus returns before map changes, callbacks or camera updates");
assert(source.includes('onFocus={() => onHighlight(route.edge.id, "focus")}'), "Keyboard edge focus emphasizes the exact directed assertion");
assert(source.includes('nodeHighlightsRef.current.pointer ?? nodeHighlightsRef.current.focus') && source.includes('edgeHighlightsRef.current.pointer ?? edgeHighlightsRef.current.focus'), "Pointer leave restores persistent keyboard focus rather than clearing it");
assert(source.includes('if (isActive && box.needsReadout) continue'), "Oversized or blocked full labels use the dedicated readout instead of painting over reserved graph labels");
assert(source.includes('listedNodes.slice(currentListPage * 40'), "The accessible picker is bounded to forty actual IDs per page");

const seeds = ["SCN1A", "KCNQ2", "SNAP25"];
const community = { nodes: seeds.map(id => ({ ...node(id, "gene"), matched: true })), edges: [] };
seeds.forEach((seed, s) => {
  for (let i = 0; i < [58, 8, 5][s]; i++) {
    const id = `${seed}-person-${i}`;
    community.nodes.push({ ...node(id, "person"), label: i < 2 ? "Alex Smith" : `${seed} researcher ${i}` });
    community.edges.push(edge(seed, id, "studied_by"));
  }
});
community.nodes.push(node("shared-paper", "paper"), node("bridge-person", "person"), node("distant-lab", "organisation"));
community.edges.push(edge("SCN1A", "shared-paper", "reported_in"), edge("KCNQ2", "shared-paper", "reported_in"), edge("SNAP25", "shared-paper", "reported_in"), edge("shared-paper", "bridge-person", "authored_by"), edge("bridge-person", "distant-lab", "affiliated_with"));
const communityIndex = api.indexGraph(community), metrics = api.seedMetrics(communityIndex, seeds.map(id => communityIndex.byId.get(id)));
assert.equal(metrics.get("shared-paper").seedIds.length, 3, "Actual shared paths remain shared when backend ownership metadata omits them");
const fair = api.fairSeedNodes(communityIndex, seeds.map(id => communityIndex.byId.get(id)), metrics, new Set(seeds), 12);
for (const seed of seeds) assert(fair.some(n => n.id.startsWith(`${seed}-person`)), "Unequal high-degree seeds must not consume every context slot");
for (const graph of [community, ...(process.argv[2] ? [JSON.parse(fs.readFileSync(process.argv[2], "utf8")).graph] : [])]) {
  const indexed = api.indexGraph(graph);
  for (const [width, height] of [[390, 380], [390, 489], [390, 650], [720, 400], [1080, 720]]) {
    const scene = api.makeScene(indexed, null, 0, width, height, "community");
    assert.equal(scene.foreground.filter(n => n.level === "anchor").length, 3, "All three returned seeds remain named simultaneously on phone and desktop");
    assert(!scene.rootId, "Community overview never implicitly selects a person or one seed");
    assert(scene.edges.every(e => graph.edges.some(actual => actual.id === e.id)), "Fair overview never invents cluster relationships");
    for (const anchor of scene.foreground.filter(n => n.level === "anchor")) assert(scene.edges.some(e => e.source === anchor.id || e.target === anchor.id), "Every seed retains a real visible neighborhood");
    for (let i = 0; i < scene.foreground.length; i++) for (let j = i + 1; j < scene.foreground.length; j++) {
      const a = scene.foreground[i], b = scene.foreground[j];
      assert(Math.abs(a.x - b.x) >= (a.width + b.width) / 2 - 1 || Math.abs(a.y - b.y) >= 67, `Community foreground collision ${width}x${height}: ${a.id}/${b.id}`);
    }
    assert(scene.nodes.some(n => n.sharedSeeds >= 2), "Actual shared bridges receive label priority");
  }
}
assert.equal(communityIndex.nodes.filter(n => n.name === "Alex Smith").length, 6, "Equal person names never merge different IDs");
const patient = { ...node("patient-association", "organisation"), subkind: "patient_group", label: "Families association" };
const ordinary = { ...node("ordinary-institution", "organisation"), subkind: "institution", label: "Patient Research Institute" };
assert(api.patientGroup(patient));
assert(api.patientGroup(node("explicit-group", "patient_group")));
assert(!api.patientGroup(ordinary) && !api.patientGroup({ ...node("person", "person"), label: "Patient representative" }), "No membership is inferred from a person's name or an institution's label");
assert.equal(api.nodeKind(patient), "patient_group", "A genuine backend subtype remains visible as a localized type annotation");
assert.notEqual(api.nodeColor(patient, true), api.nodeColor(ordinary, true));
assert.equal(api.nodeColor(patient, false), api.nodeColor({ ...patient, subkind: undefined, kind: "patient_group" }, false), "Search mode retains the existing patient-group type palette");
const patientGraph = { nodes: [...community.nodes, patient, ordinary], edges: [...community.edges, edge("KCNQ2", patient.id, "has_patient_group"), edge("KCNQ2", ordinary.id, "studied_at")] };
const patientIndex = api.indexGraph(patientGraph), patientScene = api.makeScene(patientIndex, null, 0, 1080, 720, "community");
assert(patientScene.foreground.some(n => n.id === patient.id), "An actual patient group receives a named foreground position in its real seed neighborhood");
assert(patientScene.seedIds.length === 3, "Patient emphasis preserves fair representation of all actual seeds");
const patientLabelScene = { nodes: [{ ...patient, name: patient.label, level: "context", x: 300, y: 260, width: 10, distance: 2 }, { ...ordinary, name: ordinary.label, level: "context", x: 300, y: 260, width: 10, distance: 1, degree: 100 }], foreground: [], edges: [], communityMode: true, totalNeighbors: 2, page: 0, pages: 1 };
const measured = api.contextLabels(patientLabelScene, { font: "", measureText: text => ({ width: text.length * 7 }) }, "Inter");
assert.equal(measured.ranked[0].id, patient.id, "An explicit patient group gets label priority over a nearby institution when their names collide");
assert.deepEqual(helper.rankGraphLabels([{ id: "seed", x: 0, y: 0, width: 10, height: 10, seed: true }, { id: "patient", x: 0, y: 0, width: 10, height: 10, community: true }]).map(n => n.id), ["seed", "patient"], "Seed names remain protected above community emphasis");
const quietSearchScene = api.makeScene(patientIndex, "KCNQ2", 0, 1080, 720, "search");
assert.equal(quietSearchScene.communityMode, false, "Community emphasis remains scoped to community mode");
const luminance = hex => hex.slice(1).match(/../g).map(value => parseInt(value, 16) / 255).map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4).reduce((sum, value, i) => sum + value * [.2126,.7152,.0722][i], 0);
const contrast = (a,b) => (Math.max(luminance(a),luminance(b))+.05)/(Math.min(luminance(a),luminance(b))+.05);
assert(contrast(api.nodeColor(patient,true), "#f8fafb") >= 4.5 && contrast("#dbaabd", "#14242c") >= 4.5, "Patient-group names remain readable on the actual light and dark graph surfaces");
assert(css.includes("--g-community-ink:#85566c") && css.includes("--g-community-ink:#dbaabd"), "The patient-group legend and semantic DOM labels use the same type family as Canvas names");

const hoverScene = { nodes: [{ ...node("dot", "paper"), name: "Full genuine publication name", x: 300, y: 250, width: 10, level: "context", distance: 1, degree: 3 }], foreground: [], edges: [], totalNeighbors: 0, page: 0, pages: 1 };
const hoverPositions = new Map(hoverScene.nodes.map(n => [n.id, n]));
for (const scale of [0.6, 1, 2.4]) {
  const view = { scale, x: 10, y: 20 }, fonts = [], radii = [];
  const noop = () => {};
  const ctx = { setTransform: noop, clearRect: noop, beginPath: noop, arc: (x,y,r) => radii.push(r), fill: noop, fillRect: noop, fillText: text => fonts.push({ text, font: ctx.font }), measureText: text => ({ width: text.length * 7 }) };
  api.paint({ getContext: () => ctx }, hoverScene, [], hoverPositions, view, 1080, 720, 1, "dot");
  assert(fonts.some(row => row.font.startsWith(`${14 * scale}px Inter,`)), "Full hover names use the same font family and zoom multiplier as DOM node and property names");
  assert(radii.every(r => r === 2.7), "Hover improves labels while leaving dot radius unchanged");
  assert.equal(api.contextHit(hoverScene, hoverPositions, view, { x: 300 * scale + 10, y: 250 * scale + 20 }).id, "dot", "Unnamed collision-hidden dots retain a bounded hit target after zoom and pan");
}
const edgeScene = api.makeScene(assertionIndex, "A", 0, 1080, 720), edgeRoutes = api.makeEdgeRoutes(edgeScene, 1080, 720), strokes = [];
const noop = () => {};
const edgeContext = { setTransform: noop, clearRect: noop, beginPath: noop, moveTo: noop, bezierCurveTo: noop, setLineDash: noop, stroke: () => strokes.push({ width: edgeContext.lineWidth, glow: edgeContext.shadowBlur }), closePath: noop, fill: noop, lineTo: noop, arc: noop, ellipse: noop, fillText: noop };
const edgePositions = new Map(edgeScene.nodes.map(n => [n.id,n]));
edgeRoutes.forEach(r => edgePositions.set(`edge-label:${r.edge.id}`, r.point));
api.paint({ getContext: () => edgeContext }, edgeScene, edgeRoutes, edgePositions, initialView, 1080, 720, 1, null, "B-p-A");
assert.equal(strokes.filter(s => s.width === 2.5 && s.glow === 5).length, 1, "Only the exact reverse directed assertion glows on its property hover");

const graphs = [fixture];
graphs.push({ nodes: [node("longroot", "disease"), ...Array.from({ length: 54 }, (_, i) => node(`label${i}`, i % 2 ? "paper" : "gene", i > 9))], edges: Array.from({ length: 54 }, (_, i) => edge("longroot", `label${i}`, `has_documented_${i % 2 ? "publication_evidence_supporting_association" : "associated_gene_in_the_reference_snapshot"}`, i > 9)) });
if (process.argv[2]) { const response = JSON.parse(fs.readFileSync(process.argv[2], "utf8")); graphs.push(response.graph ?? response); }
const samples = [];
for (const graph of graphs) for (const [width, height] of [[390, 489], [390, 650], [720, 400], [720, 520], [1080, 720]]) {
  const indexed = api.indexGraph(graph), root = indexed.byId.has("longroot") ? "longroot" : indexed.nodes.find(n => n.kind === "gene")?.id || indexed.nodes[0].id;
  const start = performance.now(), scene = api.makeScene(indexed, root, 0, width, height), routes = api.makeEdgeRoutes(scene, width, height);
  const elapsed = performance.now() - start;
  const boxes = routes.filter(route => route.labeled).map(route => ({ left: route.point.x - route.width / 2, right: route.point.x + route.width / 2, top: route.point.y - route.height / 2, bottom: route.point.y + route.height / 2 }));
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) assert(boxes[i].right <= boxes[j].left || boxes[j].right <= boxes[i].left || boxes[i].bottom <= boxes[j].top || boxes[j].bottom <= boxes[i].top, "Expanded property labels cannot overlap");
  const sceneCaptions = api.relationCaptions(scene.edges);
  for (const route of routes) assert.equal(route.short, sceneCaptions.get(route.full), "Expanded property names are never truncated");
  assert(scene.foreground.length <= (width < 660 ? 5 : 11));
  assert(scene.nodes.filter(n => n.level === "context").length <= (width < 660 ? 100 : 250));
  assert(scene.edges.length <= (width < 660 ? 220 : 500));
  assert(scene.nodes.every(n => Number.isFinite(n.x) && Number.isFinite(n.y)));
  for (let i = 0; i < scene.foreground.length; i++) for (let j = i + 1; j < scene.foreground.length; j++) {
    const a = scene.foreground[i], b = scene.foreground[j];
    assert(Math.abs(a.x - b.x) >= (a.width + b.width) / 2 - 1 || Math.abs(a.y - b.y) >= (a.level === "focus" || b.level === "focus" ? 77 : 67), `Named nodes overlap: ${width}x${height} ${a.id}/${b.id} dx=${Math.abs(a.x-b.x)} dy=${Math.abs(a.y-b.y)} foreground=${JSON.stringify(scene.foreground.map(n=>({id:n.id,x:n.x,y:n.y})))}`);
  }
  const noop = () => {};
  const context = { setTransform: noop, clearRect: noop, beginPath: noop, moveTo: noop, bezierCurveTo: noop, setLineDash: noop, stroke: noop, closePath: noop, fill: noop, lineTo: noop, arc: noop, ellipse: noop, fillText: noop };
  const positions = new Map(scene.nodes.map(n => [n.id, n]));
  for (const route of routes) if (route.labeled) positions.set(`edge-label:${route.edge.id}`, route.point);
  const paintStart = performance.now();
  for (let i = 0; i < 30; i++) api.paint({ getContext: () => context }, scene, routes, positions, { scale: 1, x: 0, y: 0 }, width, height, 1, null);
  samples.push({ width, height, actualNodes: graph.nodes.length, foreground: scene.foreground.length, nodes: scene.nodes.length, inViewport: scene.nodes.filter(n=>n.x>0&&n.x<width&&n.y>0&&n.y<height).length, edges: scene.edges.length, propertyLabels: routes.filter(r => r.labeled).length, layoutMs: +elapsed.toFixed(2), jsPaintStubMs: +((performance.now() - paintStart) / 30).toFixed(3) });
}
console.log(JSON.stringify({ passed: true, actualNavigationAndExpansion: true, engine: "Cytoscape finite CoSE", samples }, null, 2));
