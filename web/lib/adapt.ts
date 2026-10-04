// Maps atlas-server JSON (crates/atlas-server/src/{journeys,cards,resolve,checks}.rs) onto the
// view model in lib/types.ts. Raw shapes are typed loosely on purpose: the server is evolving, and
// a missing field must degrade one detail, never a whole screen. Only lib/api.ts imports this.
import "server-only";
import { subjectCoverage } from "./resources.ts";
export { jobs, questionModels, fundingEvidence } from "./resources.ts";

import type {
  Asset,
  Bridge,
  Channel,
  ChannelKind,
  Connection,
  ConnectionKind,
  ConnectionsResponse,
  Counterexample,
  Coverage,
  Edge,
  EdgeKind,
  Evidence,
  GapsResponse,
  IntegrityResponse,
  LlmCall,
  MatchVia,
  MessageRef,
  MessageResponse,
  NodeRef,
  ProvChain,
  ProvEntity,
  RelatedCommunity,
  ResearchQuestion,
  RelatedResponse,
  ResolveResponse,
  Sentence,
  SourceRecord,
  Strength,
  StudyStatus,
  SummaryResponse,
  Tier,
  Validator,
  VerifyResult,
  Work,
} from "@/lib/types";

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const objs = (v: unknown): Obj[] => arr(v).filter(isObj);
const str = (v: unknown): string | undefined => (typeof v === "string" && v.length > 0 ? v : undefined);
const strs = (v: unknown): string[] => arr(v).filter((x): x is string => typeof x === "string");
const num = (v: unknown): number | undefined => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const today = () => new Date().toISOString().slice(0, 10);

function nodeRef(v: unknown): NodeRef | undefined {
  if (!isObj(v)) return undefined;
  const id = str(v.id);
  if (!id) return undefined;
  return { id, kind: (str(v.kind) ?? "disease") as NodeRef["kind"], label: str(v.label) ?? id, plain_label: str(v.plain_label) };
}

// --- shared pieces -------------------------------------------------------------------------

const CONNECTION_KINDS: ConnectionKind[] = [
  "patient_group",
  "registry",
  "natural_history",
  "trial",
  "observational",
  "expanded_access",
  "expert_centre",
  "researcher",
  "organisation",
  "grant",
];

function connectionKind(k: unknown): ConnectionKind {
  const s = str(k) ?? "organisation";
  return (CONNECTION_KINDS as string[]).includes(s) ? (s as ConnectionKind) : "organisation";
}

const CHANNEL: Record<string, ChannelKind> = {
  ctgov_contacts: "registry_record",
  reporter_project: "grant_page",
  orcid: "profile",
  pubmed_author: "profile",
  contact_form: "contact_form",
  website: "website",
  email: "email",
  institution_page: "institution_page",
};

function channels(reach: unknown): Channel[] {
  if (!isObj(reach)) return [];
  const url = str(reach.url);
  if (!url) return [];
  return [{ kind: CHANNEL[str(reach.kind) ?? ""] ?? (url.startsWith("mailto:") ? "email" : "website"), url }];
}

const STATUS: Record<string, StudyStatus> = {
  RECRUITING: "recruiting",
  NOT_YET_RECRUITING: "not_yet_recruiting",
  ACTIVE_NOT_RECRUITING: "active_not_recruiting",
  ENROLLING_BY_INVITATION: "enrolling_by_invitation",
  COMPLETED: "completed",
  TERMINATED: "terminated",
  WITHDRAWN: "withdrawn",
  SUSPENDED: "suspended",
};

function studyStatus(v: unknown): StudyStatus | undefined {
  const s = str(v);
  if (!s) return undefined;
  return STATUS[s.toUpperCase().replace(/[\s-]/g, "_")] ?? "unknown";
}

/** Official records weigh most; computed links (similarity, inference) least. */
function tierOf(source: string): Tier {
  const s = source.toLowerCase();
  if (/(clinicaltrials|ctgov|orphanet|omim|hpo|mondo|hgnc|reporter|euctr|ictrp|pubmed|registry|g2p|gene2phenotype)/.test(s)) return "high";
  if (/(curated|expert|directory|org)/.test(s)) return "moderate";
  return "low";
}

