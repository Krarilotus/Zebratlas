// Public resource contracts. Keep access and provenance separate: a record page is not a
// request form, a saved checksum is not a live availability check, and missing routes stay missing.
import type { Message, NodeRef, SourceRecord } from "./types";

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => typeof v === "object" && v !== null && !Array.isArray(v) ? v as Obj : {};
const rows = (v: unknown): Obj[] => Array.isArray(v) ? v.map(obj) : [];
const text = (v: unknown): string | undefined => typeof v === "string" && v.length > 0 ? v : undefined;
const message = (v: unknown): Message | undefined => typeof v === "string" ? v : text(obj(v).key) && text(obj(v).fallback) ? v as Message : undefined;

export function publicUrl(v: unknown): string | undefined {
  if (typeof v !== "string") return undefined;
  try {
    const url = new URL(v);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? v : undefined;
  } catch { return undefined; }
}

export type SubjectCoverage = {
  status: "cached_evidence" | "outside_verified_neighbourhood";
  message?: Message;
  discovery: { source: string; url: string }[];
};

export function subjectCoverage(raw: unknown): SubjectCoverage | undefined {
  const r = obj(raw);
  if (r.status !== "cached_evidence" && r.status !== "outside_verified_neighbourhood") return undefined;
  return {
    status: r.status, message: message(r.message),
    // POST endpoints and key-required APIs cannot be offered as browser search links.
    discovery: rows(r.discovery_routes).flatMap(d => {
      const url = publicUrl(d.url), source = text(d.source);
      return url && source && (!d.method || d.method === "GET") && d.requires_api_key !== true ? [{ source, url }] : [];
    }),
  };
}

export type ResourceRoute = { route: string; url: string; note?: string };
export type Resource = {
  id: string; title: string; kind: string; holder?: string;
  match: "exact" | "gene" | "condition" | "ortholog" | "related";
  access?: { route: string; url?: string; note?: string; alternatives: ResourceRoute[]; qualifiers: Obj; reuseLicence?: string };
  facts: { key: string; value: string }[];
  sources: SourceRecord[]; readingSource?: SourceRecord;
  evidence?: string; verifyId?: string;
  licence?: { id?: string; release?: string };
};
export type ResourceJob = {
  job: string; total: number; items: Resource[];
  searched: { source: string; date: string; checked?: boolean; status: string }[];
};
export type JobsResponse = { jobs: ResourceJob[]; coverage?: SubjectCoverage };

function source(v: unknown, resourceId: string): SourceRecord | undefined {
  const s = obj(v), name = text(s.name) ?? text(s.source);
  if (!name) return undefined;
  return {
    id: resourceId, source: name, url: publicUrl(s.url),
    locator: text(s.file) ?? text(s.locator) ?? text(s.record_locator),
    retrieved_on: text(s.retrieved_at) ?? text(s.retrieved_on) ?? "",
    sha256: text(s.sha256), version: text(s.version), tier: "moderate",
  };
}

function resource(r: Obj): Resource | undefined {
  const id = text(r.id), what = obj(r.what), how = obj(r.how_to_get);
  if (!id) return undefined;
  const sources = [source(r.source, id)].filter((s): s is SourceRecord => !!s);
  // Only accept the backend's canonical evidence route for this exact record.
  const evidence = r.evidence === `/api/funding/${encodeURIComponent(id)}/evidence` && sources.length ? id : undefined;
  return {
    id, title: text(what.label) ?? id, kind: text(what.kind) ?? "resource", holder: text(obj(r.holder).name),
    match: ["exact", "gene", "condition", "ortholog"].includes(String(r.match)) ? r.match as Resource["match"] : "related",
    access: text(how.route) ? {
      route: String(how.route), url: publicUrl(how.url), note: text(how.note),
      alternatives: rows(how.routes).flatMap(a => {
        const url = publicUrl(a.url), route = text(a.route);
        return url && route ? [{ route, url, note: text(a.note) }] : [];
      }), qualifiers: obj(how.qualifiers), reuseLicence: text(how.full_text_reuse_licence),
    } : undefined,
    facts: rows(r.facts).flatMap(f => text(f.key) && text(f.value) ? [{ key: String(f.key), value: String(f.value) }] : []),
    sources, readingSource: source(r.reading_source, id), evidence,
    verifyId: r.verify === `/api/verify/${encodeURIComponent(id)}` ? id : undefined,
    licence: text(obj(r.licence).id) || text(obj(r.licence).release) ? { id: text(obj(r.licence).id), release: text(obj(r.licence).release) } : undefined,
  };
}

