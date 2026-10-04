"use client";

import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import cytoscape from "cytoscape";
import type { GraphEdge as SearchGraphEdge, GraphCommunity } from "@/lib/zebra/types";
import ReasoningProof from "./ReasoningProof";
import Sources from "./Sources";
import { graphLabelScale, rankGraphLabels, visibleGraphLabels, type GraphLabelCandidate, type ScreenLabelBox } from "./graph-labels";
import { ZebraLoader } from "./ZebraLoader";
import styles from "./Graph.module.css";

export type GraphNode = { id: string; label: string; kind: string; subkind?: string; matched?: boolean; context?: boolean; anchor_id?: string; community_seed_ids?: string[] };
export type GraphEdge = SearchGraphEdge;
export type GraphData = { nodes: GraphNode[]; edges: GraphEdge[]; community?: GraphCommunity };
export type GraphLabels = {
  region: string; zoomIn: string; zoomOut: string; fit: string; selectNode: string; nodeList: string; connections: string;
  moreNodes?: string; back?: string; reset?: string; previousNeighbors?: string; nextNeighbors?: string;
  moreNeighbors?: string; noNeighbors?: string; loading?: string; kinds?: Record<string, string>;
  edgeProperties?: string; property?: string; close?: string; sources?: string; evidence?: string; noEdgeEvidence?: string; inferred?: string; hypothesis?: string;
};
export type GraphProps = {
  graph: GraphData; selectedNodeId?: string | null; resultNodeIds?: string[]; highlightedNodeIds?: string[]; highlightedEdgeIds?: string[];
  onSelectNode: (id: string) => void; onBack?: () => void; onClearSelection?: () => void;
  canGoBack?: boolean; navigating?: boolean; labels: GraphLabels; mode?: "search" | "community";
  onSelectEdge?: (edge: GraphEdge) => void;
};
type Point = { x: number; y: number };
type NamedNode = GraphNode & { name: string };
type PositionedNode = NamedNode & Point & { level: "focus" | "anchor" | "neighbor" | "context"; width: number; relation?: string; distance?: number; degree?: number; sharedSeeds?: number };
type IndexedGraph = { nodes: NamedNode[]; byId: Map<string, NamedNode>; incident: Map<string, GraphEdge[]>; degree: Map<string, number>; community?: GraphCommunity };
type Cluster = { key: string; kind: string; relation: string; ids: string[]; allIds: string[]; color: string };
type Scene = { nodes: PositionedNode[]; foreground: PositionedNode[]; edges: GraphEdge[]; rootId?: string; totalNeighbors: number; page: number; pages: number; overview?: boolean; omitted?: number; clusters?: Cluster[]; seedIds?: string[]; communityMode?: boolean };
type LayoutMemory = { points: Map<string, Point>; rootId?: string; width: number; height: number };
type View = { scale: number; x: number; y: number };
type Transition = { from: Map<string, Point>; fromView: View; toView: View; start: number; duration: number };
type EdgeRoute = { edge: GraphEdge; short: string; full: string; meaning: string; color: string; point: Point; lane: number; labeled: boolean; width: number; height: number };
const IDENTITY: View = { scale: 1, x: 0, y: 0 };
const MOTION_MS = 320;
const CONTEXT_LIMIT = 250;
const EDGE_LIMIT = 500;
const LABEL_FONT_PX = 13;
const COLORS: Record<string, string> = {
  gene: "#17675f", disease: "#527e96", phenotype: "#82788c", pathway: "#897a60",
  person: "#4c776c", researcher: "#4c776c", organisation: "#587c8c", organization: "#587c8c",
  paper: "#7e8f99", study: "#5b8591", trial: "#5b8591", patient_group: "#54887b", asset: "#8a846c",
};
const PATIENT_GROUP_COLOR = "#85566c";
function patientGroup(node: GraphNode) { return node.kind === "patient_group" || node.subkind === "patient_group"; }
function nodeKind(node: GraphNode) { return patientGroup(node) ? "patient_group" : node.kind; }
function nodeColor(node: GraphNode, communityMode = false) { return communityMode && patientGroup(node) ? PATIENT_GROUP_COLOR : COLORS[nodeKind(node)] ?? "#748a91"; }

/** Preserve real names. Scientific identifiers are honest fallbacks; record UUIDs and URIs are not names. */
function displayName(node: GraphNode, kinds?: Record<string, string>): string | null {
  const value = node.label?.trim() ?? "";
  const identifier = (value + " " + node.id).match(/\b(MONDO|HGNC|HP|ORPHA|OMIM|DOID|NCBIGene|PMID|CHEBI|GO|CVCL|MGI|RGD|FBal|FlyBase|UniProt|UBERON|EFO|CLO|CL|TAXON|OBI|PR|ENSEMBL)[_:]([A-Za-z0-9][A-Za-z0-9.-]*)\b/i);
  const directIdentifier = value.match(/^([A-Za-z][A-Za-z0-9-]{1,20}):([A-Za-z0-9][A-Za-z0-9.-]{1,50})$/);
  const doi = (value + " " + node.id).match(/(?:doi:|doi\.org\/)(10\.\d{4,9}\/[^\s]+)/i);
  const clinicalId = (value + " " + node.id).match(/\b(NCT\d{8}|ENSG\d{6,})\b/i);
  const isRecord = /^(?:urn:|https?:\/\/|_:[\w-]+|[\da-f]{20,}|[\da-f]{8}-[\da-f-]{27,})/i.test(value);
  const isIdentifier = Boolean(directIdentifier) || /^(?:[A-Za-z]+_\d+|NCT\d{8}|ENSG\d{6,})$/i.test(value);
  if (value && !isRecord && !isIdentifier) return value.slice(0, 250);
  const canonical = identifier ? `${identifier[1]}:${identifier[2]}` : clinicalId?.[1] ?? (directIdentifier && !isRecord ? `${directIdentifier[1]}:${directIdentifier[2]}` : undefined);
  if (canonical) return `${kinds?.[node.kind] ?? node.kind.replaceAll("_", " ")} ${canonical}`;
  if (doi) return `${kinds?.[node.kind] ?? node.kind.replaceAll("_", " ")} DOI ${doi[1]}`;
  return null;
}

/** Index the response once; focus changes never run a force simulation or rescan per frame. */
function indexGraph(graph: GraphData, kinds?: Record<string, string>): IndexedGraph {
  const byId = new Map<string, NamedNode>();
  for (const node of graph.nodes) {
    const name = displayName(node, kinds);
    if (name && !byId.has(node.id)) byId.set(node.id, { ...node, name });
  }
  const incident = new Map<string, GraphEdge[]>();
  const degree = new Map<string, number>();
  for (const edge of graph.edges) {
    if (!byId.has(edge.source) || !byId.has(edge.target) || edge.source === edge.target) continue;
    const left = incident.get(edge.source) ?? [];
    const right = incident.get(edge.target) ?? [];
    left.push(edge); right.push(edge);
    incident.set(edge.source, left); incident.set(edge.target, right);
    degree.set(edge.source, left.length); degree.set(edge.target, right.length);
  }
  const nodes = [...byId.values()].sort((a, b) => Number(b.matched) - Number(a.matched)
    || (degree.get(b.id) ?? 0) - (degree.get(a.id) ?? 0) || a.name.localeCompare(b.name));
  return { nodes, byId, incident, degree, community: graph.community };
}