/** Public page of a graph node, where one exists (the record a reader can open). */
export function nodeUrl(id: string): string | undefined {
  const [prefix, local] = [id.slice(0, id.indexOf(":")), id.slice(id.indexOf(":") + 1)];
  if (/^NCT\d+$/.test(id)) return `https://clinicaltrials.gov/study/${id}`;
  switch (prefix) {
    case "PMID":
      return `https://pubmed.ncbi.nlm.nih.gov/${local}/`;
    case "ORCID":
      return `https://orcid.org/${local}`;
    case "MONDO":
      return `https://monarchinitiative.org/${id}`;
    case "HGNC":
      return `https://www.genenames.org/data/gene-symbol-report/#!/hgnc_id/${id}`;
    case "HP":
      return `https://hpo.jax.org/browse/term/${id}`;
    case "GO":
      return `https://amigo.geneontology.org/amigo/term/${id}`;
    case "G2P":
      return `https://www.ebi.ac.uk/gene2phenotype/lgd/${local}`;
    default:
      return id.startsWith("R-HSA-") ? `https://reactome.org/content/detail/${id}` : undefined;
  }
}

/**
 * The record behind an edge `from|relation|to`: the most specific end with a public page (a study,
 * paper or organisation page beats the condition). `own` is the card's own evidence page, used for
 * its organisation or grant ids, which have no public id pattern.
 */
function edgeUrl(edge: string, own?: string): string | undefined {
  const [from, , to] = edge.split("|");
  const ends = [from, to].filter(Boolean);
  for (const pick of [(x: string) => /^NCT\d+$/.test(x) || x.startsWith("PMID:"), (x: string) => x.startsWith("atlasorg:") || x.startsWith("REPORTER:")]) {
    const hit = ends.find(pick);
    if (hit) return nodeUrl(hit) ?? own;
  }
  for (const e of ends) {
    const u = nodeUrl(e);
    if (u) return u;
  }
  return own;
}

