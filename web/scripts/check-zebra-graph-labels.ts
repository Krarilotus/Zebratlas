import assert from "node:assert/strict";
import {
  graphLabelScale, layoutGraphLabels, projectGraphLabel, rankGraphLabels, visibleGraphLabels,
  type GraphLabelCandidate, type ScreenLabelBox,
} from "../components/zebra/graph-labels.ts";

const view = { scale: 1, x: 0, y: 0 }, viewport = { width: 1200, height: 800 };
const box = (id: string, extra: Partial<GraphLabelCandidate> = {}): GraphLabelCandidate => ({ id, x: 500, y: 350, width: 120, height: 20, ...extra });
const crowded = [
  box("unrelated-hub", { degree: 200, distance: 4 }),
  box("nearby", { distance: 1, degree: 2 }),
  box("shared-bridge", { sharedSeeds: 3, distance: 2, degree: 4 }),
  box("primary-seed", { seed: true }),
  box("selected-focus", { focus: true }),
];
const untouched = JSON.stringify(crowded);
const ranked = rankGraphLabels(crowded);
assert.deepEqual(ranked.map(candidate => candidate.id), ["selected-focus", "primary-seed", "shared-bridge", "nearby", "unrelated-hub"]);
assert.equal(JSON.stringify(crowded), untouched); // Never move nodes to solve label collisions.
assert.deepEqual([...visibleGraphLabels(ranked, view, viewport).keys()], ["selected-focus"]);
assert.deepEqual([...visibleGraphLabels(rankGraphLabels([...crowded].reverse()), view, viewport).keys()], ["selected-focus"]);

// A hidden lower-priority full name wins immediately on hover/focus; it blocks
// previously visible names without changing any actual graph positions.
const hovered = visibleGraphLabels(ranked, view, viewport, { hoveredId: "unrelated-hub" });
assert.deepEqual([...hovered.keys()], ["unrelated-hub"]);
assert.equal(hovered.get("unrelated-hub")?.active, true);
assert.deepEqual([...visibleGraphLabels(ranked, view, viewport, { focusedId: "nearby" }).keys()], ["nearby"]);
assert.deepEqual([...visibleGraphLabels(ranked, view, viewport, { activeId: "shared-bridge", hoveredId: "unrelated-hub", focusedId: "nearby" }).keys()], ["shared-bridge"]);

const clear = rankGraphLabels([box("left", { x: 250 }), box("right", { x: 410 })]);
assert.equal(visibleGraphLabels(clear, view, viewport).size, 2);
assert.equal(visibleGraphLabels(clear, { ...view, scale: 2 }, { width: 2000, height: 1200 }).size, 2);
const zoomedOut = visibleGraphLabels(clear, { scale: .6, x: 20, y: 10 }, viewport);
assert.equal(zoomedOut.get("left")?.width, 72);
assert.equal(zoomedOut.get("left")?.left, 134);
assert.equal(zoomedOut.get("left")?.top, 214);
for (const scale of [.6, 1, 1.25, 2.4]) {
  assert.equal(graphLabelScale(scale), 13 * scale);
  // All semantic fonts have exactly the same zoom ratio; font and label bounds
  // scale together instead of a fixed Canvas font diverging from DOM labels.
  assert.equal(projectGraphLabel(clear[0], { ...view, scale }).height, 20 * scale);
}
assert.equal(graphLabelScale(NaN), 13);

// Constant screen-space breathing room can reveal more names at higher zoom.
const nearby = rankGraphLabels([box("a", { x: 300, width: 80 }), box("b", { x: 385, width: 80 })]);
assert.equal(visibleGraphLabels(nearby, { ...view, scale: .6 }, viewport).size, 1);
assert.equal(visibleGraphLabels(nearby, { ...view, scale: 2 }, viewport).size, 2);

const obstacle: ScreenLabelBox = { left: 420, top: 310, right: 580, bottom: 390, width: 160, height: 80 };
assert.equal(visibleGraphLabels(ranked, view, viewport, { obstacles: [obstacle] }).size, 0);
const withActive = visibleGraphLabels(ranked, view, viewport, { hoveredId: "nearby", obstacles: [obstacle] });
assert.equal(withActive.has("nearby"), true);
const active = withActive.get("nearby")!;
assert.equal(active.needsReadout, false);
assert.ok(active.bottom <= obstacle.top - 4 || active.top >= obstacle.bottom + 4 || active.right <= obstacle.left - 4 || active.left >= obstacle.right + 4);

const mobile = { width: 390, height: 620 };
const atEdge = rankGraphLabels([box("edge", { x: 378, y: 40, width: 220, height: 48 })]);
assert.equal(visibleGraphLabels(atEdge, view, mobile).size, 0);
const edgeActive = visibleGraphLabels(atEdge, view, mobile, { hoveredId: "edge" }).get("edge")!;
assert.ok(edgeActive.left >= 4 && edgeActive.right <= mobile.width - 4);
assert.equal(edgeActive.needsReadout, false);
const wrapped = visibleGraphLabels(ranked, view, viewport, { hoveredId: "nearby", activeCandidate: box("nearby", { width: 220, height: 68 }) }).get("nearby")!;
assert.equal(wrapped.width, 220); assert.equal(wrapped.height, 68);
const oversized = visibleGraphLabels(atEdge, { ...view, scale: 2.4 }, mobile, { hoveredId: "edge" }).get("edge")!;
assert.equal(oversized.needsReadout, true); // Renderer keeps full accessible readout.
assert.equal(oversized.width, 528); // Never silently clamp font scale to fit.

assert.equal(visibleGraphLabels(ranked, { ...view, scale: NaN }, viewport).size, 0);
assert.equal(rankGraphLabels([box("bad", { width: Infinity })]).length, 0);
assert.equal(rankGraphLabels([box("duplicate"), box("duplicate", { focus: true })]).length, 1);

// Large synthetic population: scene sorting is separate, viewport collision
// comparisons stay far below an all-pairs pass, even with a giant obstacle.
const grid = rankGraphLabels(Array.from({ length: 5000 }, (_, i) => box(`node-${String(i).padStart(5, "0")}`, {
  x: 40 + (i % 100) * 90, y: 40 + Math.floor(i / 100) * 40, width: 56, height: 16,
  degree: i % 7, distance: i % 5,
})));
const pass = layoutGraphLabels(grid, view, { width: 10000, height: 2200 }, { maxLabels: 100 });
assert.equal(pass.visible.size, 100);
assert.ok(pass.collisionChecks < 5000, `Unexpected collision work: ${pass.collisionChecks}`);
const giant = { left: -100000, top: -100000, right: 100000, bottom: 100000, width: 200000, height: 200000 };
const blocked = layoutGraphLabels(grid, view, viewport, { obstacles: [giant] });
assert.equal(blocked.visible.size, 0);
assert.ok(blocked.collisionChecks <= grid.length);

console.log("Zebra graph label priority, hover, zoom and bounded collision checks passed.");
