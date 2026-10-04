/** Measured label boxes use WORLD coordinates with x/y at the box center.
 * Measure and rank once per scene; this module never reads DOM or measures text.
 */
export type GraphLabelCandidate = Readonly<{
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  focus?: boolean;
  seed?: boolean;
  /** Explicit patient-group type supplied by the graph, only emphasized in community mode. */
  community?: boolean;
  distance?: number;
  degree?: number;
  sharedSeeds?: number;
}>;
export type GraphLabelView = Readonly<{ scale: number; x: number; y: number }>;
export type GraphLabelViewport = Readonly<{ width: number; height: number }>;
export type ScreenLabelBox = Readonly<{
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}>;
export type VisibleGraphLabel = ScreenLabelBox & Readonly<{
  id: string;
  active: boolean;
  /** The full measured box cannot fit the viewport or avoid reserved UI. */
  needsReadout: boolean;
}>;
export type GraphLabelOptions = Readonly<{
  activeId?: string | null;
  hoveredId?: string | null;
  focusedId?: string | null;
  /** Full/wrapped text geometry, measured on hover/focus rather than each frame. */
  activeCandidate?: GraphLabelCandidate | null;
  obstacles?: readonly ScreenLabelBox[];
  maxLabels?: number;
  padding?: number;
}>;
export type GraphLabelLayout = Readonly<{
  visible: Map<string, VisibleGraphLabel>;
  hidden: Set<string>;
  collisionChecks: number;
}>;

export const GRAPH_LABEL_FONT_PX = 13;
const CELL_SIZE = 64;
const MAX_CELLS_PER_BOX = 256;