/** Facts ({key, text, edge_id, source}) become the source records of "How we know". */
function factSources(facts: unknown, _checkedOn: string, own?: string): SourceRecord[] {
  const seen = new Set<string>();
  return objs(facts)
    .flatMap((fact) => {
      const nested = [...objs(fact.records), ...objs(fact.evidence)];
      return (nested.length ? nested : [fact]).map((record) => {
      const f = record === fact ? fact : recordContext(fact, record);
      const assertion = str(fact.assertion) ?? str(fact.edge_id) ?? str(fact.key) ?? str(fact.id);
      const recordId = str(record.record) ?? str(record.id);
      const id = nested.length && assertion && recordId ? `${assertion}#${recordId}` : recordId ?? assertion ?? "";
      const source = str(f.source) ?? "";
      // The server's own record fields win (url, sha256, locator, retrieved_at); until it sends
      // them, the record page is derived from the edge's ids and the fingerprint stays one tap away.
      return {
        id,
        source: source || id,
        retrieved_on: str(f.retrieved_on) ?? str(f.retrieved_at) ?? str(f.fetched_at) ?? "",
        tier: (["high", "moderate", "low"].includes(str(f.tier) ?? "") ? str(f.tier) : tierOf(source)) as Tier,
        statement: str(f.statement) ?? str(f.text),
        statement_msg: messageRef(f.msg),
        statement_lang: str(f.display_lang) ?? "en",
        locator: str(f.locator) ?? str(f.edge_id),
        url: (/^https?:\/\//.test(str(f.url) ?? "") ? str(f.url) : undefined) ?? edgeUrl(assertion ?? id, nested.length ? undefined : own),
        sha256: str(f.sha256),
        record: recordId, assertion, kind: edgeKind(f.kind ?? f.edge_kind),
        references: strs(f.references), generated_by: str(f.generated_by),
        sha256_scope: str(f.sha256_scope), version: str(f.version), record_count: num(fact.records),
      };
      });
    })
    .filter((s) => {
      const key = JSON.stringify([s.id, s.source, s.url, s.locator, s.sha256, s.statement]);
      return Boolean(s.id) && !seen.has(key) && (seen.add(key), true);
    });
}

/** Parent assertion context is shared; a first record's hash, URL or timestamp is not. */
function recordContext(parent: Obj, record: Obj): Obj {
  return { source: parent.source, text: parent.text, edge_id: parent.edge_id, kind: parent.kind,
    edge_kind: parent.edge_kind, references: parent.references, generated_by: parent.generated_by, ...record };
}

function edgeKind(v: unknown): EdgeKind | undefined {
  return ["observed", "extracted", "inferred", "hypothesis"].includes(str(v) ?? "") ? v as EdgeKind : undefined;
}

function evidenceRecords(raw: unknown): Evidence[] {
  return objs(raw).flatMap((value) => {
    const nested = objs(value.records);
    return (nested.length ? nested.map((record) => recordContext(value, record)) : [value]).map((record) => ({
      source: str(record.source) ?? "", record: str(record.record) ?? str(record.id),
      references: strs(record.references), url: str(record.url), locator: str(record.locator),
      sha256: str(record.sha256), sha256_scope: str(record.sha256_scope), version: str(record.version),
      generated_by: str(record.generated_by), evidence_code: str(record.evidence_code), record_count: num(record.records),
      frequency: str(record.frequency), date: str(record.date) ?? str(record.retrieved_on) ?? str(record.retrieved_at) ?? str(record.fetched_at),
      confidence: num(record.confidence), quote: str(record.quote),
    }));
  });
}

function factEdges(raw: unknown): Edge[] {
  return objs(raw).flatMap((fact) => {
    const id = str(fact.edge_id) ?? str(fact.id) ?? str(fact.assertion);
    const parts = id?.split("|");
    const from = str(fact.from) ?? str(fact.source_id) ?? parts?.[0];
    const relation = str(fact.relation) ?? parts?.[1];
    const to = str(fact.to) ?? str(fact.target_id) ?? parts?.[2];
    if (!from || !relation || !to || (parts && parts.length !== 3 && !str(fact.relation))) return [];
    const records = [...objs(fact.records), ...objs(fact.evidence)];
    return [{ id, from, relation, to, kind: edgeKind(fact.kind ?? fact.edge_kind), why: str(fact.why) ?? str(fact.text),
      evidence: evidenceRecords(records.length ? records.map(record => recordContext(fact, record)) : [fact]),
      contradicted_by: evidenceRecords(fact.contradicted_by),
    }];
  });
}

function messageRef(value: unknown): MessageRef | undefined {
  return isObj(value) && typeof value.key === "string" && typeof value.fallback === "string"
    ? value as MessageRef : undefined;
}

function sentences(v: unknown, lang: string): Sentence[] {
  return objs(v).map((x) => ({ text: str(x.text) ?? "", msg: messageRef(x.msg), cites: strs(x.cites), lang: str(x.display_lang) ?? lang }));
}

function validator(v: unknown, checked: number): Validator | undefined {
  if (!isObj(v)) return undefined;
  const passed = v.passed === true;
  const notes = [str(v.fallback_reason), ...arr(v.attempts).flatMap((a) => strs(a))].filter((x): x is string => Boolean(x));
  return { ok: passed, checked, notes };
}

function llmCall(purpose: string, origin: unknown, activities: unknown): LlmCall {
  return { purpose, origin: str(origin) ?? "template", activities: strs(activities) };
}

// --- resolve -------------------------------------------------------------------------------

const VIA: Record<string, MatchVia> = {
  id: "id",
  name: "name",
  synonym: "alias",
  abbreviation: "alias",
  layperson: "alias",
  fuzzy: "typo",
  corrected: "typo",
  llm: "sentence",
  gene: "gene",
  phenotype: "symptom",
};

export function resolve(raw: unknown, q: string): ResolveResponse {
  const r = isObj(raw) ? raw : {};
  const status = str(r.status);
  const choices = objs(r.choices)
    .map((c) => {
      const condition = nodeRef(c.node);
      if (!condition) return null;
      const viaGene = str(c.via_gene);
      const via: MatchVia = viaGene ? (str(r.mention) && q.trim().split(/\s+/).length > 3 ? "sentence" : "gene") : (VIA[str(c.match_kind) ?? ""] ?? "alias");
      return { condition, via, matched: viaGene ?? str(c.matched) ?? condition.label, coverage: subjectCoverage(c.coverage) };
    })
    .filter((c): c is NonNullable<typeof c> => c !== null);
  const target = nodeRef(r.target);
  const understood = str(r.mention) ?? str(r.corrected);
  if (status === "resolved" && target) {
    const hit = choices.find((c) => c.condition.id === target.id);
    return { origin: "api", query: q, status: "one", choices: [hit ?? { condition: target, via: "name", matched: target.label, coverage: subjectCoverage(r.coverage) }], understood };
  }
  return { origin: "api", query: q, status: choices.length === 0 ? "none" : "choose", choices, understood };
}

// --- summary (/summary + /api/disease) -------------------------------------------------------

export function summary(id: string, rawSummary: unknown, rawDisease: unknown): SummaryResponse | null {
  const d = isObj(rawDisease) ? rawDisease : undefined;
  const s = isObj(rawSummary) ? rawSummary : undefined;
  const condition = nodeRef(s?.condition) ?? nodeRef(d?.node);
  if (!condition) return null;
  const label = condition.label.charAt(0).toUpperCase() + condition.label.slice(1);
  const checked = today();
  const sources: SourceRecord[] = s ? factSources(s.facts, checked) : [];

  // Only validated model text is plain enough for level 1; the template restates raw facts.
  const llm = str(s?.origin) === "llm";
  const what: Sentence[] = s && llm ? sentences(s.sentences, str(s.lang) ?? "en").slice(0, 3) : [];
  const lang = llm ? (str(s?.lang) ?? "en") : "en";

  const cases = objs(d?.prevalence).find((p) => str(p.kind) === "Cases/families" && num(p.mean_value));
  const n = cases ? Math.round(num(cases.mean_value) ?? 0) : 0;
  const pmids = cases ? strs(cases.pmids) : [];
  const notAloneSource: SourceRecord | undefined = cases
    ? {
        id: `${id}#prevalence`,
        source: "Orphanet",
        source_key: "know.sourceNames.orphanetEpidemiology",
        locator: str(cases.record) ?? pmids.join(", "),
        url: pmids[0] ? `https://pubmed.ncbi.nlm.nih.gov/${pmids[0].replace(/^PMID:/, "")}/` : undefined,
        retrieved_on: checked,
        tier: "high",
      }
    : undefined;

  const genes = objs(d?.genes)
    .map((g) => nodeRef(g.gene))
    .filter((g): g is NodeRef => Boolean(g));
  const synonyms = strs(d?.synonyms);
  return {
    origin: "api",
    lang,
    condition: { ...condition, label },
    what,
    generated: llm ? "llm" : "template",
    // Count of described people is a number, so the UI phrases it (translated) itself.
    not_alone: undefined,
    cases_described: n > 0 ? { n, source: notAloneSource as SourceRecord } : undefined,
    medical: {
      names: [label, ...synonyms.filter((x) => x.toLowerCase() !== label.toLowerCase()).slice(0, 5)],
      genes,
      terms: [],
      definition: str(d?.definition),
    },
    sources: [...sources, ...(notAloneSource ? [notAloneSource] : [])],
  };
}

// --- connections -----------------------------------------------------------------------------

export function card(raw: Obj): Connection | null {
  const who = isObj(raw.who) ? raw.who : {};
  const id = str(raw.id) ?? str(who.id);
  if (!id) return null;
  const status = isObj(raw.status) ? raw.status : {};
  const why = isObj(raw.why) ? raw.why : {};
  const llm = isObj(why.llm) ? why.llm : undefined;
  const checkedOn = str(raw.checked_on) ?? "";
  const reach = isObj(raw.reach) ? raw.reach : {};
  const evidence = objs(status.channels).map((c) => str(c.evidence_url)).find(Boolean) ?? str(reach.url);
  const sources = [...factSources(why.facts, checkedOn, evidence), ...factSources(raw.sources, checkedOn)];
  const level = str(raw.level);
  const exact = str(raw.match) === "exact";
  const countries = [...strs(status.countries), ...(str(status.country) ? [str(status.country) as string] : [])];
  const whyLang = llm ? (str(llm.lang) ?? "en") : str(why.display_lang) ?? "en";
  const whyText = (llm && str(llm.text)) || str(why.text) || "";
  return {
    id,
    kind: connectionKind(raw.kind),
    match: exact ? "exact" : level === "umbrella" || level === "broad" ? "broad" : "related",
    name: str(who.label) ?? id,
    countries: [...new Set(countries)].slice(0, 12),
    languages: strs(status.languages),
    why: { text: whyText, msg: llm ? undefined : messageRef(why.msg), cites: sources.map((s) => s.id), lang: whyLang },
    channels: channels(raw.reach),
    status: str(status.status) ? studyStatus(status.status) : undefined,
    sponsor: str(status.sponsor) ?? str(status.organisation),
    affiliation: strs(status.affiliations)[0],
    communities: status.cross_community === true ? strs(status.communities) : undefined,
    serves: [],
    sources,
    edges: [...factEdges(raw.edges), ...factEdges(why.facts)],
    edge_ids: strs(raw.edge_ids),
    checked_on: checkedOn,
    limits: [...strs(raw.limits), ...strs(why.limits)],
    conflicts: [...strs(raw.conflicts), ...strs(why.conflicts)],
    validator: llm ? validator(llm.validation, objs(llm.sentences).length) : undefined,
    llm: [llmCall("why_sentence", llm ? llm.origin : why.origin, llm?.activities)],
    placeholder: false,
  };
}

function coverage(raw: unknown): { list: Coverage[]; note?: string } {
  const c = isObj(raw) ? raw : {};
  const list = objs(Array.isArray(raw) ? raw : c.searched).map((s) => {
    const found = isObj(s.found) ? Object.values(s.found).reduce<number>((a, v) => a + (num(v) ?? 0), 0) : 0;
    return {
      source: str(s.label) ?? str(s.source) ?? "",
      searched_on: str(s.checked_on) ?? str(s.retrieved_at) ?? "",
      found,
      covered: s.checked_for_subject === false || s.covers_this_condition === false ? false : s.checked_for_subject === true || s.covers_this_condition === true ? true : undefined,
      note: str(s.note) ?? (str(s.status) && str(s.status) !== "loaded" ? str(s.status) : undefined),
      lang: str(s.display_lang) ?? "en",
    };
  });
  return { list, note: str(c.note) };
}

export function connections(raw: unknown, id: string): ConnectionsResponse | null {
  if (!isObj(raw)) return null;
  const condition = nodeRef(raw.condition) ?? { id, kind: "disease" as const, label: id };
  const cards = objs(raw.cards)
    .map((c) => card(c))
    .filter((c): c is Connection => c !== null);
  const cov = coverage(raw.coverage);
  const totals: ConnectionsResponse["totals"] = {};
  if (isObj(raw.counts))
    for (const [k, v] of Object.entries(raw.counts)) if (isObj(v)) totals[connectionKind(k)] = { exact: (totals[connectionKind(k)]?.exact ?? 0) + (num(v.exact) ?? 0), related: (totals[connectionKind(k)]?.related ?? 0) + (num(v.related) ?? 0) };
  return {
    origin: "api",
    lang: str(raw.display_lang) ?? "en",
    condition,
    totals,
    exact: cards.filter((c) => c.match === "exact"),
    related: cards.filter((c) => c.match !== "exact"),
    coverage: cov.list,
    coverage_note: cov.note,
    subject_coverage: subjectCoverage(raw.subject_coverage),
  };
}

// --- gaps ------------------------------------------------------------------------------------

export function gaps(raw: unknown, id: string): GapsResponse | null {
  if (!isObj(raw)) return null;
  const condition = nodeRef(raw.condition) ?? { id, kind: "disease" as const, label: id };
  const cov = coverage(raw.searched);
  return {
    origin: "api",
    lang: str(raw.display_lang) ?? "en",
    condition,
    searched: cov.list,
    subject_coverage: subjectCoverage(raw.subject_coverage),
    unknown: objs(raw.missing).map((m) => ({
      text: str(m.text) ?? [str(m.what), str(m.why)].filter(Boolean).join(": ") + (str(m.how_to_close) ? `. ${str(m.how_to_close)}.` : "."),
      msg: messageRef(m.msg),
      lang: str(m.display_lang) ?? "en",
      cites: [],
    })),
    questions: strs(raw.next_questions).map((text) => ({ text, cites: [], lang: str(raw.display_lang) ?? "en" })),
  };
}

// --- message ---------------------------------------------------------------------------------

export function message(raw: unknown, kind: "message" | "proposal"): MessageResponse | null {
  if (!isObj(raw)) return null;
  const lang = str(raw.lang) ?? "en";
  const cited = sentences(raw.sentences, lang);
  const facts = factSources(raw.facts, today());
  return {
    origin: "api",
    lang,
    subject: str(raw.subject) ?? "",
    body: str(raw.text) ?? "",
    sections: kind === "proposal" && cited.length > 0 ? [{ key: "why_you", sentences: cited }] : [],
    validator: validator(raw.validation, cited.length) ?? { ok: false, checked: 0, notes: [] },
    llm: llmCall(kind === "proposal" ? "partner_proposal" : "outreach_message", raw.origin, raw.activities),
    sources: facts,
  };
}

// --- verify, provenance, integrity -------------------------------------------------------------

export function verify(raw: unknown, id: string): VerifyResult {
  const r = isObj(raw) ? raw : {};
  const first = objs(r.records)[0] ?? objs(r.files)[0];
  return {
    origin: "api",
    id,
    state: r.verified === true ? "unchanged" : r.verified === false ? "changed" : "unreachable",
    checked_at: new Date().toISOString(),
    expected_sha256: str(first?.sha256) ?? str(first?.expected),
    actual_sha256: str(first?.actual) ?? str(first?.sha256),
    summary: str(r.summary),
  };
}

export function provenance(raw: unknown, id: string): ProvChain | null {
  if (!isObj(raw)) return null;
  const { entities, activities, ...rest } = raw;
  const isEdge = str(raw.kind) === "edge";
  const refId = (v: unknown) => (isObj(v) ? (str(v.id) ?? "") : (str(v) ?? ""));
  return {
    origin: "api",
    id,
    raw: rest,
    entities: objs(entities) as ProvEntity[],
    activities: objs(activities) as ProvChain["activities"],
    edge: isEdge
      ? {
          from: refId(raw.from),
          relation: str(raw.relation) ?? "",
          to: refId(raw.to),
          kind: str(raw.edge_kind),
          reason: str(raw.reason),
          records: objs(raw["prov:wasDerivedFrom"]).map((r) => ({
            record: str(r.record) ?? "",
            locator: str(r.locator),
            url: str(r.url) ?? null,
            fetched_at: str(r.fetched_at) ?? null,
            sha256: str(r.sha256) ?? null,
          })),
        }
      : undefined,
  };
}

export function integrity(raw: unknown, stats: unknown): IntegrityResponse | null {
  if (!isObj(raw)) return null;
  const nodes = isObj(raw.nodes) ? raw.nodes : {};
  const edges = isObj(raw.edges_by_kind) ? raw.edges_by_kind : {};
  const counts: Record<string, number> = {};
  for (const [k, v] of Object.entries(nodes)) if (num(v) !== undefined) counts[k] = num(v) as number;
  const edgeTotal = Object.values(edges).reduce<number>((a, v) => a + (num(v) ?? 0), 0);
  if (edgeTotal > 0) counts.edges = edgeTotal;
  const st = isObj(stats) ? stats : {};
  return {
    origin: "api",
    checked_at: new Date().toISOString(),
    counts,
    checks: objs(raw.contracts).map((c) => ({
      name: str(c.description) ?? str(c.id) ?? "",
      ok: c.passed === true,
      violations: num(c.violations) ?? 0,
      note: num(c.checked) !== undefined ? `${num(c.checked)}` : undefined,
    })),
    sources: objs(st.sources) as ProvEntity[],
  };
}

// --- related communities (/related: SimilarityIndex RelatedReport) -------------------------------

const ASSET_KINDS = new Set<ConnectionKind>(["registry", "natural_history", "trial", "observational", "expanded_access", "grant"]);
const STRENGTH: Record<string, Strength> = { same_mechanism: "strong", mechanism_only: "some", phenotype_only: "weak" };

function termSources(terms: NodeRef[], source: string, checked: string): SourceRecord[] {
  return terms.map((t) => ({ id: t.id, source, url: nodeUrl(t.id), locator: t.id, retrieved_on: checked, tier: "high" as const, statement: t.label }));
}

function upper(label: string) {
  return label.charAt(0).toUpperCase() + label.slice(1);
}

/** Shared processes and telling signs of one neighbour, strongest (highest information content) first. */
function sharedOf(n: Obj) {
  const mech = isObj(n.mechanism) ? n.mechanism : {};
  const phen = isObj(n.phenotype) ? n.phenotype : {};
  const processes = objs(mech.shared_processes)
    .map((p) => ({ id: str(p.id) ?? "", kind: "pathway" as const, label: str(p.name) ?? "" }))
    .filter((p) => p.id && p.label)
    .slice(0, 3);
  const symptoms = objs(phen.shared)
    .map((p) => nodeRef(p.term))
    .filter((p): p is NodeRef => Boolean(p))
    .slice(0, 3);
  const genes = arr(mech.shared_genes)
    .map((g) => (typeof g === "string" ? { id: g, kind: "gene" as const, label: g } : nodeRef(g)))
    .filter((g): g is NodeRef => Boolean(g));
  return { genes, processes, symptoms };
}

/**
 * Maps the similarity report onto communities (by mechanism + telling signs) and look-alikes
 * (similar signs, different cause). Scores stay in the "why" sentences; the UI shows names.
 */
export function related(raw: unknown, id: string): RelatedResponse | null {
  if (!isObj(raw)) return null;
  const items = objs(raw.items);
  const counter = objs(raw.counterexamples);
  if (items.length === 0 && counter.length === 0) return null;
  const checked = str(raw.checked_on) ?? "";
  const prov = isObj(raw.provenance) && isObj(raw.provenance.activity) ? raw.provenance.activity : {};
  const computed = (n: NodeRef, why: string[]): SourceRecord => ({
    id: `${str(prov.id) ?? "activity:similarity-related"}#${n.id}`,
    source: str(prov.label) ?? str(prov.id) ?? "",
    source_key: str(prov.label) ? undefined : "know.sourceNames.similarity",
    locator: str(prov.id),
    url: nodeUrl(n.id),
    retrieved_on: str(prov.ended_at) ?? checked,
    tier: "low",
    statement: why.join(" · "),
  });
  const communities: RelatedCommunity[] = items
    .map((it): RelatedCommunity | null => {
      const condition = nodeRef(it.neighbour);
      if (!condition) return null;
      const shared = sharedOf(it);
      const why = strs(it.why);
      // The neighbour's own top connections (card-shaped): its group, its studies, its people.
      const cards = objs(isObj(it.assets) ? it.assets.top : [])
        .map((c) => card(c))
        .filter((c): c is Connection => c !== null);
      const top = objs(isObj(it.assets) ? it.assets.top : []);
      const assets: Asset[] = cards
        .filter((c) => ASSET_KINDS.has(c.kind))
        .map((c) => {
          const raw = top.find((x) => str(x.id) === c.id);
          const st = raw && isObj(raw.status) ? raw.status : {};
          const covers = objs(isObj(st.covers) ? st.covers.items : [])
            .map((x) => nodeRef(x.node))
            .filter((x): x is NodeRef => Boolean(x));
          return {
            id: c.id,
            title: c.name,
            type: (c.kind === "expanded_access" ? "trial" : c.kind) as Asset["type"],
            status: c.status,
            sponsor: c.sponsor,
            covers,
            url: c.channels[0]?.url ?? nodeUrl(c.id),
            sources: c.sources,
          };
        });
      const group = cards.find((c) => c.kind === "patient_group" || c.kind === "organisation");
      return {
        condition: { ...condition, label: upper(condition.label) },
        why: why.map((text, index) => ({ text, msg: messageRef(arr(it.why_msg)[index]), cites: [], lang: str(it.display_lang) ?? "en" })),
        shared,
        strength: STRENGTH[str(it.verdict) ?? ""] ?? "weak",
        assets,
        group,
        cards,
        returned: isObj(it.assets) ? {
          cards: cards.length, total: num(it.assets.total) ?? null,
          partial: it.assets.partial === true || num(it.assets.total) === undefined || (num(it.assets.total) as number) > cards.length,
          counts: isObj(it.assets.exact_counts) ? it.assets.exact_counts : {},
        } : undefined,
        conflicts: [...arr(it.conflicts), ...arr(isObj(it.mechanism) ? it.mechanism.effect_conflicts : [])].flatMap(value =>
          typeof value === "string" ? [{ text: value, cites: [], lang: "en" }] : isObj(value) && (str(value.text) || str(value.why) || isObj(value.msg)) ? [sentence({ ...value, text: str(value.text) ?? str(value.why) })] : []),
        limits: [...arr(it.limits).flatMap(value => typeof value === "string" ? [{ text: value, cites: [], lang: "en" }] : isObj(value) ? [sentence(value)] : []), {text:"",msg:{key:"connectionMap.inference",fallback:"Shared pathway membership is a research lead, not proof of a shared disease mechanism or treatment."},cites:[]}, {text:"",msg:{key:"connectionMap.bounded",fallback:"Missing items in this returned subset are not confirmed absent."},cites:[]}],
        edges: [...factEdges(it.edges), ...cards.flatMap(card => card.edges)],
        sources: [computed(condition, why), ...termSources(shared.processes, "GO / Reactome", checked), ...termSources(shared.symptoms, "HPO", checked), ...factSources(it.sources, checked), ...cards.flatMap(card => card.sources)],
      };
    })
    .filter((c): c is RelatedCommunity => c !== null);
  const counterexamples: Counterexample[] = counter
    .map((c): Counterexample | null => {
      const n = isObj(c.neighbour) ? c.neighbour : {};
      const condition = nodeRef(n.neighbour);
      if (!condition) return null;
      const shared = sharedOf(n);
      const why = str(c.why) ?? "";
      return {
        condition: { ...condition, label: upper(condition.label) },
        looks_like: { text: shared.symptoms.map((s) => s.label).join(" · "), cites: shared.symptoms.map((s) => s.id), lang: "en" },
        differs: { text: why, msg: messageRef(c.why_msg), cites: [], lang: str(c.display_lang) ?? "en" },
        edges: [...factEdges(c.edges), ...factEdges(n.edges)],
        sources: [computed(condition, strs(n.why)), ...termSources(shared.symptoms, "HPO", checked), ...factSources(c.sources, checked)],
      };
    })
    .filter((c): c is Counterexample => c !== null);
  const query = nodeRef(raw.query) ?? { id, kind: "disease" as const, label: id };
  return { origin: "api", lang: str(raw.display_lang) ?? "en", condition: query, communities, counterexamples, people: [] };
}

/** Researchers the server marks as working across communities become bridges (J2.4). */
export function bridges(cards: Connection[]): Bridge[] {
  return cards
    .filter((c) => c.kind === "researcher" && (c.communities?.length ?? 0) >= 2)
    .map((c): Bridge => {
      const works: Work[] = c.sources
        .map((s) => s.locator?.split("|").find((x) => x.startsWith("PMID:")))
        .filter((x, i, a): x is string => Boolean(x) && a.indexOf(x) === i)
        .slice(0, 3)
        .map((pmid) => ({ id: pmid, title: pmid, kind: "paper", url: nodeUrl(pmid) }));
      return {
        person: { id: c.id, kind: "person", label: c.name },
        institution: c.affiliation,
        channel: c.channels[0],
        communities: (c.communities ?? []).map((label) => ({ id: label, kind: "pathway", label })),
        works,
      };
    });
}

// --- shared research questions (/questions, D27) --------------------------------------------------

function sentence(v: unknown): Sentence {
  const o = isObj(v) ? v : {};
  const msg = isObj(o.msg) && str(o.msg.key) ? { key: str(o.msg.key) as string, params: (isObj(o.msg.params) ? o.msg.params : undefined) as MessageRef["params"], fallback: str(o.msg.fallback) ?? str(o.text) ?? "" } : undefined;
  return { text: str(o.text) ?? msg?.fallback ?? "", cites: strs(o.cites), lang: str(o.display_lang) ?? str(o.lang) ?? "en", msg };
}

/** The server's questions (same shape as ResearchQuestion plus extras), normalised defensively. */
export function questions(raw: unknown): ResearchQuestion[] {
  if (!isObj(raw)) return [];
  return objs(raw.questions).map((q) => ({
    id: str(q.id) ?? "",
    origin: "api" as const,
    status: (["proposed", "running", "answered"].includes(str(q.status) ?? "") ? str(q.status) : "proposed") as ResearchQuestion["status"],
    proposed_on: str(q.checked_on),
    hypothesis: sentence(q.hypothesis),
    why_it_matters: isObj(q.why_it_matters) ? sentence(q.why_it_matters) : undefined,
    term: isObj(q.term) ? sentence(q.term) : undefined,
    shared: isObj(q.shared) ? sentence(q.shared) : undefined,
    communities: objs(q.communities)
      .map((c) => ({ node: nodeRef(c.node), patient_groups: num(c.patient_groups) }))
      .filter((c): c is { node: NodeRef; patient_groups: number | undefined } => Boolean(c.node))
      .map((c) => ({ node: { ...c.node, label: c.node.label.charAt(0).toUpperCase() + c.node.label.slice(1) }, patient_groups: c.patient_groups })),
    for: objs(q.for).map(sentence),
    against: objs(q.against).map(sentence),
    unknown: objs(q.unknown).map(sentence),
    experiment: sentence(q.experiment),
    experiment_note: isObj(q.experiment_note) ? sentence(q.experiment_note) : undefined,
    experiment_design: strs(q.experiment_design),
    drugs: objs(q.drugs).map((d) => {
      const drug = isObj(d.drug) ? d.drug : {};
      return { id: str(drug.id) ?? "", label: str(drug.label) ?? "", stage: str(d.drug_max_stage) ?? str(d.stage), sentence: sentence(d.sentence), note: str(d.note) };
    }),
    assets: objs(q.assets).map((a) => ({
      id: str(a.id) ?? "",
      title: str(a.title) ?? str(a.id) ?? "",
      type: ((["registry", "natural_history", "trial", "observational", "model", "grant", "biobank"].includes(str(a.type) ?? "") ? str(a.type) : "observational") as Asset["type"]),
      status: str(a.status) ? studyStatus(a.status) : undefined,
      sponsor: str(a.sponsor),
      covers: objs(a.covers).map(nodeRef).filter((x): x is NodeRef => Boolean(x)),
      url: str(a.url) ?? nodeUrl(str(a.id) ?? ""),
      sources: factSources(a.sources, ""),
    })),
    people: objs(q.people).map((p): Bridge => {
      const person = nodeRef(p.person) ?? { id: "", kind: "person" as const, label: "" };
      const contact = isObj(p.contact) ? p.contact : {};
      return {
        person,
        institution: str(p.institution) ?? strs(p.affiliations)[0] ?? str(contact.institution),
        communities: objs(p.communities).map(nodeRef).filter((x): x is NodeRef => Boolean(x)),
        works: [],
        facts: objs(p.facts).map(sentence),
        suggestion: isObj(p.suggestion) ? sentence(p.suggestion) : undefined,
        channel: person.id.startsWith("ORCID:") ? { kind: "profile", url: nodeUrl(person.id) as string } : undefined,
      };
    }),
    sources: objs(q.sources).map((x) => ({
      id: str(x.id) ?? "",
      source: str(x.source) ?? "",
      url: (/^https?:\/\//.test(str(x.url) ?? "") ? str(x.url) : undefined) ?? (str(x.edge_id) ? edgeUrl(str(x.edge_id) as string) : undefined),
      locator: str(x.locator) ?? str(x.record),
      retrieved_on: str(x.retrieved_on) ?? str(x.retrieved_at) ?? str(x.fetched_at) ?? "",
      sha256: str(x.sha256),
      tier: (["high", "moderate", "low"].includes(str(x.tier) ?? "") ? str(x.tier) : tierOf(str(x.source) ?? "")) as Tier,
      statement: str(x.statement),
      record: str(x.record), assertion: str(x.assertion) ?? str(x.edge_id), kind: edgeKind(x.kind ?? x.edge_kind),
      references: strs(x.references), generated_by: str(x.generated_by),
      sha256_scope: str(x.sha256_scope), version: str(x.version), record_count: num(x.records),
    })),
    checked_on: str(q.checked_on) ?? "",
  }));
}