function relationName(edge: GraphEdge): string | undefined {
  const value = (edge.relation || edge.label || "").trim();
  if (!value) return undefined;
  return value.replace(/^https?:\/\/.*[/#]/i, "").replace(/^(?:urn:.*:|[\w.-]+:)/i, "").replaceAll("_", " ").replace(/([a-z])([A-Z])/g, "$1 $2").replace(/^(?:has|is)\s+/i, "").toLowerCase();
}
function evidenceRank(edge: GraphEdge) { return edge.kind === "observed" ? 0 : edge.kind === "extracted" ? 1 : edge.kind === "inferred" ? 3 : edge.kind === "hypothesis" ? 4 : 2; }

/** Assertion identity is the backend ID. Endpoints, predicate and provenance must never be merged here. */
function visibleEdges(index: IndexedGraph, nodes: PositionedNode[], limit: number): GraphEdge[] {
  const visible = new Set(nodes.map(node => node.id)), seen = new Set<string>(), edges: GraphEdge[] = [];
  for (const node of nodes) for (const edge of index.incident.get(node.id) ?? []) {
    if (seen.has(edge.id) || !visible.has(edge.source) || !visible.has(edge.target) || edges.length >= limit) continue;
    seen.add(edge.id); edges.push(edge);
  }
  return edges;
}

function identityFraction(id: string) {
  let hash = 2166136261;
  for (let i = 0; i < id.length; i++) hash = Math.imul(hash ^ id.charCodeAt(i), 16777619);
  return (hash >>> 0) / 4294967296;
}
function clusterAngle(kind: string, edge?: GraphEdge, rootId?: string) {
  const directions: Record<string, number> = { gene: -1.57, disease: -2.55, phenotype: 2.45, pathway: -0.65, person: 3.14, researcher: 3.14, organisation: 0.1, organization: 0.1, paper: 0.8, study: -0.3, trial: -0.3, grant: 1.3, asset: 1.8, patient_group: 2.7 };
  return (directions[kind] ?? 0.4) + (identityFraction(edge?.relation ?? kind) - 0.5) * 0.5 + (edge?.target === rootId ? 0.18 : 0);
}
function clusterKey(node: NamedNode, edge: GraphEdge, rootId: string) { return `${edge.relation || edge.label}|${nodeKind(node)}|${edge.source === rootId ? "out" : "in"}`; }
function rememberedPoint(id: string, rootId: string | undefined, center: Point, width: number, height: number, memory?: LayoutMemory): Point | undefined {
  const point = memory?.points.get(id);
  if (!point || !memory || !memory.width || !memory.height) return undefined;
  const scaleX = width / memory.width, scaleY = height / memory.height;
  const previousRoot = memory.points.get(rootId ?? "");
  return { x: point.x * scaleX + (previousRoot ? center.x - previousRoot.x * scaleX : 0), y: point.y * scaleY + (previousRoot ? center.y - previousRoot.y * scaleY : 0) };
}

/** Existing map positions survive navigation; new context grows along actual graph paths. */
function contextLayout(index: IndexedGraph, candidates: NamedNode[], foreground: PositionedNode[], width: number, height: number, rootId?: string, memory?: LayoutMemory): PositionedNode[] {
  const center = { x: width / 2, y: height / 2 - 8 };
  const available = new Set([...candidates, ...foreground].map(node => node.id));
  const parents = new Map<string, { id: string; edge: GraphEdge; depth: number }>();
  const queue = foreground.map(node => node.id), discovered = new Set(queue);
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const parentId = queue[cursor], depth = parents.get(parentId)?.depth ?? 0;
    for (const edge of index.incident.get(parentId) ?? []) {
      const id = edge.source === parentId ? edge.target : edge.source;
      if (!available.has(id) || discovered.has(id)) continue;
      discovered.add(id); parents.set(id, { id: parentId, edge, depth: depth + 1 }); queue.push(id);
    }
  }
  const positions = new Map<string, Point>(foreground.map(node => [node.id, node]));
  // Breadth-first placement ensures children expand from a known real parent.
  const order = [...candidates].sort((a, b) => (parents.get(a.id)?.depth ?? 100) - (parents.get(b.id)?.depth ?? 100));
  const context: PositionedNode[] = [];
  for (const node of order) {
    let point = rememberedPoint(node.id, rootId, center, width, height, memory);
    if (!point) {
      const parent = parents.get(node.id), origin = parent ? positions.get(parent.id) : undefined;
      const jitter = identityFraction(node.id);
      if (parent && origin) {
        const angle = parent.id === rootId ? clusterAngle(node.kind, parent.edge, rootId) + (jitter - 0.5) * 1.1
          : Math.atan2(origin.y - center.y, origin.x - center.x) + (jitter - 0.5) * 2.3;
        const radius = parent.id === rootId ? Math.min(width, height) * (0.31 + jitter * 0.2) : 45 + jitter * 100;
        point = { x: origin.x + Math.cos(angle) * radius, y: origin.y + Math.sin(angle) * radius };
      } else {
        // An unconnected response entity is kept distinct; no relationship is fabricated.
        point = { x: 24 + jitter * (width - 48), y: height - 94 - identityFraction(node.id + ":y") * 36 };
      }
    }
    positions.set(node.id, point); context.push({ ...node, ...point, level: "context", width: 0 });
  }
  return context;
}

/** Pack named labels near their actual spring positions. The focus stays fixed; no predefined slots. */
function separateForeground(nodes: PositionedNode[], width: number, height: number, top = 98) {
  const fixed = nodes.filter(node => node.level === "focus");
  type Placement = { node: PositionedNode; point: Point };
  let beam: { placements: Placement[]; cost: number }[] = [{ placements: fixed.map(node => ({ node, point: node })), cost: 0 }];
  for (const node of nodes.filter(node => node.level !== "focus")) {
    const minX = node.width / 2 + 16, maxX = width - node.width / 2 - 16;
    const minY = top, maxY = height - 99;
    const desired = { x: Math.max(minX, Math.min(maxX, node.x)), y: Math.max(minY, Math.min(maxY, node.y)) };
    const candidates: Point[] = [desired];
    for (let y = minY; y <= maxY; y += 20) for (let x = minX; x <= maxX; x += 20) candidates.push({ x, y });
    const next: typeof beam = [];
    for (const option of beam) for (const point of candidates) {
      if (!option.placements.every(other => Math.abs(point.x - other.point.x) >= (node.width + other.node.width) / 2 + 12 || Math.abs(point.y - other.point.y) >= (other.node.level === "focus" ? 103 : 88))) continue;
      next.push({ placements: [...option.placements, { node, point }], cost: option.cost + (point.x - desired.x) ** 2 + (point.y - desired.y) ** 2 });
    }
    if (next.length) beam = next.sort((a, b) => a.cost - b.cost).slice(0, width < 660 ? 24 : 4);
    else for (const option of beam) option.placements.push({ node, point: desired });
  }
  for (const { node, point } of beam[0]?.placements ?? []) { node.x = point.x; node.y = point.y; }
}
/** CoSE uses real edges and seeded positions, finishes once, and is destroyed before drawing. */
function springNeighborhood(index: IndexedGraph, foreground: PositionedNode[], context: PositionedNode[], width: number, memory?: LayoutMemory) {
  const supporting = context.slice(0, width < 660 ? 16 : 32);
  const nodes = [...foreground, ...supporting], ids = new Set(nodes.map(node => node.id));
  const edges = new Map<string, GraphEdge>();
  for (const node of nodes) for (const edge of index.incident.get(node.id) ?? []) {
    if (ids.has(edge.source) && ids.has(edge.target)) edges.set(edge.id, edge);
  }
  if (nodes.length < 2 || !edges.size) return;
  const cy = cytoscape({ headless: true, styleEnabled: true, layout: { name: "preset" },
    elements: [
      ...nodes.map(node => ({ data: { id: node.id, width: node.width || 18, height: node.level === "focus" ? 104 : node.level === "context" ? 18 : 80 }, position: { x: node.x, y: node.y }, locked: node.level === "focus" || (node.level === "context" && Boolean(memory?.points.has(node.id))) })),
      ...[...edges.values()].map(edge => ({ data: { id: `layout:${edge.id}`, source: edge.source, target: edge.target, context: Boolean(edge.context) } })),
    ], style: [{ selector: "node", style: { width: "data(width)", height: "data(height)" } }],
  });
  try {
    cy.layout({ name: "cose", animate: false, randomize: false, fit: false, numIter: 90,
      initialTemp: 18, coolingFactor: 0.92, minTemp: 0.3, gravity: 0, nodeOverlap: 12,
      nodeRepulsion: 18000, idealEdgeLength: edge => edge.data("context") ? 120 : width < 660 ? 185 : 245, edgeElasticity: 180,
    }).run();
    for (const node of nodes) if (node.level !== "focus" && !(node.level === "context" && memory?.points.has(node.id))) {
      const point = cy.getElementById(node.id).position();
      if (Number.isFinite(point.x) && Number.isFinite(point.y)) { node.x = point.x; node.y = point.y; }
    }
  } finally { cy.destroy(); }
}

type SeedMetric = { distance: number; seedIds: string[] };
function seedMetrics(index: IndexedGraph, seeds: NamedNode[]): Map<string, SeedMetric> {
  const distances = new Map<string, Map<string, number>>();
  for (const seed of seeds) {
    const reached = new Map([[seed.id, 0]]), queue = [seed.id];
    for (let cursor = 0; cursor < queue.length; cursor++) for (const edge of index.incident.get(queue[cursor]) ?? []) {
      const other = edge.source === queue[cursor] ? edge.target : edge.source;
      if (reached.has(other)) continue;
      reached.set(other, (reached.get(queue[cursor]) ?? 0) + 1); queue.push(other);
    }
    distances.set(seed.id, reached);
  }
  const seedIds = new Set(seeds.map(seed => seed.id));
  const shared = new Map(index.community?.shared_nodes.map(node => [node.id, node.seed_ids]) ?? []);
  const metrics = new Map<string, SeedMetric>();
  for (const node of index.nodes) {
    const measured = seeds.map(seed => ({ id: seed.id, distance: distances.get(seed.id)?.get(node.id) ?? Infinity }));
    const distance = Math.min(...measured.map(item => item.distance));
    const declared = shared.get(node.id) ?? node.community_seed_ids;
    // Ownership metadata is a hint, not a complete shared-path count. Actual adjacency can prove more bridges.
    const owners = [...(declared?.filter(id => seedIds.has(id)) ?? []), ...measured.filter(item => item.distance <= 2 || item.distance === distance && Number.isFinite(distance)).map(item => item.id)];
    metrics.set(node.id, { distance: Number.isFinite(distance) ? distance : 5, seedIds: [...new Set(owners)] });
  }
  return metrics;
}
function compareSeedNodes(index: IndexedGraph, metrics: Map<string, SeedMetric>, a: NamedNode, b: NamedNode): number {
  const x = metrics.get(a.id)!, y = metrics.get(b.id)!;
  return Number(patientGroup(b)) - Number(patientGroup(a)) || Number(y.seedIds.length > 1) - Number(x.seedIds.length > 1) || x.distance - y.distance
    || (index.degree.get(b.id) ?? 0) - (index.degree.get(a.id) ?? 0) || a.id.localeCompare(b.id);
}
/** Fair response-local sampling: shared IDs occur once; each real seed gets a turn before another fills the view. */
function fairSeedNodes(index: IndexedGraph, seeds: NamedNode[], metrics: Map<string, SeedMetric>, excluded: Set<string>, limit: number): NamedNode[] {
  const picked: NamedNode[] = [], seen = new Set(excluded);
  const queues = seeds.map(seed => index.nodes.filter(node => !seen.has(node.id) && metrics.get(node.id)?.seedIds.includes(seed.id)).sort((a, b) => compareSeedNodes(index, metrics, a, b)));
  while (picked.length < limit) {
    let advanced = false;
    for (const queue of queues) {
      while (queue.length && seen.has(queue[0].id)) queue.shift();
      const next = queue.shift();
      if (next) { seen.add(next.id); picked.push(next); advanced = true; }
      if (picked.length === limit) break;
    }
    if (!advanced) break;
  }
  for (const node of index.nodes) if (!seen.has(node.id) && picked.length < limit) { picked.push(node); seen.add(node.id); }
  return picked;
}
function makeCommunityOverview(index: IndexedGraph, anchors: NamedNode[], page: number, width: number, height: number): Scene {
  const compact = width < 660, pages = Math.max(1, Math.ceil(anchors.length / 3)), current = Math.min(Math.max(page, 0), pages - 1);
  const seeds = anchors.slice(current * 3, (current + 1) * 3), metrics = seedMetrics(index, seeds);
  const top = compact ? 166 : 112, bottom = height - 99, middle = (top + bottom) / 2;
  const foreground: PositionedNode[] = seeds.map((seed, i) => ({ ...seed, level: "anchor", width: compact ? 128 : 172,
    x: compact ? width * (seeds.length === 1 ? 0.5 : i === 1 ? 0.75 : 0.25) : width * (i + 0.5) / seeds.length,
    y: compact ? i < 2 ? top : bottom : Math.max(top, height * 0.27) }));
  const included = new Set(foreground.map(node => node.id));
  const members = new Map<string, string>();
  const perSeed = compact ? height >= 450 ? 1 : 0 : Math.min(3, Math.max(1, Math.floor((height - 190) / 130)));
  for (let round = 0; round < perSeed; round++) for (const seed of seeds) {
    const reachable = index.nodes.filter(node => !included.has(node.id) && metrics.get(node.id)?.seedIds.includes(seed.id) && metrics.get(node.id)!.distance <= 3);
    const direct = new Set((index.incident.get(seed.id) ?? []).map(edge => edge.source === seed.id ? edge.target : edge.source));
    reachable.sort((a, b) => Number(patientGroup(b)) - Number(patientGroup(a)) || Number(direct.has(b.id)) - Number(direct.has(a.id)) || compareSeedNodes(index, metrics, a, b));
    const next = reachable[0];
    if (next) { included.add(next.id); members.set(next.id, seed.id); }
  }
  const centers = new Map(foreground.map(node => [node.id, node]));
  for (const [id, owner] of members) {
    const node = index.byId.get(id)!, metric = metrics.get(id)!, seed = centers.get(owner)!;
    const rootIndex = seeds.findIndex(root => root.id === owner);
    const related = metric.seedIds.map(seedId => centers.get(seedId)).filter((point): point is PositionedNode => Boolean(point));
    const siblings = [...members].filter(([, root]) => root === owner).map(([child]) => child), rank = siblings.indexOf(id);
    let x = compact ? rootIndex === 2 ? width * 0.75 : seed.x : seed.x + (rank === 0 ? 0 : rank === 1 ? -1 : 1) * Math.min(90, width / seeds.length * 0.26);
    let y = compact ? rootIndex === 2 ? bottom : middle : height * (rank === 0 ? 0.48 : 0.73);
    if (!compact && related.length > 1) { x = related.reduce((sum, point) => sum + point.x, 0) / related.length; y = Math.min(bottom, related.reduce((sum, point) => sum + point.y, 0) / related.length + 150); }
    foreground.push({ ...node, level: "neighbor", width: compact ? 136 : Math.min(146, width / seeds.length * 0.45), x, y });
  }
  separateForeground(foreground, width, height, top);
  const candidates = fairSeedNodes(index, seeds, metrics, included, compact ? 100 : CONTEXT_LIMIT);
  const context = contextLayout(index, candidates, foreground, width, height);
  const positions = new Map(foreground.map(node => [node.id, node]));
  for (const node of context) {
    const metric = metrics.get(node.id)!;
    const roots = metric.seedIds.map(id => centers.get(id)).filter((point): point is PositionedNode => Boolean(point));
    const actualParent = (index.incident.get(node.id) ?? []).map(edge => positions.get(edge.source === node.id ? edge.target : edge.source)).find(Boolean);
    const origin = roots.length > 1 ? { x: roots.reduce((sum, point) => sum + point.x, 0) / roots.length, y: roots.reduce((sum, point) => sum + point.y, 0) / roots.length + 90 } : actualParent ?? roots[0];
    if (origin) {
      const angle = identityFraction(node.id) * Math.PI * 2, radius = (compact ? 32 : 56) + Math.min(3, metric.distance) * (compact ? 24 : 38);
      node.x = Math.max(22, Math.min(width - 22, origin.x + Math.cos(angle) * radius));
      node.y = Math.max(136, Math.min(height - 58, origin.y + Math.sin(angle) * radius));
    } else { node.x = 22 + identityFraction(node.id) * (width - 44); node.y = height - 60; }
    positions.set(node.id, node);
  }
  const nodes = [...context, ...foreground];
  for (const node of nodes) { const metric = metrics.get(node.id)!; node.distance = metric.distance; node.sharedSeeds = metric.seedIds.length; node.degree = index.degree.get(node.id) ?? 0; }
  // Label processing receives priority order once, never a per-frame sort.
  context.sort((a, b) => compareSeedNodes(index, metrics, a, b));
  const edges = visibleEdges(index, [...foreground, ...context], compact ? 220 : EDGE_LIMIT);
  return { nodes: [...context, ...foreground], foreground, edges, seedIds: seeds.map(seed => seed.id), totalNeighbors: index.nodes.length, page: current, pages, overview: true, omitted: Math.max(0, index.nodes.length - nodes.length) };
}

function makeAnchoredOverview(index: IndexedGraph, anchors: NamedNode[], page: number, width: number, height: number, memory?: LayoutMemory): Scene {
  const compact = width < 660, clusterCount = compact ? 1 : 3;
  const contextLimit = compact ? 100 : CONTEXT_LIMIT, edgeLimit = compact ? 220 : EDGE_LIMIT;
  const pages = Math.max(1, Math.ceil(anchors.length / clusterCount)), current = Math.min(Math.max(0, page), pages - 1);
  const chosen = anchors.slice(current * clusterCount, (current + 1) * clusterCount);
  const foreground: PositionedNode[] = [];
  const included = new Set(chosen.map(node => node.id));
  chosen.forEach((anchor, group) => {
    const centerX = width * (group + 0.5) / chosen.length;
    foreground.push({ ...anchor, x: centerX, y: height * (compact ? 0.27 : 0.31), width: compact ? 184 : 172, level: "anchor" });
    const members: NamedNode[] = [], queued = new Set([anchor.id]);
    const queue = [anchor.id];
    for (let cursor = 0; cursor < queue.length && members.length < 3; cursor++) {
      const connected = [...(index.incident.get(queue[cursor]) ?? [])].sort((a, b) => {
        const score = (edge: GraphEdge) => {
          const other = edge.source === queue[cursor] ? edge.target : edge.source;
          const node = index.byId.get(other);
          return evidenceRank(edge) * 1000 - (["person", "researcher"].includes(node?.kind ?? "") ? 100 : node?.kind === "organisation" ? 60 : 0) - (index.degree.get(other) ?? 0);
        };
        return score(a) - score(b);
      });
      for (const edge of connected) {
        const id = edge.source === queue[cursor] ? edge.target : edge.source;
        if (queued.has(id) || included.has(id)) continue;
        queued.add(id); queue.push(id);
        const node = index.byId.get(id);
        if (node) { included.add(id); members.push(node); }
        // One real bridge first, then its researchers/institution through actual second-hop links.
        if (cursor === 0 && members.length === 1) break;
        if (members.length === 3) break;
      }
    }
    members.forEach((node, i) => {
      const row = i === 0 ? 0 : 1;
      foreground.push({ ...node, level: "neighbor", width: compact ? Math.min(154, (width - 54) / 2) : Math.min(146, width / chosen.length * 0.43),
        x: i === 0 ? centerX : centerX + (i === 1 ? -1 : 1) * Math.min(compact ? width * 0.25 : width / chosen.length * 0.26, 100),
        y: height * (row === 0 ? compact ? 0.48 : 0.52 : compact ? 0.72 : 0.75) });
    });
  });
  const context = contextLayout(index, index.nodes.filter(node => !included.has(node.id)).slice(0, contextLimit), foreground, width, height, undefined, memory);
  const visible = new Set([...foreground, ...context].map(node => node.id));
  const edges = visibleEdges(index, [...foreground, ...context], edgeLimit);
  return { nodes: [...context, ...foreground], foreground, edges, totalNeighbors: index.nodes.length, page: current, pages, overview: true, omitted: Math.max(0, index.nodes.length - visible.size) };
}

function makeOverview(index: IndexedGraph, page: number, width: number, height: number, memory?: LayoutMemory, resultNodeIds?: string[], community = false): Scene {
  const resultAnchors = [...new Set(resultNodeIds ?? [])].map(id => index.byId.get(id)).filter((node): node is NamedNode => Boolean(node));
  const declaredSeeds = community ? index.community?.seeds.map(seed => index.byId.get(seed.id)).filter((node): node is NamedNode => Boolean(node)) : undefined;
  const anchors = declaredSeeds?.length ? declaredSeeds : resultAnchors.length ? resultAnchors : index.nodes.filter(node => node.matched && ["gene", "disease"].includes(node.kind));
  if (community && anchors.length) return makeCommunityOverview(index, anchors, page, width, height);
  if (anchors.length) return makeAnchoredOverview(index, anchors, page, width, height, memory);
  const compact = width < 660;
  const contextLimit = compact ? 100 : CONTEXT_LIMIT, edgeLimit = compact ? 220 : EDGE_LIMIT;
  const size = compact ? 6 : 10;
  const components = new Map<string, number>();
  let component = 0;
  for (const node of index.nodes) {
    if (components.has(node.id)) continue;
    const queue = [node.id]; components.set(node.id, component);
    for (let cursor = 0; cursor < queue.length; cursor++) {
      for (const edge of index.incident.get(queue[cursor]) ?? []) {
        const other = edge.source === queue[cursor] ? edge.target : edge.source;
        if (!components.has(other)) { components.set(other, component); queue.push(other); }
      }
    }
    component++;
  }
  const ordered = [...index.nodes].sort((a, b) => (components.get(a.id) ?? 0) - (components.get(b.id) ?? 0));
  const pages = Math.max(1, Math.ceil(ordered.length / size));
  const current = Math.min(Math.max(page, 0), pages - 1);
  const chosen = ordered.slice(current * size, (current + 1) * size);
  const foreground: PositionedNode[] = chosen.map((node, i) => {
    const angle = -Math.PI / 2 + i / chosen.length * Math.PI * 2;
    const point = compact ? { x: width * (i % 2 === 0 ? 0.25 : 0.75), y: height * (0.25 + Math.floor(i / 2) * 0.25) - 8 }
      : { x: width / 2 + Math.cos(angle) * width * 0.34, y: height / 2 + Math.sin(angle) * height * 0.32 - 8 };
    return { ...node, ...point, level: "neighbor", width: compact ? Math.min(154, (width - 54) / 2) : Math.min(176, width * 0.23) };
  });
  const selected = new Set(chosen.map(node => node.id));
  const context = contextLayout(index, ordered.filter(node => !selected.has(node.id)).slice(0, contextLimit), foreground, width, height, undefined, memory);
  const visible = new Set([...foreground, ...context].map(node => node.id));
  const edges = visibleEdges(index, [...foreground, ...context], edgeLimit);
  return { nodes: [...context, ...foreground], foreground, edges, totalNeighbors: ordered.length, page: current, pages, overview: true, omitted: Math.max(0, ordered.length - visible.size) };
}

function makeScene(index: IndexedGraph, selected: string | null | undefined, page: number, width: number, height: number, mode = "search", memory?: LayoutMemory, resultNodeIds?: string[]): Scene {
  if (!selected) return { ...makeOverview(index, page, width, height, memory, mode === "search" ? resultNodeIds : undefined, mode === "community"), communityMode: mode === "community" };
  const root = (selected ? index.byId.get(selected) : undefined) ?? index.nodes[0];
  if (!root) return { nodes: [], foreground: [], edges: [], totalNeighbors: 0, page: 0, pages: 1 };
  const compact = width < 660;
  const contextLimit = compact ? 100 : CONTEXT_LIMIT, edgeLimit = compact ? 220 : EDGE_LIMIT;
  const pageSize = compact ? Math.min(4, Math.max(2, Math.floor((height - 190) / 100) * 2)) : Math.min(10, Math.max(4, Math.floor((height - 180) / 90) * 2));
  const neighbors = new Map<string, { node: NamedNode; edge: GraphEdge }>();
  for (const edge of index.incident.get(root.id) ?? []) {
    const id = edge.source === root.id ? edge.target : edge.source;
    const node = index.byId.get(id);
    const previous = neighbors.get(id);
    if (node && (!previous || Number(Boolean(edge.context)) * 10 + evidenceRank(edge) < Number(Boolean(previous.edge.context)) * 10 + evidenceRank(previous.edge))) neighbors.set(id, { node, edge });
  }
  const all = [...neighbors.values()].sort((a, b) => Number(Boolean(a.edge.context || a.node.context)) - Number(Boolean(b.edge.context || b.node.context)) || evidenceRank(a.edge) - evidenceRank(b.edge) || Number(b.node.matched) - Number(a.node.matched)
    || (mode === "community" ? Number(patientGroup(b.node)) - Number(patientGroup(a.node)) : 0) || (index.degree.get(b.node.id) ?? 0) - (index.degree.get(a.node.id) ?? 0) || a.node.name.localeCompare(b.node.name));
  const pages = Math.max(1, Math.ceil(all.length / pageSize));
  const currentPage = Math.min(Math.max(0, page), pages - 1);
  const chosen = all.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
  const center = { x: width / 2, y: height / 2 - 8 };
  const centerWidth = compact ? Math.min(204, width - 80) : Math.min(226, width * 0.3);
  const leafWidth = compact ? Math.min(146, (width - 54) / 2) : Math.min(178, width * 0.22);
  const foreground: PositionedNode[] = [{ ...root, ...center, level: "focus", width: centerWidth }];
  // Actual property, direction and entity kind determine the neighborhood's clusters.
  const groups = new Map<string, typeof chosen>();
  for (const item of chosen) {
    const key = clusterKey(item.node, item.edge, root.id);
    const group = groups.get(key) ?? []; group.push(item); groups.set(key, group);
  }
  const clusters: Cluster[] = [];
  for (const [key, members] of groups) {
    const first = members[0];
    const remembered = members.map(item => rememberedPoint(item.node.id, root.id, center, width, height, memory)).filter((point): point is Point => Boolean(point));
    // Follow the existing map when a cluster was already visible; a new cluster has a semantic bearing.
    const angle = remembered.length ? Math.atan2(remembered.reduce((sum, p) => sum + p.y - center.y, 0), remembered.reduce((sum, p) => sum + p.x - center.x, 0))
      : clusterAngle(nodeKind(first.node), first.edge, root.id);
    const spread = Math.min(1.8, 0.48 * Math.max(1, members.length - 1));
    const ordered = [...members].sort((a, b) => {
      const pa = rememberedPoint(a.node.id, root.id, center, width, height, memory), pb = rememberedPoint(b.node.id, root.id, center, width, height, memory);
      return pa && pb ? Math.atan2(pa.y - center.y, pa.x - center.x) - Math.atan2(pb.y - center.y, pb.x - center.x) : a.node.name.localeCompare(b.node.name);
    });
    ordered.forEach(({ node, edge }, i) => {
      const bearing = angle + (i - (ordered.length - 1) / 2) * spread / Math.max(1, ordered.length - 1);
      const prior = rememberedPoint(node.id, root.id, center, width, height, memory);
      const radiusX = Math.min(width * (compact ? 0.28 : 0.36), 410), radiusY = Math.min(height * 0.32, 255);
      const point = prior && Math.hypot(prior.x - center.x, prior.y - center.y) > 70 ? prior
        : { x: center.x + Math.cos(bearing) * radiusX, y: center.y + Math.sin(bearing) * radiusY };
      foreground.push({ ...node, ...point, level: "neighbor", width: leafWidth, relation: relationName(edge) });
    });
    clusters.push({ key, kind: nodeKind(first.node), relation: first.edge.relation || first.edge.label || "", ids: members.map(item => item.node.id), allIds: all.filter(item => clusterKey(item.node, item.edge, root.id) === key).map(item => item.node.id), color: relationColor(first.edge) });
  }
  separateForeground(foreground, width, height);
  const foregroundIds = new Set(foreground.map(node => node.id));
  const contextCandidates: NamedNode[] = [];
  const contextIds = new Set(foregroundIds);
  for (const node of [...all.map(item => item.node), ...index.nodes]) {
    if (contextIds.has(node.id)) continue;
    contextIds.add(node.id); contextCandidates.push(node);
    if (contextCandidates.length === contextLimit) break;
  }
  const context = contextLayout(index, contextCandidates, foreground, width, height, root.id, memory);
  springNeighborhood(index, foreground, context, width, memory);
  separateForeground(foreground, width, height);
  // New peripheral positions remain in view. Previously explored positions retain their map coordinates.
  for (const node of context) if (!memory?.points.has(node.id)) {
    const radiusX = width / 2 - 22, radiusY = height / 2 - 86;
    node.x = center.x + Math.tanh((node.x - center.x) / radiusX) * radiusX;
    node.y = center.y + Math.tanh((node.y - center.y) / radiusY) * radiusY;
  }
  const distances = new Map([[root.id, 0]]), distanceQueue = [root.id];
  for (let cursor = 0; cursor < distanceQueue.length; cursor++) for (const edge of index.incident.get(distanceQueue[cursor]) ?? []) {
    const id = edge.source === distanceQueue[cursor] ? edge.target : edge.source;
    if (!distances.has(id)) { distances.set(id, (distances.get(distanceQueue[cursor]) ?? 0) + 1); distanceQueue.push(id); }
  }
  const nodes = [...context, ...foreground];
  for (const node of nodes) { node.distance = distances.get(node.id) ?? 5; node.degree = index.degree.get(node.id) ?? 0; node.sharedSeeds = node.community_seed_ids?.length ?? 0; }
  const visibleIds = new Set(nodes.map(node => node.id));
  // First preserve actual direct links, then add a small quiet context around them.
  const edges = visibleEdges(index, [...foreground, ...context], edgeLimit);
  return { nodes, foreground, edges, rootId: root.id, totalNeighbors: all.length, page: currentPage, pages, omitted: Math.max(0, index.nodes.length - visibleIds.size), clusters, communityMode: mode === "community" };
}

function relationColor(edge: GraphEdge): string {
  const property = (edge.relation || edge.label || "").toLowerCase();
  if (/gene|allele|variant/.test(property)) return "#357d70";
  if (/phenotype|symptom|condition|disease/.test(property)) return "#6785a0";
  if (/pathway|mechanism|function|process/.test(property)) return "#957651";
  if (/study|trial|registry/.test(property)) return "#477f91";
  if (/paper|publication|author/.test(property)) return "#83759d";
  if (/person|member|community|organisation|organization/.test(property)) return "#68876e";
  return "#788b96";
}
function labelKey(id: string) { return `edge-label:${id}`; }
function relationCaptions(edges: GraphEdge[]): Map<string, string> {
  const meanings = new Map<string, Set<string>>(), captions = new Map<string, string>();
  for (const edge of edges) {
    const full = edge.relation || edge.label || "", name = relationName(edge) || full;
    const properties = meanings.get(name) ?? new Set<string>(); properties.add(full); meanings.set(name, properties);
  }
  for (const edge of edges) {
    const full = edge.relation || edge.label || "", name = relationName(edge) || full;
    // Distinct namespace properties can share a human label. Show the actual identifier when ambiguous.
    captions.set(full, (meanings.get(name)?.size ?? 0) > 1 ? `${name} · ${full}` : name);
  }
  return captions;
}
/** Endpoint grouping assigns visual lanes only; it never merges assertions. */
function parallelLanes(edges: GraphEdge[]): Map<string, number> {
  const groups = new Map<string, GraphEdge[]>(), lanes = new Map<string, number>();
  for (const edge of edges) {
    const key = JSON.stringify([edge.source, edge.target].sort());
    const group = groups.get(key) ?? []; group.push(edge); groups.set(key, group);
  }
  for (const group of groups.values()) {
    const ordered = [...group].sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
    ordered.forEach((edge, index) => lanes.set(edge.id, 160 * Math.tanh((index - (group.length - 1) / 2) * 32 / 160)));
  }
  return lanes;
}
function laneNormal(a: Point, b: Point, edge: GraphEdge): Point {
  const distance = Math.max(1, Math.hypot(b.x - a.x, b.y - a.y));
  const sign = edge.source < edge.target ? 1 : -1;
  return { x: -(b.y - a.y) / distance * sign, y: (b.x - a.x) / distance * sign };
}
function makeEdgeRoutes(scene: Scene, width: number, height: number): EdgeRoute[] {
  const nodes = new Map(scene.nodes.map(node => [node.id, node]));
  const foreground = new Set(scene.foreground.map(node => node.id));
  const boxes: { x: number; y: number; width: number; height: number }[] = [];
  const lanes = parallelLanes(scene.edges);
  const captions = relationCaptions(scene.edges);
  let visibleLabels = 0;
  return scene.edges.map(edge => {
    const a = nodes.get(edge.source)!, b = nodes.get(edge.target)!;
    const full = edge.relation || edge.label || "";
    const short = captions.get(full) ?? full;
    const labelWidth = Math.min(width < 660 ? 144 : 220, Math.max(64, short.length * LABEL_FONT_PX * 0.53 + 12));
    const lines = Math.ceil(short.length * LABEL_FONT_PX * 0.53 / (labelWidth - 12));
    const labelHeight = 8 + lines * 17 + (["inferred", "hypothesis"].includes(edge.kind ?? "") ? 18 : 0);
    const midpoint = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const lane = lanes.get(edge.id) ?? 0, normal = laneNormal(a, b, edge);
    let point = { x: midpoint.x + normal.x * lane, y: midpoint.y + normal.y * lane };
    let labeled = foreground.has(a.id) && foreground.has(b.id) && visibleLabels < 12;
    if (labeled) {
      let found = false;
      for (const offset of [0, 18, -18, 38, -38, 64, -64, 96, -96, 128, -128, 160, -160]) {
        const candidate = { x: midpoint.x + normal.x * (lane + offset), y: midpoint.y + normal.y * (lane + offset) };
        const box = { x: candidate.x - labelWidth / 2, y: candidate.y - labelHeight / 2, width: labelWidth, height: labelHeight };
        if (box.x < 12 || box.x + box.width > width - 12 || box.y < 66 || box.y + box.height > height - 72) continue;
        const overlaps = (other: typeof box) => box.x < other.x + other.width + 4 && box.x + box.width + 4 > other.x && box.y < other.y + other.height + 4 && box.y + box.height + 4 > other.y;
        if (scene.foreground.some(node => overlaps({ x: node.x - node.width / 2, y: node.y - (node.level === "focus" ? 52 : 40), width: node.width, height: node.level === "focus" ? 104 : 80 })) || boxes.some(overlaps)) continue;
        // A routed property's connecting curve must not pass underneath another named entity.
        const bend = { x: (candidate.x - midpoint.x) * 4 / 3, y: (candidate.y - midpoint.y) * 4 / 3 };
        const c = { x: a.x + (b.x - a.x) * 0.42 + bend.x, y: a.y + bend.y };
        const d = { x: b.x - (b.x - a.x) * 0.42 + bend.x, y: b.y + bend.y };
        const obstructed = scene.foreground.some(node => node.id !== a.id && node.id !== b.id && Array.from({ length: 15 }, (_, i) => curvePoint(a, b, c, d, (i + 1) / 16)).some(p => Math.abs(p.x - node.x) < node.width / 2 + 5 && Math.abs(p.y - node.y) < 45));
        if (obstructed) continue;
        point = candidate; boxes.push(box); found = true; break;
      }
      labeled = found;
      if (found) visibleLabels++;
    }
    return { edge, short, full, meaning: `${a.name} → ${short} → ${b.name}`, color: relationColor(edge), point, lane, labeled, width: labelWidth, height: labelHeight };
  });
}

function curvePoint(a: Point, b: Point, c: Point, d: Point, t: number): Point {
  const s = 1 - t;
  return { x: s * s * s * a.x + 3 * s * s * t * c.x + 3 * s * t * t * d.x + t * t * t * b.x,
    y: s * s * s * a.y + 3 * s * s * t * c.y + 3 * s * t * t * d.y + t * t * t * b.y };
}
function targetPositions(scene: Scene, routes: EdgeRoute[]) {
  return new Map<string, Point>([...scene.nodes.map(node => [node.id, node] as [string, Point]), ...routes.filter(route => route.labeled).map(route => [labelKey(route.edge.id), route.point] as [string, Point])]);
}

function ease(t: number) { return 1 - Math.pow(1 - t, 3); }
function lerp(a: number, b: number, t: number) { return a + (b - a) * t; }
function interpolateView(a: View, b: View, t: number): View { return { scale: lerp(a.scale, b.scale, t), x: lerp(a.x, b.x, t), y: lerp(a.y, b.y, t) }; }

function wheelView(view: View, deltaY: number, deltaMode: number, anchor: Point, height: number): View {
  const pixels = deltaY * (deltaMode === 1 ? 16 : deltaMode === 2 ? height : 1);
  const scale = Math.min(2.4, Math.max(0.6, view.scale * Math.exp(-Math.max(-240, Math.min(240, pixels)) * 0.0025)));
  const ratio = scale / view.scale;
  return { scale, x: anchor.x - (anchor.x - view.x) * ratio, y: anchor.y - (anchor.y - view.y) * ratio };
}

type ContextLabelData = { ranked: GraphLabelCandidate[]; names: Map<string, string>; fontFamily: string; full: Map<string, { lines: string[]; width: number; height: number }> };
const contextLabelCache = new WeakMap<Scene, ContextLabelData>();
function measureName(ctx: CanvasRenderingContext2D, name: string) { return typeof ctx.measureText === "function" ? ctx.measureText(name).width : name.length * 7; }
function contextLabels(scene: Scene, ctx: CanvasRenderingContext2D, fontFamily: string) {
  const cached = contextLabelCache.get(scene);
  if (cached?.fontFamily === fontFamily) return cached;
  ctx.font = `13px ${fontFamily}`;
  const names = new Map<string, string>();
  const candidates = scene.nodes.filter(node => node.level === "context").map(node => {
    const name = node.name.length > 36 ? node.name.slice(0, 35) + "…" : node.name;
    names.set(node.id, name);
    const width = measureName(ctx, name);
    return { id: node.id, x: node.x + 7 + width / 2, y: node.y - 5, width, height: 18, seed: node.matched, community: scene.communityMode && patientGroup(node), distance: node.distance, degree: node.degree, sharedSeeds: node.sharedSeeds };
  });
  const data = { ranked: rankGraphLabels(candidates), names, fontFamily, full: new Map<string, { lines: string[]; width: number; height: number }>() };
  contextLabelCache.set(scene, data);
  return data;
}
function fullContextLabel(data: ContextLabelData, node: PositionedNode, ctx: CanvasRenderingContext2D, maxWidth: number) {
  const key = `${node.id}:${Math.round(maxWidth)}`;
  const cached = data.full.get(key);
  if (cached) return cached;
  ctx.font = `14px ${data.fontFamily}`;
  const lines: string[] = [];
  let line = "";
  for (const word of node.name.split(/\s+/)) {
    const next = line ? `${line} ${word}` : word;
    if (line && measureName(ctx, next) > maxWidth) { lines.push(line); line = word; }
    else line = next;
  }
  if (line) lines.push(line);
  const result = { lines, width: Math.max(...lines.map(value => measureName(ctx, value)), 1) + 12, height: lines.length * 19 + 10 };
  data.full.set(key, result);
  return result;
}
function labelObstacles(scene: Scene, routes: EdgeRoute[], positions: Map<string, Point>, view: View, width: number, height: number): ScreenLabelBox[] {
  const boxes: ScreenLabelBox[] = [];
  const add = (point: Point, w: number, h: number) => {
    const x = point.x * view.scale + view.x, y = point.y * view.scale + view.y;
    boxes.push({ left: x - w * view.scale / 2, top: y - h * view.scale / 2, right: x + w * view.scale / 2, bottom: y + h * view.scale / 2, width: w * view.scale, height: h * view.scale });
  };
  for (const node of scene.foreground) add(positions.get(node.id) ?? node, node.width + 10, node.level === "focus" ? 112 : 88);
  for (const route of routes) if (route.labeled) add(positions.get(labelKey(route.edge.id)) ?? route.point, route.width + 8, route.height + 8);
  const bottomHeight = scene.communityMode && scene.nodes.some(patientGroup) ? 100 : 50;
  boxes.push({ left: width - 202, top: 8, right: width, bottom: 130, width: 202, height: 122 }, { left: 0, top: height - bottomHeight, right: width, bottom: height, width, height: bottomHeight });
  return boxes;
}
function contextHit(scene: Scene, positions: Map<string, Point>, view: View, point: Point) {
  let closest: PositionedNode | null = null, minimum = 18;
  for (const node of scene.nodes) {
    if (node.level !== "context") continue;
    const position = positions.get(node.id) ?? node;
    const distance = Math.hypot(position.x * view.scale + view.x - point.x, position.y * view.scale + view.y - point.y);
    if (distance < minimum) { closest = node; minimum = distance; }
  }
  return closest;
}
type LabelPalette = { muted: string; active: string; surface: string; community: string; fontFamily: string };
const DEFAULT_LABEL_PALETTE: LabelPalette = { muted: "#4c6470", active: "#102e2c", surface: "#f8fafb", community: PATIENT_GROUP_COLOR, fontFamily: 'Inter,ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif' };
function paint(canvas: HTMLCanvasElement, scene: Scene, routes: EdgeRoute[], positions: Map<string, Point>, view: View, width: number, height: number, density: number, hovered: string | null, hoveredEdge: string | null = null, palette: LabelPalette = DEFAULT_LABEL_PALETTE) {
  const ctx = canvas.getContext("2d", { alpha: true });
  if (!ctx) return;
  ctx.setTransform(density, 0, 0, density, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const foreground = new Set(scene.foreground.map(node => node.id));
  const screen = (id: string) => { const point = positions.get(id); return point ? { x: point.x * view.scale + view.x, y: point.y * view.scale + view.y } : null; };
  const byId = new Map(scene.nodes.map(node => [node.id, node]));
  for (const cluster of scene.clusters ?? []) {
    const members = cluster.ids.map(id => screen(id)).filter((point): point is Point => Boolean(point));
    if (members.length < 2) continue;
    const minX = Math.min(...members.map(point => point.x)), maxX = Math.max(...members.map(point => point.x));
    const minY = Math.min(...members.map(point => point.y)), maxY = Math.max(...members.map(point => point.y));
    ctx.beginPath(); ctx.ellipse((minX + maxX) / 2, (minY + maxY) / 2, (maxX - minX) / 2 + 96 * view.scale, (maxY - minY) / 2 + 51 * view.scale, 0, 0, Math.PI * 2);
    ctx.fillStyle = cluster.color; ctx.globalAlpha = 0.045; ctx.fill();
  }
  for (const route of routes) {
    const edge = route.edge;
    const a = screen(edge.source), b = screen(edge.target);
    if (!a || !b) continue;
    const direct = (scene.overview || edge.source === scene.rootId || edge.target === scene.rootId) && foreground.has(edge.source) && foreground.has(edge.target);
    const label = route.labeled ? screen(labelKey(edge.id)) : null;
    const normal = laneNormal(a, b, edge);
    const offset = label ? { x: (label.x - (a.x + b.x) / 2) * 4 / 3, y: (label.y - (a.y + b.y) / 2) * 4 / 3 } : { x: normal.x * route.lane * view.scale * 4 / 3, y: normal.y * route.lane * view.scale * 4 / 3 };
    const c = { x: a.x + (b.x - a.x) * 0.42 + offset.x, y: a.y + offset.y };
    const d = { x: b.x - (b.x - a.x) * 0.42 + offset.x, y: b.y + offset.y };
    ctx.beginPath(); ctx.moveTo(a.x, a.y);
    ctx.bezierCurveTo(c.x, c.y, d.x, d.y, b.x, b.y);
    // Backend 'highlighted' is not focus state: unrelated edges always recede.
    ctx.strokeStyle = route.color;
    const distance = Math.min(byId.get(edge.source)?.distance ?? 1, byId.get(edge.target)?.distance ?? 1);
    const activeEdge = hoveredEdge === edge.id;
    ctx.globalAlpha = activeEdge ? 1 : direct ? 0.88 : Math.max(0.18, (scene.overview ? 0.45 : 0.42) - distance * 0.07);
    ctx.lineWidth = activeEdge ? 2.5 : direct ? 1.35 : 0.7;
    ctx.shadowColor = route.color; ctx.shadowBlur = activeEdge ? 5 : 0;
    ctx.setLineDash(direct && ["inferred", "hypothesis"].includes(edge.kind ?? "") ? [4, 3] : []);
    ctx.stroke();
    ctx.shadowBlur = 0;
    ctx.setLineDash([]);
    if (direct) {
      const target = byId.get(edge.target)!;
      const halfWidth = target.width * view.scale / 2 + 6;
      const halfHeight = (target.level === "focus" ? 52 : 40) * view.scale + 6;
      for (let t = 0.98; t > 0.08; t -= 0.025) {
        const tip = curvePoint(a, b, c, d, t);
        if (Math.abs(tip.x - b.x) <= halfWidth && Math.abs(tip.y - b.y) <= halfHeight) continue;
        const behind = curvePoint(a, b, c, d, Math.max(0, t - 0.03));
        const length = Math.max(1, Math.hypot(tip.x - behind.x, tip.y - behind.y));
        const dx = (tip.x - behind.x) / length, dy = (tip.y - behind.y) / length;
        ctx.beginPath(); ctx.moveTo(tip.x, tip.y); ctx.lineTo(tip.x - dx * 7 - dy * 3, tip.y - dy * 7 + dx * 3); ctx.lineTo(tip.x - dx * 7 + dy * 3, tip.y - dy * 7 - dx * 3); ctx.closePath(); ctx.fillStyle = route.color; ctx.fill(); break;
      }
    }
  }
  for (const node of scene.nodes) {
    if (node.level !== "context") continue;
    const point = screen(node.id);
    if (!point) continue;
    ctx.beginPath(); ctx.arc(point.x, point.y, 2.7, 0, Math.PI * 2);
    ctx.fillStyle = scene.communityMode && patientGroup(node) ? palette.community : nodeColor(node, scene.communityMode);
    ctx.globalAlpha = node.id === hovered ? 0.9 : scene.communityMode && patientGroup(node) ? Math.max(0.5, 0.86 - (node.distance ?? 1) * 0.06) : Math.max(0.25, 0.61 - (node.distance ?? 1) * 0.08);
    ctx.fill();
  }
  const data = contextLabels(scene, ctx, palette.fontFamily);
  const active = hovered ? scene.nodes.find(node => node.id === hovered && node.level === "context") : undefined;
  const full = active ? fullContextLabel(data, active, ctx, Math.min(220, (width - 36) / view.scale)) : undefined;
  const ranked = data.ranked.map(candidate => {
    const node = byId.get(candidate.id)!, position = positions.get(candidate.id) ?? node;
    return { ...candidate, x: candidate.x + position.x - node.x, y: candidate.y + position.y - node.y };
  });
  const activePoint = active ? positions.get(active.id) ?? active : undefined;
  const visible = visibleGraphLabels(ranked, view, { width, height }, {
    activeId: active?.id, maxLabels: 24, obstacles: labelObstacles(scene, routes, positions, view, width, height),
    activeCandidate: active && activePoint && full ? { id: active.id, x: activePoint.x + 7 + full.width / 2, y: activePoint.y - full.height / 2 - 5, width: full.width, height: full.height } : undefined,
  });
  for (const [id, box] of visible) {
    const isActive = id === active?.id;
    if (isActive && box.needsReadout) continue;
    ctx.font = `${graphLabelScale(view.scale, isActive ? 14 : LABEL_FONT_PX)}px ${palette.fontFamily}`;
    const communityLabel = scene.communityMode && patientGroup(byId.get(id)!);
    ctx.globalAlpha = isActive || communityLabel ? 1 : 0.72;
    if (isActive && full) {
      ctx.fillStyle = palette.surface;
      if (typeof ctx.fillRect === "function") ctx.fillRect(box.left, box.top, box.width, box.height);
      ctx.fillStyle = palette.active;
      full.lines.forEach((line, row) => ctx.fillText(line, box.left + 6 * view.scale, box.top + (19 + row * 19) * view.scale));
    } else { ctx.fillStyle = communityLabel ? palette.community : palette.muted; ctx.fillText(data.names.get(id)!, box.left, box.top + graphLabelScale(view.scale)); }
  }
  ctx.globalAlpha = 1;
}

function Arrow({ direction }: { direction: "back" | "next" }) {
  return <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><path d={direction === "back" ? "M15 5l-7 7 7 7" : "M9 5l7 7-7 7"} /></svg>;
}

function ForegroundNode({ node, selected, labels, communityMode, onSelect, onHighlight, getOrigin }: {
  node: PositionedNode; selected?: string | null; labels: GraphLabels; communityMode?: boolean;
  onSelect: (id: string) => void; onHighlight: (id: string | null, source: "pointer" | "focus") => void; getOrigin: (id: string) => Point | null;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const button = ref.current, from = getOrigin(node.id);
    if (!button || !from || window.matchMedia("(prefers-reduced-motion: reduce)").matches
      || Math.abs(from.x - node.x) + Math.abs(from.y - node.y) < 1) return;
    const animation = button.animate([
      { transform: `translate(${from.x}px,${from.y}px) translate(-50%,-50%)` },
      { transform: `translate(${node.x}px,${node.y}px) translate(-50%,-50%)` },
    ], { duration: MOTION_MS, easing: "cubic-bezier(.33,1,.68,1)" });
    return () => animation.cancel();
  }, [getOrigin, node.id, node.x, node.y]);
  return <button ref={ref} type="button" className={`${styles.node} ${node.level === "focus" ? styles.focusNode : node.level === "anchor" ? styles.anchorNode : styles.neighborNode} ${communityMode && patientGroup(node) ? styles.patientNode : ""}`}
    style={{ width: node.width, transform: `translate(${node.x}px,${node.y}px) translate(-50%,-50%)`, borderColor: node.level === "focus" ? nodeColor(node, communityMode) : undefined }}
    aria-pressed={node.id === selected} title={node.name} onClick={() => onSelect(node.id)} onPointerEnter={() => onHighlight(node.id, "pointer")} onPointerLeave={() => onHighlight(null, "pointer")} onFocus={() => onHighlight(node.id, "focus")} onBlur={() => onHighlight(null, "focus")}>
    <span className={styles.nodeCaption}><span className={styles.nodeKind}><span className={styles.nodeDot} style={{ backgroundColor: nodeColor(node, communityMode) }} />{labels.kinds?.[nodeKind(node)] ?? nodeKind(node).replaceAll("_", " ")}</span><strong>{node.name}</strong></span>
  </button>;
}

function EdgeLabel({ route, labels, onInspect, onHighlight, getOrigin }: { route: EdgeRoute; labels: GraphLabels; onInspect: () => void; onHighlight: (id: string | null, source: "pointer" | "focus") => void; getOrigin: (id: string) => Point | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const button = ref.current, from = getOrigin(labelKey(route.edge.id));
    if (!button || !from || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const animation = button.animate([{ transform: `translate(${from.x}px,${from.y}px) translate(-50%,-50%)` }, { transform: `translate(${route.point.x}px,${route.point.y}px) translate(-50%,-50%)` }], { duration: MOTION_MS, easing: "cubic-bezier(.33,1,.68,1)" });
    return () => animation.cancel();
  }, [getOrigin, route.edge.id, route.point.x, route.point.y]);
  const qualifier = route.edge.kind === "inferred" ? labels.inferred : route.edge.kind === "hypothesis" ? labels.hypothesis : undefined;
  return <button ref={ref} type="button" className={styles.edgeLabel} style={{ width: route.width, minHeight: route.height, transform: `translate(${route.point.x}px,${route.point.y}px) translate(-50%,-50%)`, color: route.color }} aria-label={route.meaning} title={`${route.meaning}\n${route.full}`} onClick={onInspect} onPointerEnter={() => onHighlight(route.edge.id, "pointer")} onPointerLeave={() => onHighlight(null, "pointer")} onFocus={() => onHighlight(route.edge.id, "focus")} onBlur={() => onHighlight(null, "focus")}>
    {qualifier && <span className={styles.qualifier}>{qualifier}</span>}<span>{route.short}</span>
  </button>;
}

export default function Graph({ graph, selectedNodeId, resultNodeIds, onSelectNode, onSelectEdge, navigating = false, labels, mode = "search" }: GraphProps) {
  const index = useMemo(() => indexGraph(graph, labels.kinds), [graph, labels.kinds]);
  const focusId = selectedNodeId ?? "";
  const [dimensions, setDimensions] = useState({ width: 0, height: 0, density: 1 });
  const [pager, setPager] = useState({ focusId: "", page: 0 });
  const [listOpen, setListOpen] = useState(false);
  const [listFilter, setListFilter] = useState<string[] | null>(null);
  const [listPage, setListPage] = useState(0);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [legendOpen, setLegendOpen] = useState(false);
  const [inspectedEdgeId, setInspectedEdgeId] = useState<string | null>(null);
  const [layoutMemory, setLayoutMemory] = useState<LayoutMemory | undefined>(undefined);
  const page = pager.focusId === focusId ? pager.page : 0;
  const scene = useMemo<Scene>(() => dimensions.width && dimensions.height ? makeScene(index, focusId, page, dimensions.width, dimensions.height, mode, layoutMemory, resultNodeIds)
    : { nodes: [], foreground: [], edges: [], totalNeighbors: 0, page: 0, pages: 1 }, [index, focusId, page, dimensions.width, dimensions.height, mode, layoutMemory, resultNodeIds]);
  const routes = useMemo(() => makeEdgeRoutes(scene, dimensions.width, dimensions.height), [scene, dimensions.width, dimensions.height]);
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const layerRef = useRef<HTMLDivElement>(null);
  const listButtonRef = useRef<HTMLButtonElement>(null);
  const listId = useId();
  const sceneRef = useRef<Scene>(scene);
  const routesRef = useRef<EdgeRoute[]>(routes);
  const positionsRef = useRef(new Map<string, Point>());
  const dimensionsRef = useRef(dimensions);
  const viewRef = useRef<View>({ ...IDENTITY });
  const transitionRef = useRef<Transition | null>(null);
  const reducedMotionRef = useRef(false);
  const hoveredRef = useRef<string | null>(null);
  const hoveredEdgeRef = useRef<string | null>(null);
  const nodeHighlightsRef = useRef<{ pointer: string | null; focus: string | null }>({ pointer: null, focus: null });
  const edgeHighlightsRef = useRef<{ pointer: string | null; focus: string | null }>({ pointer: null, focus: null });
  const labelPaletteRef = useRef<LabelPalette>(DEFAULT_LABEL_PALETTE);
  const requestPaintRef = useRef<() => void>(() => {});
  const pointersRef = useRef(new Map<number, Point>());
  const gestureRef = useRef({ start: { x: 0, y: 0 }, distance: 0, view: { ...IDENTITY }, moved: false });
  const getOrigin = useCallback((id: string) => {
    if (!positionsRef.current.size) return null;
    return positionsRef.current.get(id) ?? positionsRef.current.get(sceneRef.current.rootId ?? "") ?? null;
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current, host = hostRef.current;
    if (!canvas || !host) return;
    const pointers = pointersRef.current;
    let frame = 0;
    const updatePalette = () => {
      const style = getComputedStyle(host);
      labelPaletteRef.current = { muted: style.getPropertyValue("--g-label-muted").trim() || DEFAULT_LABEL_PALETTE.muted, active: style.getPropertyValue("--g-label-active").trim() || DEFAULT_LABEL_PALETTE.active, surface: style.getPropertyValue("--g-label-surface").trim() || DEFAULT_LABEL_PALETTE.surface, community: style.getPropertyValue("--g-community-ink").trim() || DEFAULT_LABEL_PALETTE.community, fontFamily: style.fontFamily || DEFAULT_LABEL_PALETTE.fontFamily };
      requestPaintRef.current();
    };
    updatePalette();
    const themeObserver = new MutationObserver(updatePalette);
    for (const element of [host, document.documentElement, document.body]) themeObserver.observe(element, { attributes: true, attributeFilter: ["data-theme", "class"] });
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateMotion = () => {
      reducedMotionRef.current = motion.matches;
      if (motion.matches && transitionRef.current) {
        transitionRef.current = null;
        positionsRef.current = targetPositions(sceneRef.current, routesRef.current);
        for (const button of layerRef.current?.querySelectorAll("button") ?? []) for (const animation of button.getAnimations()) animation.cancel();
        requestPaintRef.current();
      }
    };
    updateMotion(); motion.addEventListener("change", updateMotion);
    const draw = (now: number) => {
      frame = 0;
      const current = sceneRef.current;
      const transition = transitionRef.current;
      const progress = transition ? Math.min(1, (now - transition.start) / transition.duration) : 1;
      const eased = ease(progress);
      const positions = new Map<string, Point>();
      for (const node of current.nodes) {
        const from = transition?.from.get(node.id) ?? node;
        positions.set(node.id, { x: lerp(from.x, node.x, eased), y: lerp(from.y, node.y, eased) });
      }
      for (const route of routesRef.current) if (route.labeled) {
        const key = labelKey(route.edge.id), from = transition?.from.get(key) ?? route.point;
        positions.set(key, { x: lerp(from.x, route.point.x, eased), y: lerp(from.y, route.point.y, eased) });
      }
      positionsRef.current = positions;
      if (transition) viewRef.current = interpolateView(transition.fromView, transition.toView, eased);
      const { width, height, density } = dimensionsRef.current;
      paint(canvas, current, routesRef.current, positions, viewRef.current, width, height, density, hoveredRef.current, hoveredEdgeRef.current, labelPaletteRef.current);
      if (transition && progress < 1) frame = requestAnimationFrame(draw);
      else transitionRef.current = null;
    };
    requestPaintRef.current = () => { if (!frame) frame = requestAnimationFrame(draw); };
    const resize = () => {
      const width = Math.round(host.clientWidth), height = Math.round(host.clientHeight);
      if (!width || !height) return;
      const density = Math.min(window.devicePixelRatio || 1, width < 660 ? 1.5 : 2);
      const previous = dimensionsRef.current;
      if (previous.width === width && previous.height === height && previous.density === density) return;
      dimensionsRef.current = { width, height, density };
      canvas.width = Math.round(width * density); canvas.height = Math.round(height * density);
      setDimensions({ width, height, density });
    };
    const observer = new ResizeObserver(resize); observer.observe(host); resize();
    const wheel = (event: WheelEvent) => {
      if (event.target instanceof Element && event.target.closest("[data-zebra-graph-scroll]")) return;
      event.preventDefault(); event.stopPropagation();
      const bounds = canvas.getBoundingClientRect();
      transitionRef.current = null;
      positionsRef.current = targetPositions(sceneRef.current, routesRef.current);
      for (const button of layerRef.current?.querySelectorAll("button") ?? []) for (const animation of button.getAnimations()) animation.cancel();
      host.dataset.interacting = "true";
      const anchor = { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
      const next = wheelView(viewRef.current, event.deltaY, event.deltaMode, anchor, host.clientHeight);
      viewRef.current = next;
      if (layerRef.current) { layerRef.current.style.transition = "none"; layerRef.current.style.transform = `translate(${next.x}px,${next.y}px) scale(${next.scale})`; }
      requestPaintRef.current();
    };
    host.addEventListener("wheel", wheel, { passive: false });
    return () => {
      observer.disconnect(); themeObserver.disconnect(); motion.removeEventListener("change", updateMotion); host.removeEventListener("wheel", wheel);
      if (frame) cancelAnimationFrame(frame);
      requestPaintRef.current = () => {}; pointers.clear();
    };
  }, []);

  useEffect(() => {
    const previous = sceneRef.current;
    const rootChanged = previous.rootId !== scene.rootId;
    const moved = scene.nodes.some(node => { const point = positionsRef.current.get(node.id); return point && Math.abs(point.x - node.x) + Math.abs(point.y - node.y) > 1; });
    const added = scene.nodes.some(node => !positionsRef.current.has(node.id));
    const toView = rootChanged ? { ...IDENTITY } : { ...viewRef.current };
    const from = new Map(positionsRef.current);
    const previousRoot = from.get(previous.rootId ?? "") ?? { x: dimensions.width / 2, y: dimensions.height / 2 };
    for (const node of scene.nodes) if (!from.has(node.id)) {
      const connection = scene.edges.find(edge => edge.source === node.id ? from.has(edge.target) : edge.target === node.id && from.has(edge.source));
      const parent = connection ? from.get(connection.source === node.id ? connection.target : connection.source) : undefined;
      from.set(node.id, parent ?? previousRoot);
    }
    for (const route of routes) if (!from.has(labelKey(route.edge.id))) from.set(labelKey(route.edge.id), previousRoot);
    sceneRef.current = scene;
    routesRef.current = routes;
    if (!reducedMotionRef.current && previous.nodes.length > 0 && (moved || rootChanged || added)) {
      transitionRef.current = { from, fromView: { ...viewRef.current }, toView, start: performance.now(), duration: MOTION_MS };
    } else {
      transitionRef.current = null;
      positionsRef.current = targetPositions(scene, routes);
      viewRef.current = toView;
    }
    if (layerRef.current) {
      layerRef.current.style.transition = reducedMotionRef.current ? "none" : `transform ${MOTION_MS}ms cubic-bezier(.33,1,.68,1)`;
      layerRef.current.style.transform = `translate(${toView.x}px,${toView.y}px) scale(${toView.scale})`;
    }
    requestPaintRef.current();
  }, [scene, routes, dimensions.width, dimensions.height]);

  function cancelTransition() {
    transitionRef.current = null;
    positionsRef.current = targetPositions(sceneRef.current, routesRef.current);
    if (hostRef.current) hostRef.current.dataset.interacting = "true";
    if (layerRef.current) {
      layerRef.current.style.transition = "none";
      for (const button of layerRef.current.querySelectorAll("button")) for (const animation of button.getAnimations()) animation.cancel();
    }
  }
  function applyView(view: View) {
    viewRef.current = view;
    if (layerRef.current) layerRef.current.style.transform = `translate(${view.x}px,${view.y}px) scale(${view.scale})`;
    requestPaintRef.current();
  }
  function changeView(view: View, factor: number, anchor: Point) {
    cancelTransition();
    const scale = Math.min(2.4, Math.max(0.6, view.scale * factor)), ratio = scale / view.scale;
    applyView({ scale, x: anchor.x - (anchor.x - view.x) * ratio, y: anchor.y - (anchor.y - view.y) * ratio });
  }
  function zoom(factor: number) { changeView(viewRef.current, factor, { x: dimensionsRef.current.width / 2, y: dimensionsRef.current.height / 2 }); }
  function fit() { cancelTransition(); applyView({ ...IDENTITY }); }
  function rememberMap() {
    const { width, height } = dimensionsRef.current;
    setLayoutMemory({ points: new Map(positionsRef.current), rootId: sceneRef.current.rootId, width, height });
  }
  function select(id: string) {
    if (id === selectedNodeId) return;
    rememberMap();
    if (hostRef.current) delete hostRef.current.dataset.interacting;
    nodeHighlightsRef.current = { pointer: null, focus: null }; edgeHighlightsRef.current = { pointer: null, focus: null }; hoveredEdgeRef.current = null;
    setHoveredId(null); hoveredRef.current = null; setListOpen(false); setListFilter(null); setInspectedEdgeId(null); onSelectNode(id);
  }
  function highlightNode(id: string | null, source: "pointer" | "focus" = "pointer") {
    nodeHighlightsRef.current[source] = id;
    const active = nodeHighlightsRef.current.pointer ?? nodeHighlightsRef.current.focus;
    if (active === hoveredRef.current) return;
    hoveredRef.current = active; setHoveredId(active); requestPaintRef.current();
  }
  function highlightEdge(id: string | null, source: "pointer" | "focus") {
    edgeHighlightsRef.current[source] = id;
    const active = edgeHighlightsRef.current.pointer ?? edgeHighlightsRef.current.focus;
    if (active === hoveredEdgeRef.current) return;
    hoveredEdgeRef.current = active; requestPaintRef.current();
  }
  function localPoint(event: { clientX: number; clientY: number; currentTarget: HTMLCanvasElement }) {
    const bounds = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
  }
  function hit(point: Point) {
    return contextHit(sceneRef.current, positionsRef.current, viewRef.current, point);
  }
  function startGesture() {
    const points = [...pointersRef.current.values()];
    gestureRef.current = { start: points.length > 1 ? { x: (points[0].x + points[1].x) / 2, y: (points[0].y + points[1].y) / 2 } : points[0], distance: points.length > 1 ? Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y) : 0, view: { ...viewRef.current }, moved: points.length > 1 };
  }
  const hoveredNode = hoveredId ? index.byId.get(hoveredId) : null;
  const root = scene.foreground[0];
  const inspected = routes.find(route => route.edge.id === inspectedEdgeId);
  const foregroundIds = new Set(scene.foreground.map(node => node.id));
  const legend = routes.filter(route => foregroundIds.has(route.edge.source) && foregroundIds.has(route.edge.target));
  const listedNodes = index.nodes.filter(node => !listFilter || listFilter.includes(node.id));
  const listPages = Math.max(1, Math.ceil(listedNodes.length / 40));
  const currentListPage = Math.min(listPage, listPages - 1);
  const patientGroups = mode === "community" ? index.nodes.filter(patientGroup) : [];
  const neighborCaption = scene.overview ? `${labels.nodeList} · ${scene.totalNeighbors}` : (labels.moreNeighbors ?? labels.connections).replace("{count}", String(scene.totalNeighbors));

  return <div ref={hostRef} className={styles.graph} role="region" aria-label={labels.region} tabIndex={0} aria-busy={navigating}
    onKeyDown={event => {
      if (event.target !== event.currentTarget) return;
      const directions: Record<string, Point> = { ArrowLeft: { x: 36, y: 0 }, ArrowRight: { x: -36, y: 0 }, ArrowUp: { x: 0, y: 36 }, ArrowDown: { x: 0, y: -36 } };
      if (directions[event.key]) { event.preventDefault(); cancelTransition(); applyView({ ...viewRef.current, x: viewRef.current.x + directions[event.key].x, y: viewRef.current.y + directions[event.key].y }); }
      if (event.key === "Escape") fit();
    }}>
    <canvas ref={canvasRef} className={styles.canvas} aria-hidden="true"
      onPointerDown={event => { if (event.button !== 0) return; cancelTransition(); event.currentTarget.setPointerCapture(event.pointerId); pointersRef.current.set(event.pointerId, localPoint(event)); startGesture(); }}
      onPointerMove={event => {
        const point = localPoint(event);
        if (!pointersRef.current.has(event.pointerId)) {
          const node = hit(point);
          highlightNode(node?.id ?? null, "pointer");
          event.currentTarget.style.cursor = node ? "pointer" : "grab"; return;
        }
        pointersRef.current.set(event.pointerId, point);
        const points = [...pointersRef.current.values()], gesture = gestureRef.current;
        const center = points.length > 1 ? { x: (points[0].x + points[1].x) / 2, y: (points[0].y + points[1].y) / 2 } : point;
        if (Math.abs(center.x - gesture.start.x) + Math.abs(center.y - gesture.start.y) > 5) gesture.moved = true;
        const distance = points.length > 1 ? Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y) : 0;
        const scale = gesture.distance ? Math.min(2.4, Math.max(0.6, gesture.view.scale * distance / gesture.distance)) : gesture.view.scale;
        const ratio = scale / gesture.view.scale;
        applyView({ scale, x: center.x - (gesture.start.x - gesture.view.x) * ratio, y: center.y - (gesture.start.y - gesture.view.y) * ratio });
      }}
      onPointerUp={event => {
        if (!pointersRef.current.has(event.pointerId)) return;
        const node = !gestureRef.current.moved && pointersRef.current.size === 1 ? hit(localPoint(event)) : null;
        pointersRef.current.delete(event.pointerId); if (pointersRef.current.size) { startGesture(); gestureRef.current.moved = true; }
        if (node) select(node.id);
      }}
      onPointerCancel={event => { pointersRef.current.delete(event.pointerId); if (pointersRef.current.size) startGesture(); }}
      onPointerLeave={() => highlightNode(null, "pointer")} />
    <div ref={layerRef} className={styles.nodeLayer}>
      {scene.foreground.map(node => <ForegroundNode key={node.id} node={node} selected={selectedNodeId} labels={labels} communityMode={scene.communityMode} onSelect={select} onHighlight={highlightNode} getOrigin={getOrigin} />)}
      {routes.filter(route => route.labeled).map(route => <EdgeLabel key={route.edge.id} route={route} labels={labels} getOrigin={getOrigin} onHighlight={highlightEdge} onInspect={() => setInspectedEdgeId(route.edge.id)} />)}
    </div>
    {legend.length > 0 && labels.edgeProperties && <div className={styles.legend} data-zebra-graph-legend data-zebra-graph-scroll>
      <button type="button" onClick={() => setLegendOpen(!legendOpen)} aria-expanded={legendOpen}>{labels.edgeProperties}<span className={styles.legendSwatches}>{[...new Set(legend.map(route => route.color))].map(color => <i key={color} style={{ backgroundColor: color }} />)}</span></button>
      {legendOpen && <ul>
        {scene.clusters?.map(cluster => <li key={cluster.key}><span className={styles.legendLine} style={{ backgroundColor: cluster.color }} /><button type="button" title={cluster.relation} onClick={() => { setListPage(0); setListFilter(cluster.allIds); setListOpen(true); setLegendOpen(false); }}>{labels.kinds?.[cluster.kind] ?? cluster.kind.replaceAll("_", " ")}<span className={styles.clusterCount}>{cluster.allIds.length}</span></button></li>)}
        {legend.map(route => <li key={route.edge.id}><span className={styles.legendLine} style={{ backgroundColor: route.color }} /><button type="button" title={route.full} onClick={() => setInspectedEdgeId(route.edge.id)}><span>{route.meaning}</span>{route.edge.kind === "inferred" && labels.inferred && <small>{labels.inferred}</small>}{route.edge.kind === "hypothesis" && labels.hypothesis && <small>{labels.hypothesis}</small>}{route.edge.evidence?.length ? <small>{[...new Set(route.edge.evidence.map(record => record.source))].join(" / ")}</small> : null}</button></li>)}
      </ul>}
    </div>}
    {inspected && <aside className={styles.edgeInspector} data-zebra-graph-scroll aria-label={labels.property ?? labels.connections}>
      <header><strong style={{ color: inspected.color }}>{inspected.short}</strong><button type="button" onClick={() => setInspectedEdgeId(null)} aria-label={labels.close ?? labels.back ?? labels.fit}>×</button></header>
      <p>{index.byId.get(inspected.edge.source)?.name} → {index.byId.get(inspected.edge.target)?.name}</p>
      <code>{inspected.full}</code>
      <ReasoningProof proof={inspected.edge.proof} labels={new Map(index.nodes.map(node => [node.id, node.name]))} />
      {["inferred", "hypothesis"].includes(inspected.edge.kind ?? "") && <p>{inspected.edge.kind === "inferred" ? labels.inferred : labels.hypothesis}</p>}
      <Sources assertions={[inspected.edge]} evidence={inspected.edge.evidence} />
      {onSelectEdge && labels.evidence && <button type="button" className={styles.evidenceButton} onClick={() => onSelectEdge(inspected.edge)}>{labels.evidence}</button>}
    </aside>}
    <div className={styles.neighborNavigation}>
      {scene.totalNeighbors > 0 ? <span>{neighborCaption}</span> : root && labels.noNeighbors ? <span>{labels.noNeighbors}</span> : null}
      {scene.pages > 1 && <div className={styles.pages}>
        <button type="button" onClick={() => setPager({ focusId, page: scene.page - 1 })} disabled={scene.page === 0} aria-label={labels.previousNeighbors ?? labels.back ?? labels.nodeList}><Arrow direction="back" /></button>
        <span>{scene.page + 1} / {scene.pages}</span>
        <button type="button" onClick={() => setPager({ focusId, page: scene.page + 1 })} disabled={scene.page === scene.pages - 1} aria-label={labels.nextNeighbors ?? labels.nodeList}><Arrow direction="next" /></button>
      </div>}
    </div>
    {!!patientGroups.length && labels.kinds?.patient_group && <button type="button" className={styles.communityKey} data-zebra-graph-scroll onClick={() => { setListPage(0); setListFilter(patientGroups.map(node => node.id)); setListOpen(true); }}><span aria-hidden="true" />{labels.kinds.patient_group}<small>{patientGroups.length}</small></button>}
    <div className={styles.controls} data-zebra-graph-controls>
      <button type="button" onClick={() => zoom(1.2)} aria-label={labels.zoomIn} title={labels.zoomIn}>+</button>
      <button type="button" onClick={() => zoom(1 / 1.2)} aria-label={labels.zoomOut} title={labels.zoomOut}>−</button>
      <button type="button" onClick={fit} aria-label={labels.fit} title={labels.fit}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5" /><circle cx="12" cy="12" r="3" /></svg></button>
      <button ref={listButtonRef} type="button" onClick={() => { setListPage(0); setListFilter(null); setListOpen(!listOpen); }} aria-label={labels.nodeList} title={labels.nodeList} aria-expanded={listOpen} aria-controls={listId}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M9 6h12M9 12h12M9 18h12" /><circle cx="4" cy="6" r="1" /><circle cx="4" cy="12" r="1" /><circle cx="4" cy="18" r="1" /></svg></button>
    </div>
    {listOpen && <div id={listId} className={styles.nodeList} data-zebra-graph-scroll role="group" aria-label={labels.nodeList} onKeyDown={event => { if (event.key === "Escape") { setListOpen(false); listButtonRef.current?.focus(); } }}>
      {listedNodes.slice(currentListPage * 40, (currentListPage + 1) * 40).map(node => <button key={node.id} type="button" aria-pressed={node.id === selectedNodeId} onFocus={() => highlightNode(node.id, "focus")} onBlur={() => highlightNode(null, "focus")} onPointerEnter={() => highlightNode(node.id, "pointer")} onPointerLeave={() => highlightNode(null, "pointer")} onClick={() => { select(node.id); listButtonRef.current?.focus(); }}><span className={styles.nodeDot} style={{ backgroundColor: nodeColor(node, scene.communityMode) }} /><span>{node.name}</span></button>)}
      {listPages > 1 && <div className={styles.pages}><button type="button" disabled={!currentListPage} onClick={() => setListPage(currentListPage - 1)} aria-label={labels.previousNeighbors ?? labels.back ?? labels.nodeList}><Arrow direction="back" /></button><span>{currentListPage + 1} / {listPages}</span><button type="button" disabled={currentListPage === listPages - 1} onClick={() => setListPage(currentListPage + 1)} aria-label={labels.nextNeighbors ?? labels.nodeList}><Arrow direction="next" /></button></div>}
    </div>}
    {hoveredNode && <div className={styles.hoverLabel} aria-hidden="true"><strong>{hoveredNode.name}</strong><small>{labels.kinds?.[nodeKind(hoveredNode)] ?? nodeKind(hoveredNode).replaceAll("_", " ")}</small></div>}
    {navigating && labels.loading && <div className={styles.loading} data-zebra-graph-status role="status"><ZebraLoader />{labels.loading}</div>}
    {!!scene.omitted && labels.moreNodes && <div className={styles.contextCount}>{labels.moreNodes.replace("{count}", String(scene.omitted))}</div>}
  </div>;
}
