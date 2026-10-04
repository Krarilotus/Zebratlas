// View model for the journeys (docs/design/JOURNEYS.md). lib/api.ts maps server responses onto
// these types; fields the server does not provide yet are listed in web/API-GAPS.md.

// --- shared graph types (docs/design/API.md v0) -------------------------------------------

export type EdgeKind = "observed" | "extracted" | "inferred" | "hypothesis";

export type Evidence = {
  source: string;
  record?: string;
  url?: string;
  locator?: string;
  sha256?: string;
  sha256_scope?: string;
  version?: string;
  record_count?: number;
  generated_by?: string;
  references: string[];
  evidence_code?: string;
  frequency?: string;
  date?: string;
  confidence?: number;
  quote?: string;
};

export type Edge = {
  id?: string;
  from: string;
  to: string;
  relation: string;
  /** Missing in older fact payloads; never assume an assertion was observed. */
  kind?: EdgeKind;
  evidence: Evidence[];
  contradicted_by?: Evidence[];
  why?: string;
};

export type NodeKind =
  | "disease"
  | "gene"
  | "phenotype"
  | "pathway"
  | "paper"
  | "study"
  | "grant"
  | "person"
  | "organisation"
  | "asset";

export type NodeRef = { id: string; kind: NodeKind; label: string; plain_label?: string };

/**
 * Where a response came from: the atlas server, a hand-checked evidence trail with its sources
 * (web/content/trails), or hand-written sample data with placeholders (always shown as such).
 */
export type Origin = "api" | "curated" | "mock";

/** Text in the language it was written in, kept beside a translation. */
export type Original = { text: string; lang: string };

/**
 * Server text as a catalog message (D26): rendered through the reader's catalog, else `fallback`.
 * A param may be a list (joined in the reader's language: "A, B and C") or a nested message
 * (e.g. a plain process phrase), rendered first.
 */
export type MessageRef = { key: string; params?: Record<string, MessageParam>; fallback: string };
export type MessageParam = string | number | boolean | null | MessageRef | MessageParam[];
/** Server text: plain (already in a language) or a catalog message. */
export type Message = string | MessageRef;

/** A generated or curated sentence; `cites` are SourceRecord ids. */
export type Sentence = {
  text: string;
  /** When set, the sentence is rendered from the catalog in the reader's language (D26). */
  msg?: MessageRef;
  cites: string[];
  /** Language of `text` when it may differ from the page (marked with lang=, WCAG 3.1.2). */
  lang?: string;
  original?: Original;
};

export type Tier = "high" | "moderate" | "low";

/** One source record behind a statement (D15: URL + locator + retrieval date + sha256). */
export type SourceRecord = {
  id: string;
  source: string;
  /** Catalog label for a local fallback; external source names stay verbatim. */
  source_key?: import("@/lib/i18n/messages").MessageKey;
  url?: string;
  locator?: string;
  retrieved_on: string;
  sha256?: string;
  version?: string;
  tier: Tier;
  quote?: Original;
  /** The fact this record supports, in plain words (server `facts[].text`). */
  statement?: string;
  statement_msg?: MessageRef;
  statement_lang?: string;
  record?: string;
  assertion?: string;
  kind?: EdgeKind;
  references?: string[];
  generated_by?: string;
  sha256_scope?: string;
  record_count?: number;
};

export type Validator = { ok: boolean; checked: number; notes: string[] };

/** One language-model step behind a text: which path produced it and its PROV-O activity ids. */
export type LlmCall = {
  purpose: string;
  /** "llm" (validated model output), "template" (deterministic fallback) or "lexical". */
  origin: string;
  activities: string[];
  provider?: string;
  model?: string;
  cached?: boolean;
};

// --- J1.1 resolve ----------------------------------------------------------------------------

export type MatchVia = "name" | "alias" | "gene" | "typo" | "sentence" | "id" | "symptom";

export type ResolveChoice = { condition: NodeRef; via: MatchVia; matched: string; coverage?: import("./resources").SubjectCoverage };

export type ResolveResponse = {
  origin: Origin;
  query: string;
  status: "one" | "choose" | "none";
  choices: ResolveChoice[];
  /** The part of the input the resolver used, e.g. "STXBP1" from a sentence. */
  understood?: string;
};

// --- J1.2 summary ------------------------------------------------------------------------------

export type SummaryResponse = {
  origin: Origin;
  lang: string;
  condition: NodeRef;
  /** Two plain sentences: what this is. */
  what: Sentence[];
  /** "llm": validated plain text; otherwise the UI writes its own plain sentences from the catalog. */
  generated: "llm" | "template" | "source";
  /** One plain sentence for "you're not alone", when the server writes one. */
  not_alone?: Sentence;
  /** People or families described in the literature (Orphanet "Cases/families"); the UI phrases it. */
  cases_described?: { n: number; source: SourceRecord };
  medical: { names: string[]; genes: NodeRef[]; terms: { term: string; plain: string }[]; definition?: string };
  sources: SourceRecord[];
};

