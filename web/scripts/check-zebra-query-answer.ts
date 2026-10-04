import assert from "node:assert/strict";
import { answerNodeId, answerUriLabel, executedQueryTexts, hasRenderableAnswer, isGraphOnlyTable, normalizeQueryAnswer, safeAnswerHref } from "../lib/zebra/query-answer.ts";

const literal = (value: string) => ({ type: "literal", value, datatype: "http://www.w3.org/2001/XMLSchema#integer" });
const uri = (value: string) => ({ type: "uri", value });
const envelope = (data: unknown, extra: Record<string, unknown> = {}) => ({
  answer: { plan: null, results: [{ backend: "nrese", data, provenance: [], query: "SELECT ?count WHERE {}", truncated: false, notes: [], ...extra }], model_provenance: [], tool_trace: [], latency_ms: 7 },
  semantic_focus: [], activity: { "@id": "urn:atlas:search-query:fixture" },
});
const table = (vars: string[], bindings: unknown[]) => ({ head: { vars }, results: { bindings } });

// Zero is an actual aggregate answer, even when no graph node exists.
const zero = envelope(table(["count"], [{ count: literal("0") }]));
assert.equal(hasRenderableAnswer(zero), true);
const zeroData = normalizeQueryAnswer(zero)!.results[0];
assert.equal(zeroData.kind, "table");
if (zeroData.kind === "table") assert.equal(zeroData.rows[0][0]?.value, "0");

// Numeric lexical values larger than JavaScript's integer precision remain untouched.
const precise = normalizeQueryAnswer(envelope(table(["count"], [{ count: literal("9007199254740993123456789") }])))!.results[0];
if (precise.kind === "table") assert.equal(precise.rows[0][0]?.value, "9007199254740993123456789");

const nonNode = normalizeQueryAnswer(envelope(table(["source", "checked", "optional"], [{ source: uri("https://example.org/source/1"), checked: { type: "literal", value: "2026-10-04" } }])))!.results[0];
assert.equal(nonNode.kind, "table");
if (nonNode.kind === "table") {
  assert.equal(nonNode.rows[0][2], null);
  assert.equal(isGraphOnlyTable(nonNode, new Map()), false);
}

const geneUri = "https://w3id.org/rare-disease-atlas/id/HGNC%3A11444";
assert.equal(answerNodeId(geneUri), "HGNC:11444");
assert.equal(answerUriLabel(geneUri, new Map([["HGNC:11444", "STXBP1"]])), "STXBP1");
assert.equal(answerUriLabel(geneUri), "HGNC:11444");
const represented = normalizeQueryAnswer(envelope(table(["node", "label"], [{ node: uri(geneUri), label: { type: "literal", value: "STXBP1" } }])))!.results[0];
if (represented.kind === "table") assert.equal(isGraphOnlyTable(represented, new Map([["HGNC:11444", "STXBP1"]])), true);
assert.equal(answerUriLabel("http://www.w3.org/ns/prov#Entity"), "prov:Entity");
assert.equal(answerUriLabel("https://unknown.example/name%20one"), "https://unknown.example/name%20one");
assert.equal(answerNodeId("https://w3id.org/rare-disease-atlas/id/%zz"), undefined);
assert.equal(safeAnswerHref("javascript:alert(1)"), undefined);
assert.equal(safeAnswerHref("https://user:pass@example.org"), undefined);
assert.equal(safeAnswerHref("/local/path"), undefined);
assert.equal(safeAnswerHref("https://example.org/evidence"), "https://example.org/evidence");

