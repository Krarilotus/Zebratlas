import pin from "./tbox-pin.json";
import cytoscape from "cytoscape";
export type TBoxExpression = { type: "named"; id: string } | { type: "literal"; value: string; datatype?: string | null; language?: string | null } | { type: "intersection" | "union"; members: TBoxExpression[] } | { type: "restriction"; property: string; quantifier: string; target: TBoxExpression };
export type TBoxClass = { id: string; label: string; definition: string; modules: string[] };
export type TBoxProperty = { id: string; label: string; definition: string; kind: "object" | "datatype"; module: string; domains: TBoxExpression[]; ranges: TBoxExpression[] };
export type TBoxData = {
  schema: string; version: string; defaultBridge: "bfo"; scope: "authored_tbox_only"; activeStoreInference: false;
  counts: { classes: number; bridgeClasses: number; allNamedClasses: number; properties: number; equivalences: number };
  classes: TBoxClass[]; properties: TBoxProperty[]; references: { id: string; label: string }[];
  subclasses: { source: string; target: TBoxExpression; module: string }[];
  equivalences: { source: string; expression: TBoxExpression; module: string }[];
  axioms?: { source: string; target: TBoxExpression; kind: "inverse" | "disjoint" | "subproperty"; predicate: string; module: string }[];
  modules: { id: string; file: string; bytes: number; sha256: string }[];
  sourcePins: { file: string; source_url: string; sha256: string; version: string; license: string }[];
  manifestSha256: string; governanceSha256: string;
  validation: { sha256: string; pinChecks: string; regressions: number; profiles: { profile: string; reasoner: string; consistent: boolean; expected_inconsistent: boolean }[]; limitations: string[] };
};
export const ATLAS_VOCAB = "https://w3id.org/rare-disease-atlas/vocab#";
export const OWL_THING = "http://www.w3.org/2002/07/owl#Thing";
export const TBOX_PIN = pin;
export function compactTBoxId(id: string) {
  const prefixes: [string, string][] = [[ATLAS_VOCAB, "ra:"], ["http://purl.obolibrary.org/obo/", "obo:"], ["http://www.ontologydesignpatterns.org/ont/dul/DUL.owl#", "dul:"], ["http://www.w3.org/2002/07/owl#", "owl:"], ["http://www.w3.org/ns/prov#", "prov:"], ["http://www.w3.org/2001/XMLSchema#", "xsd:"]];
  const prefix = prefixes.find(([namespace]) => id.startsWith(namespace));
  return prefix ? prefix[1] + id.slice(prefix[0].length) : id;
}
export function expressionText(expression: TBoxExpression): string {
  switch (expression.type) {
    case "named": return compactTBoxId(expression.id);
    case "literal": return JSON.stringify(expression.value) + (expression.datatype ? `^^${compactTBoxId(expression.datatype)}` : expression.language ? `@${expression.language}` : "");
    case "intersection": return `(${expression.members.map(expressionText).join(" AND ")})`;
    case "union": return `(${expression.members.map(expressionText).join(" OR ")})`;
    case "restriction": return `${compactTBoxId(expression.property)} ${expression.quantifier.toUpperCase()} ${expressionText(expression.target)}`;
  }
}
export function tboxTerm(data: TBoxData, id: string) {
  const term = data.classes.find(item => item.id === id) ?? data.properties.find(item => item.id === id) ?? data.references.find(item => item.id === id);
  return { id, label: term?.label ?? compactTBoxId(id), kind: data.properties.some(item => item.id === id) ? "property" : id.startsWith("http://www.w3.org/2001/XMLSchema#") ? "datatype" : "class", external: !id.startsWith(ATLAS_VOCAB) };
}
export type TBoxDiagramNode = ReturnType<typeof tboxTerm> & { x: number; y: number; width: number; height: number; selected: boolean; prominent?: boolean; expression?: TBoxExpression; modules?: string[] };
export type TBoxDiagramEdge = { id: string; source: string; target: string; kind: "subclass" | "domain" | "range" | "union" | "equivalent" | "expression" | "inverse" | "disjoint" | "subproperty" | "property"; wildcard: boolean; module?: string; role?: string; predicate?: string; propertyId?: string; label?: string; projection?: boolean };
type Diagram = { nodes: TBoxDiagramNode[]; edges: TBoxDiagramEdge[]; totalEdges: number; omitted: number; height: number };
export type FullTBoxDiagram = Diagram & { width: number; focusBounds: { left: number; top: number; width: number; height: number } };
const fullSceneCache = new WeakMap<TBoxData, Omit<FullTBoxDiagram, "focusBounds">>();