export function jobs(raw: unknown): JobsResponse | null {
  const r = obj(raw);
  if (!Array.isArray(r.jobs)) return null;
  return {
    coverage: subjectCoverage(r.coverage),
    jobs: rows(r.jobs).map(j => ({
      job: text(j.job) ?? "", total: typeof j.total === "number" ? j.total : 0,
      items: rows(j.items).map(resource).filter((i): i is Resource => !!i),
      searched: rows(j.searched).map(s => ({
        source: text(s.label) ?? text(s.source) ?? "", date: text(s.retrieved_at) ?? "",
        checked: typeof s.checked_for_subject === "boolean" ? s.checked_for_subject : undefined,
        status: text(s.status) ?? "unknown",
      })),
    })),
  };
}

/** Older servers: use only questions.models, never the separate neighbouring-gene models. */
export function questionModels(raw: unknown, genes: NodeRef[]): Resource[] {
  const causal = new Set(genes.map(g => g.label));
  const seen = new Set<string>();
  return rows(obj(raw).questions).flatMap(q => rows(q.models).flatMap(m => {
    const original = text(m.id), id = original?.replace(/^Cellosaurus:/, "CVCL:");
    if (!id || !causal.has(String(m.gene)) || seen.has(id)) return [];
    seen.add(id);
    const verifyId = m.verify === `/api/verify/${encodeURIComponent(id)}` ? id : undefined;
    return [{
      id, title: text(m.title) ?? id, kind: text(m.asset_type) ?? "model", match: "gene" as const,
      access: publicUrl(m.url) ? { route: "official_page", url: publicUrl(m.url), alternatives: [], qualifiers: {} } : undefined,
      facts: [{ key: "gene", value: String(m.gene) }],
      sources: rows(m.sources).map(s => source(s, id)).filter((s): s is SourceRecord => !!s), verifyId,
    }];
  }));
}

export type FundingEvidence = {
  id: string; title: string; organisation?: string; agency?: string; genes: string[];
  projectNumbers: string[]; years: number[]; url?: string;
  cachedVerified: boolean; checks: { sha256: string; locator: string; matches: boolean }[];
};
export function fundingEvidence(raw: unknown, id: string): FundingEvidence | null {
  const r = obj(raw);
  if (r.id !== id) return null;
  const checks = rows(r.api_response_checks).map(c => ({ sha256: text(c.sha256) ?? "", locator: text(c.record_locator) ?? "", matches: c.matches === true }));
  return {
    id, title: text(r.title) ?? id,
    organisation: text(r.organisation) ?? text(obj(r.organisation).name),
    agency: text(r.agency) ?? text(obj(r.agency).name) ?? text(obj(r.agency).code),
    genes: Array.isArray(r.gene) ? r.gene.filter((g): g is string => typeof g === "string") : text(r.gene) ? [String(r.gene)] : [],
    projectNumbers: Array.isArray(r.project_nums) ? r.project_nums.filter((p): p is string => typeof p === "string") : [],
    years: Array.isArray(r.fiscal_years) ? r.fiscal_years.filter((y): y is number => typeof y === "number") : [],
    url: publicUrl(r.source_url), cachedVerified: r.cached_api_responses_verified === true && checks.length > 0 && checks.every(c => c.matches), checks,
  };
}