// --- J1.3/J1.4/J2.3/J3 connections -------------------------------------------------------------

export type ConnectionKind =
  | "patient_group"
  | "registry"
  | "natural_history"
  | "trial"
  | "observational"
  | "expanded_access"
  | "expert_centre"
  | "researcher"
  | "organisation"
  | "grant";

/** exact: names this diagnosis; related: a neighbouring community; broad: an umbrella group. */
export type MatchLevel = "exact" | "related" | "broad";

export type StudyStatus =
  | "recruiting"
  | "not_yet_recruiting"
  | "active_not_recruiting"
  | "enrolling_by_invitation"
  | "completed"
  | "terminated"
  | "withdrawn"
  | "suspended"
  | "unknown";

export type ChannelKind =
  | "website"
  | "contact_form"
  | "email"
  | "study_contact"
  | "registry_record"
  | "institution_page"
  | "profile"
  | "grant_page";
export type Channel = { kind: ChannelKind; url: string };

export type Connection = {
  id: string;
  kind: ConnectionKind;
  match: MatchLevel;
  /** Names are never translated. */
  name: string;
  /** ISO 3166 country codes; shown with Intl.DisplayNames in the reader's language. */
  countries: string[];
  /** Languages the organisation works in (BCP 47). */
  languages: string[];
  why: Sentence;
  channels: Channel[];
  status?: StudyStatus;
  /** Who runs it (studies: sponsor; grants: institution). */
  sponsor?: string;
  /** People: first listed institution. */
  affiliation?: string;
  /** People: research communities they publish in, when they span two or more (bridges, J2.4). */
  communities?: string[];
  /** Conditions this connection already serves (shared assets across communities). */
  serves: NodeRef[];
  sources: SourceRecord[];
  /** Graph path from the condition to this connection. */
  edges: Edge[];
  /** Atlas edge ids behind the card (`/api/provenance/{id}`, `/api/verify/{id}`). */
  edge_ids: string[];
  checked_on: string;
  limits?: string[];
  conflicts?: string[];
  validator?: Validator;
  llm?: LlmCall[];
  /** Hand-written sample (mock): never a real organisation. */
  placeholder?: boolean;
};

/** One source we searched; `covered: false` = the source was not fetched for this condition's gene. */
export type Coverage = { source: string; searched_on: string; found: number; covered?: boolean; note?: string; lang?: string };

export type ConnectionsResponse = {
  origin: Origin;
  lang: string;
  condition: NodeRef;
  exact: Connection[];
  related: Connection[];
  /** All the server knows per kind (the cards are the first few of each). */
  totals?: Partial<Record<ConnectionKind, { exact: number; related: number }>>;
  coverage: Coverage[];
  /** Plain note when nothing exact was found ("no record found, not that none exists"). */
  coverage_note?: string;
  subject_coverage?: import("./resources").SubjectCoverage;
};

// --- J2 related communities ----------------------------------------------------------------------

export type Strength = "strong" | "some" | "weak";

export type AssetType = "registry" | "natural_history" | "trial" | "observational" | "model" | "grant" | "biobank";

export type Asset = {
  id: string;
  title: string;
  type: AssetType;
  status?: StudyStatus;
  sponsor?: string;
  covers: NodeRef[];
  duplicate_of?: string[];
  url?: string;
  sources: SourceRecord[];
  placeholder?: boolean;
};

export type RelatedCommunity = {
  returned?: { cards: number; total: number | null; partial: boolean; counts: Record<string, unknown> };
  conflicts?: Sentence[];
  condition: NodeRef;
  /** The community's patient group, if one is known. */
  group?: Connection;
  /** Who to contact first for working together (e.g. a shared study's team). */
  partner?: Connection;
  /** Set when this link comes from a hand-checked trail (web/content/trails): the check date. */
  checked_by_hand?: string;
  /** The community's own top connections (group, studies, people), for drafts and the gap path. */
  cards?: Connection[];
  why: Sentence[];
  /** What this link does not show (variant differences, naming conflicts, eligibility). */
  limits?: Sentence[];
  /** One plain sentence that must stay visible in "How we know". */
  caveat?: string;
  shared: { genes: NodeRef[]; processes: NodeRef[]; symptoms: NodeRef[] };
  strength: Strength;
  assets: Asset[];
  edges: Edge[];
  sources: SourceRecord[];
};

export type Counterexample = {
  condition: NodeRef;
  looks_like: Sentence;
  differs: Sentence;
  edges: Edge[];
  sources: SourceRecord[];
};

export type Work = { id: string; title: string; kind: "paper" | "grant"; year?: number; url?: string };

