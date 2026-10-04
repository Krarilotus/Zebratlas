import assert from "node:assert/strict";
import { overviewSubject } from "../lib/zebra/overview-identity.ts";

const gene = { id: "HGNC:11444", label: "STXBP1", kind: "gene", matched: true };
const disease = { id: "MONDO:0012812", label: "Developmental and epileptic encephalopathy, 4", kind: "disease", matched: false };
const data = { interpretation: { entities: [gene, disease] }, graph: { nodes: [gene, disease], edges: [] }, execution: {}, plan: { focus: [gene.id] } };
assert.deepEqual(overviewSubject(data), { id: gene.id, label: gene.label, kind: "gene" });
assert.equal(overviewSubject({ ...data, plan: { focus: [gene.id, disease.id] } }), null);
assert.equal(overviewSubject({ ...data, plan: undefined }), null);
assert.equal(overviewSubject({ ...data, retrieval: { scope: "name_candidates_only" } }), null);
assert.equal(overviewSubject({ ...data, plan: { focus: ["HGNC:9999999"] } }), null);
assert.equal(overviewSubject({ ...data, graph: { nodes: [{ ...gene, kind: "researcher" }] } }), null);
assert.deepEqual(overviewSubject({ ...data, query_execution: { semantic_focus: [disease.id] } }), { id: disease.id, label: disease.label, kind: "disease" });
assert.equal(overviewSubject(null), null);
console.log("Overview subject: 8 identity and ambiguity checks passed.");
