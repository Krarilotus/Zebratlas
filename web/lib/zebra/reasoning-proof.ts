const hierarchy = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const base = "https://w3id.org/rare-disease-atlas/id/";
type ObjectValue = Record<string, unknown>;
const object = (value: unknown): ObjectValue | null => value && typeof value === "object" && !Array.isArray(value) ? value as ObjectValue : null;
const text = (value: unknown) => typeof value === "string" ? value : null;
const binding = (value: unknown) => text(object(value)?.value);
export type ProofStep = { subject: string; object: string; origin: "asserted" | "inferred"; rule: string | null };
export type ProofRecord = { id: string; locator: string; hash: string; sourceHash: string; source: string; url: string | null; version: string | null; retrieved: string | null };
export type VerifiedReasoningProof = { origin: "asserted" | "inferred"; steps: ProofStep[]; records: ProofRecord[] };

/** Display only the complete, verified ontology proof returned by the nrese adapter. */
export function normalizeReasoningProof(value: unknown): VerifiedReasoningProof | null {
  const input = object(value), justification = object(input?.justification), proof = object(input?.proof);
  if (!input || input.engine !== "nrese" || input.rule !== "rdfs-subclass-transitivity" || input.relation !== "subclass_of"
    || justification?.verified !== true || justification.complete !== true || !Array.isArray(proof?.steps)
    || !proof.steps.length || proof.steps.length > 64 || !Array.isArray(input.premise_records) || !input.premise_records.length) return null;
  const steps: ProofStep[] = [];
  const rawSteps = proof.steps;
  for (const raw of rawSteps) {
    const step = object(raw), subject = text(step?.subject), target = text(step?.object);
    if (!step || step.predicate !== hierarchy || !subject?.startsWith(base) || !target?.startsWith(base)
      || (step.origin !== "asserted" && step.origin !== "inferred")) return null;
    if (step.origin === "inferred" && (!text(step.rule) || !Array.isArray(step.premises) || !step.premises.length
      || step.premises.some(index => !Number.isInteger(index) || Number(index) < 0 || Number(index) >= rawSteps.length))) return null;
    steps.push({ subject, object: target, origin: step.origin, rule: text(step.rule) });
  }
  if (input.origin !== steps[0].origin) return null;
  const records: ProofRecord[] = [];
  for (const raw of input.premise_records) {
    const premise = object(raw);
    if (!Array.isArray(premise?.records) || !premise.records.length) return null;
    for (const rawRecord of premise.records) {
      const row = object(rawRecord);
      const id = binding(row?.record), locator = binding(row?.locator), hash = binding(row?.hash), sourceHash = binding(row?.sourceHash), source = binding(row?.source);
      if (!id || !locator || !hash || !sourceHash || !source) return null;
      if (records.length < 16) records.push({ id, locator, hash, sourceHash, source, url: binding(row?.url), version: binding(row?.version), retrieved: binding(row?.retrieved) });
    }
  }
  return { origin: steps[0].origin, steps, records };
}

export function reasoningTermLabel(term: string, labels?: ReadonlyMap<string, string>): string {
  let id = term;
  if (term.startsWith(base)) {
    try { id = decodeURIComponent(term.slice(base.length)); } catch { /* Keep the exact IRI. */ }
  }
  return labels?.get(id) || labels?.get(term) || id;
}

export function proofSourceUrl(value: string | null): string | null {
  if (!value) return null;
  try { const url = new URL(value); return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : null; }
  catch { return null; }
}