const bounded = normalizeQueryAnswer(envelope(table(Array.from({ length: 12 }, (_, i) => `v${i}`), Array.from({ length: 45 }, () => ({ v0: literal("1") })))))!.results[0];
if (bounded.kind === "table") {
  assert.equal(bounded.rows.length, 40); assert.equal(bounded.columns.length, 10);
  assert.equal(bounded.totalRows, 45); assert.equal(bounded.totalColumns, 12); assert.equal(bounded.truncated, true);
}
assert.equal(normalizeQueryAnswer(envelope({ boolean: false }))!.results[0].kind, "boolean");
assert.equal(hasRenderableAnswer(envelope({ boolean: false })), true);
assert.equal(hasRenderableAnswer(envelope(table(["result"], []))), true);
assert.equal(hasRenderableAnswer({ status: "unsupported" }), true);
assert.equal(hasRenderableAnswer({ status: "not_executed", answer: zero.answer }), true);
const notExecuted = normalizeQueryAnswer({ status: "not_executed", question_status: "not_executed", answer: { results: [], tool_trace: [{ stage: "query_status", status: "not_executed", reason: "deadline_exceeded", successful_queries: 0 }] } })!;
assert.equal(notExecuted.status, "not_executed");
assert.equal(notExecuted.results.length, 0, "A failed query is not an executed zero-row result");
assert.equal(normalizeQueryAnswer({ status: "not_executed", answer: zero.answer })!.results.length, 0, "Failure status cannot reuse a stale answer");
const partial = normalizeQueryAnswer({ ...zero, status: "executed", question_status: "partial" })!;
assert.equal(partial.questionStatus, "partial");
assert.equal(partial.results[0].kind, "table");
const tracePartial = normalizeQueryAnswer({ ...zero, answer: { ...zero.answer, tool_trace: [{ stage: "query_status", status: "partial", reason: "budget_exhausted", successful_queries: 1 }] } })!;
assert.equal(tracePartial.questionStatus, "partial");
assert.equal(tracePartial.results.length, 1, "Retain actual successful query result under partial question coverage");
assert.equal(normalizeQueryAnswer({ ...zero, status: "executed" })!.questionStatus, undefined, "Executed empty or zero results are not partial without actual backend status");
assert.deepEqual(executedQueryTexts({ execution: { engine: "none", sparql: "SELECT ?attempted WHERE {}" }, query_execution: { status: "not_executed", answer: { results: [] } } }), [], "A failed query must not appear as an executed query");
assert.deepEqual(executedQueryTexts({ execution: { engine: "nrese" }, query_execution: { ...zero, status: "executed", question_status: "partial" } }), ["SELECT ?count WHERE {}"], "Partial coverage retains exact successful query text");
assert.deepEqual(executedQueryTexts({ execution: { engine: "indexed-atlas", sparql: "SELECT ?legacy WHERE {}" } }), ["SELECT ?legacy WHERE {}"], "Preserve actual legacy executed query fallback");
assert.deepEqual(executedQueryTexts({ execution: { engine: "nrese", sparql: "SELECT ?first WHERE {}", queries: [
  { engine: "nrese", stage: "neighborhood", sparql: "SELECT ?first WHERE {}" },
  { engine: "nrese", stage: "frontier", sparql: "SELECT ?second WHERE {}" },
  { engine: "none", sparql: "SELECT ?unexecuted WHERE {}" },
] } }), ["SELECT ?first WHERE {}", "SELECT ?second WHERE {}"], "Separate actual store requests remain individually inspectable and runnable");
assert.deepEqual(executedQueryTexts({ execution: { engine: "nrese", sparql: "SELECT ?stale WHERE {}", queries: [] } }), [], "An explicit empty execution list must not expose stale fallback text");
assert.equal(hasRenderableAnswer({ status: "executed", ...zero }), true);
assert.equal(hasRenderableAnswer({ status: "executed", answer: zero }), true);
assert.equal(hasRenderableAnswer({ answer: {} }), true); // Render the malformed-answer state, never claim zero matches.
assert.equal(hasRenderableAnswer(envelope(table(["count"], [{ count: { type: "literal", value: 4 } }]))), true); // Explicit invalid state with zero valid rows, never a fabricated "4".
assert.equal(normalizeQueryAnswer(envelope(table(["count"], [{ count: { type: "literal", value: 4 } }])))!.status, "invalid");
assert.equal(normalizeQueryAnswer(envelope({ head: { vars: ["a", "a"] }, results: { bindings: [] } }))!.status, "invalid");
console.log("Zebra query answer: aggregate zero, lexical precision, non-node rows, URI safety, bounds and malformed/status cases passed.");