/** Both authored bridges are shown as schema context, without activating either in a store. */
export function fullTBoxDiagram(data: TBoxData, viewportWidth: number, selected = ""): FullTBoxDiagram {
  let cached = fullSceneCache.get(data);
  if (!cached) {
    const terms = new Map<string, TBoxDiagramNode>(), edges: TBoxDiagramEdge[] = [];
    const term = (id: string, module = "atlas") => {
      if (!terms.has(id)) terms.set(id, { ...tboxTerm(data, id), x: 0, y: 0, width: 142, height: 54, selected: false, modules: [] });
      const node = terms.get(id)!;
      if (!node.modules!.includes(module)) node.modules!.push(module);
      return node;
    };
    for (const item of data.classes) for (const moduleName of item.modules) term(item.id, moduleName);
    for (const item of data.properties) term(item.id, item.module);
    const add = (source: string, target: string, kind: TBoxDiagramEdge["kind"], module: string, role?: string, extra?: Partial<TBoxDiagramEdge>) => {
      term(source, module); term(target, module);
      const id = `${source}|${kind}|${target}|${module}|${role ?? ""}`;
      if (!edges.some(edge => edge.id === id)) edges.push({ id, source, target, kind, module, role, wildcard: target === OWL_THING, ...extra });
    };
    const syntax = (expression: TBoxExpression, module: string): string => {
      if (expression.type === "named") { term(expression.id, module); return expression.id; }
      const id = `urn:atlas:tbox-expression:${JSON.stringify(expression)}`;
      if (!terms.has(id)) {
        const label = expression.type === "intersection" ? "AND" : expression.type === "union" ? "OR" : expression.type === "literal" ? JSON.stringify(expression.value) : expression.type === "restriction" ? `${compactTBoxId(expression.property)} ${expression.quantifier.toUpperCase()}` : expressionText(expression);
        terms.set(id, { id, label, kind: "expression", external: false, expression, x: 0, y: 0, width: 142, height: 54, selected: false, modules: [module] });
      } else term(id, module);
      if (expression.type === "intersection" || expression.type === "union") {
        expression.members.forEach((member, index) => add(id, syntax(member, module), "expression", module, `${expression.type === "intersection" ? "AND" : "OR"} member ${index + 1}`));
      } else if (expression.type === "restriction") {
        term(expression.property, module).kind = "property";
        add(id, expression.property, "expression", module, "onProperty");
        add(id, syntax(expression.target, module), "expression", module, expression.quantifier === "some" ? "someValuesFrom" : expression.quantifier === "only" ? "allValuesFrom" : "hasValue");
      } else if (expression.type === "literal" && expression.datatype) {
        add(id, expression.datatype, "expression", module, "literal datatype");
      }
      return id;
    };
    for (const item of data.subclasses) add(item.source, syntax(item.target, item.module), "subclass", item.module);
    for (const item of data.equivalences) add(item.source, syntax(item.expression, item.module), "equivalent", item.module, undefined, { predicate: "http://www.w3.org/2002/07/owl#equivalentClass" });
    for (const item of data.axioms ?? []) add(item.source, syntax(item.target, item.module), item.kind, item.module, undefined, { predicate: item.predicate });
    for (const item of data.properties) {
      item.domains.forEach(value => add(item.id, syntax(value, item.module), "domain", item.module));
      item.ranges.forEach(value => add(item.id, syntax(value, item.module), "range", item.module));
      const domain = item.domains[0], range = item.ranges[0];
      if (item.kind === "object" && item.domains.length === 1 && item.ranges.length === 1 && domain.type === "named" && range.type === "named") {
        // A labelled domain/range projection is schema notation, never a newly asserted instance triple.
        add(domain.id, range.id, "property", item.module, item.id, { propertyId: item.id, label: item.label, predicate: item.id, projection: true });
      }
    }
    const list = [...terms.values()].sort((left, right) => left.id.localeCompare(right.id));
    const cy = cytoscape({ headless: true, styleEnabled: false, layout: { name: "preset" }, elements: [
      ...list.map((node, index) => ({ data: { id: node.id }, position: { x: (index % 18) * 90, y: Math.floor(index / 18) * 90 } })),
      ...edges.filter(edge => !edge.projection).map(edge => ({ data: { id: edge.id, source: edge.source, target: edge.target } })),
    ] });
    try {
      cy.layout({ name: "cose", animate: false, randomize: false, numIter: 90, nodeRepulsion: () => 10000, idealEdgeLength: () => 110, edgeElasticity: () => 80, gravity: .12, componentSpacing: 180, nodeOverlap: 20, initialTemp: 100, coolingFactor: .95, minTemp: 1 } as cytoscape.CoseLayoutOptions).run();
      const positions = list.map(node => cy.getElementById(node.id).position());
      const left = Math.min(...positions.map(position => position.x)), top = Math.min(...positions.map(position => position.y));
      list.forEach((node, index) => { node.x = positions[index].x - left + 160; node.y = positions[index].y - top + 160; });
      cached = { nodes: list, edges, totalEdges: edges.length, omitted: 0, width: Math.max(...list.map(node => node.x)) + 160, height: Math.max(...list.map(node => node.y)) + 160 };
      fullSceneCache.set(data, cached);
    } finally { cy.destroy(); }
  }
  const initialAnchor = ATLAS_VOCAB + "MaterialEntity";
  const primary = new Set([selected || initialAnchor]);
  if (selected) {
    cached.edges.filter(edge => edge.source === selected || edge.target === selected).sort((a, b) => Number(b.kind === "property") - Number(a.kind === "property") || a.id.localeCompare(b.id)).slice(0, viewportWidth < 600 ? 8 : 14).forEach(edge => { primary.add(edge.source); primary.add(edge.target); });
  } else {
    // MaterialEntity has real BFO/DUL parents and the actual constitutedBy property connection.
    for (const edge of cached.edges.filter(edge => edge.source === initialAnchor || edge.target === initialAnchor)) {
      primary.add(edge.source); primary.add(edge.target);
      if (edge.propertyId) primary.add(edge.propertyId);
    }
  }
  const nodes = cached.nodes.map(node => ({ ...node, selected: node.id === selected, prominent: primary.has(node.id) }));
  const focus = nodes.filter(node => node.prominent), padding = 130;
  const left = Math.min(...focus.map(node => node.x)) - padding, top = Math.min(...focus.map(node => node.y)) - padding;
  const focusBounds = { left, top, width: Math.max(360, Math.max(...focus.map(node => node.x)) - left + padding), height: Math.max(320, Math.max(...focus.map(node => node.y)) - top + padding) };
  if (!selected) {
    const anchor = nodes.find(node => node.id === initialAnchor)!;
    focusBounds.width = Math.min(focusBounds.width, Math.max(360, viewportWidth * .95));
    focusBounds.height = Math.min(focusBounds.height, 420);
    focusBounds.left = anchor.x - focusBounds.width / 2;
    focusBounds.top = anchor.y - focusBounds.height / 2;
  }
  return { ...cached, nodes, focusBounds };
}

