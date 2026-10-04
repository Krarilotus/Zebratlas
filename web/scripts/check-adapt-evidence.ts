import assert from "node:assert/strict";
import { card, connections, related } from "../lib/adapt.ts";
import { sourceDates } from "../lib/zebra/normalize.ts";

const study = (id: string, hash: string, status: string) => ({
  id, who: { id, label: `Study ${id}` }, kind: "trial", match: "exact", checked_on: "2025-01-01",
  status: { status, sponsor: `Sponsor ${id}` },
  reach: { kind: "ctgov_contacts", url: `https://clinicaltrials.gov/study/${id}` },
  why: { text: "The official registry names this condition.", facts: [{
    key: `${id}|studies_condition|MONDO:fixture`, edge_id: `${id}|studies_condition|MONDO:fixture`,
    text: "Names condition", source: "ClinicalTrials.gov", kind: "observed",
    records: [{ record: `${id}-record`, url: `https://clinicaltrials.gov/study/${id}`, sha256: hash,
      retrieved_at: "2024-12-01", references: [id] }],
    contradicted_by: [{ source: "Independent review", record: `${id}-contrary`, references: ["PMID:fixture"],
      quote: "The condition attribution was disputed.", sha256: "contrary-hash", date: "2024-12-02" }],
  }] },
  limits: ["Eligibility must be confirmed by the study team."], conflicts: ["Condition attribution disputed."],
});
const a = study("NCT00000001", "hash-a", "RECRUITING");
const b = study("NCT00000002", "hash-b", "COMPLETED");
const result = connections({ cards: [a, b] }, "MONDO:fixture")!;
assert.equal(result.exact.length, 2);
assert.equal(result.exact[0].status, "recruiting");
assert.equal(result.exact[1].status, "completed");
assert.equal(result.exact[0].sources[0].sha256, "hash-a");
assert.equal(result.exact[1].sources[0].sha256, "hash-b");
assert.notEqual(result.exact[0].sources[0].url, result.exact[1].sources[0].url);
assert.equal(result.exact[0].edges[0].evidence[0].record, "NCT00000001-record");
assert.equal(result.exact[0].edges[0].evidence[0].references[0], "NCT00000001");
assert.equal(result.exact[0].edges[0].kind, "observed");
assert.equal(result.exact[0].edges[0].contradicted_by?.[0].sha256, "contrary-hash");
assert.equal(result.exact[0].limits?.[0], a.limits[0]);
assert.equal(result.exact[0].conflicts?.[0], a.conflicts[0]);

// Multiple records for one assertion retain their own attribution; the parent's first-record
// hash/time must not leak into a secondary record with missing metadata.
const multi = card({ id: "atlasorg:fixture", kind: "organisation", why: { facts: [{
  key: "assertion:fixture", source: "Parent", sha256: "parent-first-record", retrieved_at: "2020-01-01",
  records: [{ record: "independent-a", source: "Source A", sha256: "independent-hash-a", url: "https://example.org/a" },
    { record: "independent-b", source: "Source B", url: "https://example.org/b" }],
}] } })!;
assert.equal(multi.sources.length, 2);
assert.notEqual(multi.sources[0].id, multi.sources[1].id);
assert.equal(multi.sources[1].sha256, undefined);
assert.equal(multi.sources[1].retrieved_on, "");
assert.equal(multi.sources[1].source, "Source B");

const community = related({ items: [{ neighbour: { id: "MONDO:neighbour", kind: "disease", label: "Neighbour" },
  why: ["Related source-backed mechanism"], assets: { top: [a, b], total: 5, exact_counts: { trial: 5 } },
  limits: ["A research lead, not treatment evidence."],
  mechanism: { effect_conflicts: [{ why: "Opposite reported variant effects.", opposite: true }] },
}] }, "MONDO:fixture")!.communities[0];
assert.equal(community.assets.length, 2);
assert.equal(community.assets[1].sources[0].sha256, "hash-b");
assert.equal(community.returned?.partial, true);
assert.equal(community.returned?.total, 5);
assert.equal(community.conflicts?.[0].text, "Opposite reported variant effects.");
assert.equal(community.limits?.[0].text, "A research lead, not treatment evidence.");

// Older card payloads lack kind and record metadata: retain the path without inventing proof.
const old = card({ id: "NCT00000003", why: { facts: [{ key: "old", edge_id: "NCT00000003|studies_condition|MONDO:fixture", source: "ctgov", text: "Old fact" }] } })!;
assert.equal(old.sources[0].retrieved_on, "");
assert.equal(old.edges[0].kind, undefined);
assert.equal(card({}), null);
assert.deepEqual(connections({ cards: [{}] }, "MONDO:fixture")?.exact, []);

// Exercise the real post-adapter cleaner, including assertion-scoped repeated record identifiers.
const rawDates = { facts: [{ key: "assertion:a", retrieved_at: "2020-01-01", records: [
  { record: "same-local-id", retrieved_at: "2024-01-02" }, { record: "undated" },
] }, { key: "assertion:b", records: [{ record: "same-local-id", fetched_at: "2024-03-04" }] }] };
const normalized = sourceDates({ sources: [
  { id: "assertion:a#same-local-id", assertion: "assertion:a", record: "same-local-id", retrieved_on: "today-must-not-survive" },
  { id: "assertion:b#same-local-id", assertion: "assertion:b", record: "same-local-id", retrieved_on: "" },
  { id: "assertion:a#undated", assertion: "assertion:a", record: "undated", retrieved_on: "2020-01-01" },
  { id: "unknown", retrieved_on: "today-must-not-survive" },
] }, rawDates);
assert.deepEqual(normalized.sources.map(source => source.retrieved_on), ["2024-01-02", "2024-03-04", "", ""]);
assert.equal(sourceDates(result, [a, b]).exact[0].sources[0].retrieved_on, "2024-12-01");
console.log("PASS: independent study provenance, contrary evidence, missing metadata, bounded counts and older payloads");