function nonnegative(value: number | undefined) {
  return value !== undefined && Number.isFinite(value) ? Math.max(0, value) : 0;
}
function distance(value: number | undefined) {
  return value !== undefined && Number.isFinite(value) ? Math.max(0, value) : Infinity;
}
function valid(candidate: GraphLabelCandidate) {
  return Boolean(candidate.id) && [candidate.x, candidate.y, candidate.width, candidate.height].every(Number.isFinite)
    && candidate.width > 0 && candidate.height > 0;
}
function comparePriority(a: GraphLabelCandidate, b: GraphLabelCandidate) {
  const delta = Number(Boolean(b.focus)) - Number(Boolean(a.focus))
    || Number(Boolean(b.seed)) - Number(Boolean(a.seed))
    || Number(Boolean(b.community)) - Number(Boolean(a.community))
    || Number(nonnegative(b.sharedSeeds) >= 2) - Number(nonnegative(a.sharedSeeds) >= 2)
    || distance(a.distance) - distance(b.distance)
    || nonnegative(b.sharedSeeds) - nonnegative(a.sharedSeeds)
    || nonnegative(b.degree) - nonnegative(a.degree);
  // Avoid locale-dependent ordering and make shuffled equal-priority inputs stable.
  return delta || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** Focus, explicit seeds, actual multi-seed bridges, hop distance, connectivity.
 * Sorting happens once per scene. No input node/position is modified.
 */
export function rankGraphLabels(candidates: readonly GraphLabelCandidate[]): GraphLabelCandidate[] {
  const seen = new Set<string>();
  return candidates.filter(valid).slice().sort(comparePriority).filter(candidate => {
    if (seen.has(candidate.id)) return false;
    seen.add(candidate.id);
    return true;
  });
}

/** Canvas uses this exact font size; DOM label layers use the same view scale.
 * No independent font clamp or device-pixel ratio enters semantic font sizing.
 */
export function graphLabelScale(scale: number, basePx = GRAPH_LABEL_FONT_PX): number {
  const sharedScale = Number.isFinite(scale) && scale > 0 ? scale : 1;
  return (Number.isFinite(basePx) && basePx > 0 ? basePx : GRAPH_LABEL_FONT_PX) * sharedScale;
}

export function projectGraphLabel(candidate: GraphLabelCandidate, view: GraphLabelView): ScreenLabelBox {
  const width = candidate.width * view.scale, height = candidate.height * view.scale;
  const left = candidate.x * view.scale + view.x - width / 2;
  const top = candidate.y * view.scale + view.y - height / 2;
  return { left, top, right: left + width, bottom: top + height, width, height };
}

function intersects(a: ScreenLabelBox, b: ScreenLabelBox, padding: number) {
  return a.left < b.right + padding && a.right > b.left - padding
    && a.top < b.bottom + padding && a.bottom > b.top - padding;
}
function moved(box: ScreenLabelBox, left: number, top: number): ScreenLabelBox {
  return { ...box, left, top, right: left + box.width, bottom: top + box.height };
}
function inside(box: ScreenLabelBox, viewport: GraphLabelViewport, padding: number) {
  return box.left >= padding && box.top >= padding
    && box.right <= viewport.width - padding && box.bottom <= viewport.height - padding;
}
function clampBox(box: ScreenLabelBox, viewport: GraphLabelViewport, padding: number) {
  return moved(box,
    Math.max(padding, Math.min(viewport.width - padding - box.width, box.left)),
    Math.max(padding, Math.min(viewport.height - padding - box.height, box.top)));
}

/** Small screen-space index. Giant obstacles use a bounded fallback list rather
 * than allocating thousands of grid cells. Only accepted names are indexed.
 */
class CollisionIndex {
  private cells = new Map<string, ScreenLabelBox[]>();
  private large: ScreenLabelBox[] = [];
  private all: ScreenLabelBox[] = [];
  private padding: number;
  checks = 0;
  constructor(padding: number) { this.padding = padding; }
  private keys(box: ScreenLabelBox): string[] | null {
    const minX = Math.floor((box.left - this.padding) / CELL_SIZE), maxX = Math.floor((box.right + this.padding) / CELL_SIZE);
    const minY = Math.floor((box.top - this.padding) / CELL_SIZE), maxY = Math.floor((box.bottom + this.padding) / CELL_SIZE);
    if ((maxX - minX + 1) * (maxY - minY + 1) > MAX_CELLS_PER_BOX) return null;
    const result: string[] = [];
    for (let x = minX; x <= maxX; x++) for (let y = minY; y <= maxY; y++) result.push(`${x}:${y}`);
    return result;
  }
  add(box: ScreenLabelBox) {
    this.all.push(box);
    const keys = this.keys(box);
    if (!keys) { this.large.push(box); return; }
    for (const key of keys) {
      const occupants = this.cells.get(key) ?? [];
      occupants.push(box); this.cells.set(key, occupants);
    }
  }
  overlaps(box: ScreenLabelBox) {
    const keys = this.keys(box);
    const candidates = keys ? new Set([...this.large, ...keys.flatMap(key => this.cells.get(key) ?? [])]) : this.all;
    for (const other of candidates) {
      this.checks++;
      if (intersects(box, other, this.padding)) return true;
    }
    return false;
  }
}

/** Ordinary labels stay at the real graph coordinates. An active full name may
 * move a short fixed distance to avoid controls/strong foreground boxes; it is
 * never dropped. Oversized/fully blocked text requests an accessible readout.
 */
function activeBox(box: ScreenLabelBox, viewport: GraphLabelViewport, padding: number, index: CollisionIndex, obstacles: readonly ScreenLabelBox[]) {
  const gap = padding + 8;
  const offsets = [[0, 0], [0, -box.height - gap], [0, box.height + gap], [box.width + gap, 0], [-box.width - gap, 0]];
  for (const [x, y] of offsets) {
    const placed = clampBox(moved(box, box.left + x, box.top + y), viewport, padding);
    if (inside(placed, viewport, padding) && !index.overlaps(placed)) return { placed, needsReadout: false };
  }
  // A large foreground/control box can be taller than the active text. Align
  // beside its actual screen boundary instead of moving by just the text height.
  for (const obstacle of obstacles.slice(0, 12)) {
    const adjacent = [
      moved(box, box.left, obstacle.top - padding - box.height),
      moved(box, box.left, obstacle.bottom + padding),
      moved(box, obstacle.left - padding - box.width, box.top),
      moved(box, obstacle.right + padding, box.top),
    ];
    for (const candidate of adjacent) {
      const placed = clampBox(candidate, viewport, padding);
      if (inside(placed, viewport, padding) && !index.overlaps(placed)) return { placed, needsReadout: false };
    }
  }
  return { placed: clampBox(box, viewport, padding), needsReadout: true };
}

/** O(N + local collision work) per viewport change after a one-time scene rank.
 * Return names only: hidden names must keep their node hit target and full
 * accessible name/list entry in the renderer, so hover/focus can reveal them.
 */
export function layoutGraphLabels(
  ranked: readonly GraphLabelCandidate[], view: GraphLabelView, viewport: GraphLabelViewport, options: GraphLabelOptions = {},
): GraphLabelLayout {
  const visible = new Map<string, VisibleGraphLabel>(), hidden = new Set(ranked.map(candidate => candidate.id));
  if (![view.scale, view.x, view.y, viewport.width, viewport.height].every(Number.isFinite)
    || view.scale <= 0 || viewport.width <= 0 || viewport.height <= 0) return { visible, hidden, collisionChecks: 0 };
  const padding = Number.isFinite(options.padding) ? Math.max(0, options.padding ?? 4) : 4;
  const maxLabels = Math.max(0, Math.min(128, Math.floor(Number.isFinite(options.maxLabels) ? options.maxLabels ?? 24 : 24)));
  const activeId = options.activeId ?? options.focusedId ?? options.hoveredId;
  const candidateOverride = options.activeCandidate;
  const active = candidateOverride && candidateOverride.id === activeId && valid(candidateOverride) ? candidateOverride : ranked.find(candidate => candidate.id === activeId);
  const index = new CollisionIndex(padding);
  for (const obstacle of options.obstacles ?? []) {
    if ([obstacle.left, obstacle.top, obstacle.right, obstacle.bottom, obstacle.width, obstacle.height].every(Number.isFinite)
      && obstacle.width > 0 && obstacle.height > 0) index.add(obstacle);
  }
  if (active && valid(active)) {
    const { placed, needsReadout } = activeBox(projectGraphLabel(active, view), viewport, padding, index, options.obstacles ?? []);
    visible.set(active.id, { ...placed, id: active.id, active: true, needsReadout });
    hidden.delete(active.id); index.add(placed);
  }
  for (const candidate of ranked) {
    if (candidate.id === active?.id || visible.size >= maxLabels || !valid(candidate)) continue;
    const box = projectGraphLabel(candidate, view);
    if (!inside(box, viewport, padding) || index.overlaps(box)) continue;
    visible.set(candidate.id, { ...box, id: candidate.id, active: false, needsReadout: false });
    hidden.delete(candidate.id); index.add(box);
  }
  return { visible, hidden, collisionChecks: index.checks };
}

export function visibleGraphLabels(
  ranked: readonly GraphLabelCandidate[], view: GraphLabelView, viewport: GraphLabelViewport, options: GraphLabelOptions = {},
): Map<string, VisibleGraphLabel> {
  return layoutGraphLabels(ranked, view, viewport, options).visible;
}