export type Bridge = {
  person: NodeRef;
  institution?: string;
  channel?: Channel;
  communities: NodeRef[];
  /** What they worked on (cited). */
  works: Work[];
  /** What they worked on, as cited statements (grants, papers, trials). */
  facts?: Sentence[];
  /** Why they might matter here: our suggestion (a hypothesis), always labelled as such. */
  suggestion?: Sentence;
  placeholder?: boolean;
};

export type RelatedResponse = {
  origin: Origin;
  lang: string;
  condition: NodeRef;
  communities: RelatedCommunity[];
  counterexamples: Counterexample[];
  people: Bridge[];
};

// --- D27 shared research questions ------------------------------------------------------------

/**
 * A research question whose answer would compound across communities: the hypothesis, who it
 * concerns, evidence for and against, the deciding experiment, who could run it, how to join.
 */
export type ResearchQuestion = {
  id: string;
  origin: Origin;
  status: "proposed" | "running" | "answered";
  proposed_on?: string;
  hypothesis: Sentence;
  why_it_matters?: Sentence;
  /** The scientific name of the shared process, shown one tap down. */
  term?: Sentence;
  communities: { node: NodeRef; families?: { n: number; label: Sentence; cites: string[] }; patient_groups?: number }[];
  /** The shared mechanism in plain words. */
  shared?: Sentence;
  for: Sentence[];
  against: Sentence[];
  unknown: Sentence[];
  experiment: Sentence;
  experiment_note?: Sentence;
  /** Steps of the proposed design. */
  experiment_design?: string[];
  first_step?: Sentence;
  /** Drugs in clinical study on a shared target or a related indication (never efficacy). */
  drugs?: { id: string; label: string; stage?: string; sentence: Sentence; note?: string }[];
  /** Who to write to first (a connection with an official channel). */
  partner?: Connection;
  assets: Asset[];
  people: Bridge[];
  sources: SourceRecord[];
  checked_on: string;
};

// --- J3 gaps ---------------------------------------------------------------------------------

export type StartStep = "contact_related" | "join_registry" | "find_researchers" | "ask_doctor";

export type GapsResponse = {
  origin: Origin;
  lang: string;
  condition: NodeRef;
  searched: Coverage[];
  subject_coverage?: import("./resources").SubjectCoverage;
  /** What our sources do not record, and how it could be closed. */
  unknown: Sentence[];
  /** Questions to take to a specialist, most important first. */
  questions: Sentence[];
};

// --- J1.5/J2.5 messages ---------------------------------------------------------------------------

export type MessageKind = "message" | "proposal";
export type Sender = "family" | "group";

export type MessageRequest = {
  condition: string;
  connection: string;
  lang: string;
  kind: MessageKind;
  sender: Sender;
};

export type ProposalSection = { key: "why_us" | "why_you" | "share" | "check" | "ask"; sentences: Sentence[] };

export type MessageResponse = {
  origin: Origin;
  lang: string;
  subject: string;
  body: string;
  /** Proposals: the cited reasons behind the body. */
  sections: ProposalSection[];
  validator: Validator;
  llm?: LlmCall;
  sources: SourceRecord[];
};

// --- U1 verify, provenance, integrity -----------------------------------------------------------

export type VerifyResult = {
  origin: Origin;
  id: string;
  state: "unchanged" | "changed" | "unreachable";
  checked_at: string;
  expected_sha256?: string;
  actual_sha256?: string;
  url?: string;
  /** Server's one-line summary of what was re-hashed. */
  summary?: string;
};

export type ProvEntity = {
  "@id": string;
  "prov:atLocation"?: string;
  file?: string;
  version?: string | null;
  retrieved_at?: string | null;
  sha256?: string | null;
  bytes?: number | null;
  licence?: string | null;
};

export type ProvActivity = {
  "@id": string;
  "rdfs:label"?: string;
  "prov:startedAtTime"?: string | null;
  "prov:endedAtTime"?: string | null;
  "prov:used"?: string[];
  "prov:wasAssociatedWith"?: { name?: string; version?: string; commit?: string | null };
};

export type ProvRecord = { record: string; locator?: string; url?: string | null; fetched_at?: string | null; sha256?: string | null };

export type ProvChain = {
  origin: Origin;
  id: string;
  raw: unknown;
  /** Set when the id is an edge: what it connects and the exact source records. */
  edge?: { from: string; relation: string; to: string; kind?: string; reason?: string; records: ProvRecord[] };
  entities: ProvEntity[];
  activities: ProvActivity[];
};

export type IntegrityCheck = { name: string; ok: boolean; violations: number; note?: string };

export type IntegrityResponse = {
  origin: Origin;
  checked_at: string;
  counts: Record<string, number>;
  checks: IntegrityCheck[];
  sources: ProvEntity[];
};