/** Separate directed axioms sharing endpoints; routing never changes their semantics. */
export function tboxEdgeRoutes(scene: Diagram) {
  const nodes = new Map(scene.nodes.map(node => [node.id, node]));
  const boundary = (node: TBoxDiagramNode, toward: TBoxDiagramNode) => {
    const dx = toward.x - node.x, dy = toward.y - node.y;
    const ratio = Math.min((node.width / 2 + 4) / Math.max(1, Math.abs(dx)), (node.height / 2 + 5) / Math.max(1, Math.abs(dy)));
    return { x: node.x + dx * ratio, y: node.y + dy * ratio };
  };
  return scene.edges.map(edge => {
    const source = nodes.get(edge.source)!, target = nodes.get(edge.target)!;
    if (source.id === target.id) {
      const loops = scene.edges.filter(item => item.source === edge.source && item.target === edge.target).sort((left, right) => left.id.localeCompare(right.id));
      const index = loops.findIndex(item => item.id === edge.id), angle = index / Math.max(1, loops.length) * Math.PI * 2;
      const radius = 70 + Math.floor(index / 8) * 16, nx = Math.cos(angle), ny = Math.sin(angle);
      const a = { x: source.x + nx * 16 - ny * 10, y: source.y + ny * 16 + nx * 10 }, b = { x: source.x + nx * 16 + ny * 10, y: source.y + ny * 16 - nx * 10 };
      return { edge, path: `M${a.x},${a.y} C${source.x + nx * radius - ny * 45},${source.y + ny * radius + nx * 45} ${source.x + nx * radius + ny * 45},${source.y + ny * radius - nx * 45} ${b.x},${b.y}`, label: { x: source.x + nx * radius * .8, y: source.y + ny * radius * .8 - 7 } };
    }
    const a = boundary(source, target), b = boundary(target, source);
    const parallels = scene.edges.filter(item => (item.source === edge.source && item.target === edge.target) || (item.source === edge.target && item.target === edge.source)).sort((left, right) => left.id.localeCompare(right.id));
    const lane = (parallels.findIndex(item => item.id === edge.id) - (parallels.length - 1) / 2) * 104;
    const dx = b.x - a.x, dy = b.y - a.y, length = Math.max(1, Math.hypot(dx, dy)), sign = edge.source < edge.target ? 1 : -1;
    const nx = -dy / length * sign, ny = dx / length * sign;
    return {
      edge,
      path: `M${a.x},${a.y} C${a.x + dx * .35 + nx * lane},${a.y + ny * lane} ${b.x - dx * .35 + nx * lane},${b.y + ny * lane} ${b.x},${b.y}`,
      label: { x: (a.x + b.x) / 2 + nx * lane * .75, y: (a.y + b.y) / 2 + ny * lane * .75 - 7 },
    };
  });
}

export async function loadTBox(signal?: AbortSignal): Promise<TBoxData> {
  const response = await fetch(pin.url, { cache: "force-cache", signal });
  if (!response.ok) throw new Error("tbox_artifact_unavailable");
  const bytes = await response.arrayBuffer();
  if (bytes.byteLength !== pin.bytes) throw new Error("tbox_artifact_integrity");
  const actualHash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map(value => value.toString(16).padStart(2, "0")).join("");
  if (actualHash !== pin.sha256) throw new Error("tbox_artifact_integrity");
  const data = JSON.parse(new TextDecoder().decode(bytes)) as TBoxData;
  if (data.schema !== pin.schema || data.version !== pin.version || data.scope !== "authored_tbox_only" || data.activeStoreInference !== false || data.defaultBridge !== "bfo") throw new Error("invalid_tbox_artifact");
  return data;
}
