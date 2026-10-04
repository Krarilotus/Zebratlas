import assert from "node:assert/strict";
import { normalizeReasoningProof, reasoningTermLabel, proofSourceUrl } from "../lib/zebra/reasoning-proof.ts";
const base = "https://w3id.org/rare-disease-atlas/id/";
const term = (id: string) => base + encodeURIComponent(id);
const step = (subject: string, object: string, origin: string, extra = {}) => ({ subject: term(subject), predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf", object: term(object), origin, ...extra });
const binding = (value: string) => ({ type: "literal", value });
const fixture = {
  engine: "nrese", rule: "rdfs-subclass-transitivity", relation: "subclass_of", origin: "inferred",
  justification: { verified: true, complete: true },
  proof: { steps: [step("MONDO:fixture-child", "MONDO:fixture-ancestor", "inferred", { rule: "prp-trp", premises: [1, 2] }), step("MONDO:fixture-child", "MONDO:fixture-parent", "asserted"), step("MONDO:fixture-parent", "MONDO:fixture-ancestor", "asserted")] },
  premise_records: [{ records: [{ record: binding("fixture-record"), locator: binding("fixture-locator"), hash: binding("fixture-hash"), sourceHash: binding("fixture-source-hash"), source: binding("fixture-source") }] }],
};
assert.equal(normalizeReasoningProof(fixture)?.origin, "inferred");
assert.equal(normalizeReasoningProof(fixture)?.steps.length, 3);
assert.equal(normalizeReasoningProof({ ...fixture, justification: { verified: false, complete: true } }), null);
assert.equal(normalizeReasoningProof({ ...fixture, justification: { verified: true, complete: false } }), null);
assert.equal(normalizeReasoningProof({ ...fixture, premise_records: [] }), null);
assert.equal(normalizeReasoningProof({ ...fixture, engine: "model" }), null);
assert.equal(normalizeReasoningProof({ ...fixture, proof: { steps: [step("MONDO:x", "MONDO:y", "inferred", { rule: "prp-trp", premises: [99] })] } }), null);
assert.equal(normalizeReasoningProof(null), null);
assert.equal(reasoningTermLabel(term("MONDO:1"), new Map([["MONDO:1", "Known ontology label"]])), "Known ontology label");
assert.equal(reasoningTermLabel(term("HP:1")), "HP:1");
assert.equal(proofSourceUrl("javascript:alert(1)"), null);
assert.equal(proofSourceUrl("https://user:password@example.org"), null);
assert.equal(proofSourceUrl("https://example.org/source"), "https://example.org/source");
console.log("PASS: complete verified proof guard, bounded premise steps, label mapping and safe source links");
