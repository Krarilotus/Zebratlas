export type OverviewEntity = { id: string; label: string; kind: "gene" | "disease"; official_name?: string };
export type OverviewEvidence = {
  source?: string; source_id?: string; name?: string; url?: string; source_url?: string; record?: string; locator?: string;
  record_locator?: string; retrieved_at?: string; version?: string; sha256?: string;
  [key: string]: unknown;
};
export type EntityOverviewData = {
  entity: OverviewEntity; text: string; mode: "source" | "ai"; language: string;
  requested_language: string; source_language?: string; can_enhance: boolean;
  evidence: OverviewEvidence[]; source_ids: string[]; facts: unknown[];
  model?: { connection: string; id: string }; reason?: string; activities: unknown[];
};
export type OverviewLoader = (entity: OverviewEntity, language: string, enhance: boolean, signal: AbortSignal) => Promise<unknown>;
type MemoEntry = { value: EntityOverviewData; expires: number };
const successfulAi = new Map<string, MemoEntry>();
let memoAccountRevision: number | undefined;
const MEMO_TTL = 10 * 60 * 1000;
/** Memory only: account transitions discard every explanation from the previous session. */
export function syncOverviewAccountRevision(revision: number) {
  if (memoAccountRevision !== revision) { successfulAi.clear(); memoAccountRevision = revision; }
}
export function clearOverviewMemo() { successfulAi.clear(); memoAccountRevision = undefined; }
type OverviewLoadScope = { accountRevision: () => number; now?: () => number };
async function memoKey(source: EntityOverviewData, revision: number): Promise<string | undefined> {
  if (!source.can_enhance || !source.model || !globalThis.crypto?.subtle) return;
  const exactSource = JSON.stringify({ entity: { id: source.entity.id, kind: source.entity.kind, official_name: source.entity.official_name },
    language: source.requested_language, actual_language: source.language, source_language: source.source_language,
    text: source.text, facts: source.facts, evidence: source.evidence, source_ids: source.source_ids, model: source.model });
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(exactSource));
  return `${revision}:${Array.from(new Uint8Array(digest), value => value.toString(16).padStart(2,"0")).join("")}`;
}
const sameModel = (a: EntityOverviewData, b: EntityOverviewData) =>
  a.model?.connection === b.model?.connection && a.model?.id === b.model?.id;
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
const string = (value: unknown) => typeof value === "string" ? value.trim() : "";
export function overviewHref(value: unknown): string | undefined {
  if (typeof value !== "string") return;
  try { const url = new URL(value); if (["http:", "https:"].includes(url.protocol) && !url.username && !url.password) return url.href; } catch { /* Source identifiers remain plain text. */ }
}
/** Bound text without paraphrasing facts or deriving a disease explanation from a gene. */
export function compactOverviewText(value: string, language: string): string {
  const text = value.replace(/\s+/gu, " ").trim();
  if (!text) return "";
  let sentences: string[];
  try { sentences = Array.from(new Intl.Segmenter(language, { granularity: "sentence" }).segment(text), part => part.segment).slice(0, 2); }
  catch { sentences = text.match(/[^.!?。！？]+[.!?。！？]*/gu)?.slice(0, 2) ?? [text]; }
  const excerpt = sentences.join("").trim();
  try {
    let words = 0;
    for (const part of new Intl.Segmenter(language, { granularity: "word" }).segment(excerpt)) {
      if (part.isWordLike && ++words > 90) return `${excerpt.slice(0, part.index).trimEnd()}…`;
    }
  } catch {
    const words = excerpt.split(/\s+/u);
    if (words.length > 90) return `${words.slice(0, 90).join(" ")}…`;
  }
  return excerpt;
}
/** Accept only the requested canonical identity and honest language/provenance metadata. */
export function normalizeOverview(raw: unknown, expected: OverviewEntity, language: string): EntityOverviewData | null {
  if (!object(raw) || !object(raw.entity) || raw.entity.id !== expected.id || raw.entity.kind !== expected.kind
    || raw.requested_language !== language || !["source", "ai"].includes(string(raw.mode))) return null;
  const actualLanguage = string(raw.language);
  if (!actualLanguage || (raw.mode === "ai" && actualLanguage !== language)) return null;
  const text = compactOverviewText(string(raw.text), actualLanguage);
  const evidence = Array.isArray(raw.evidence) ? raw.evidence.filter(object).map(item => ({ ...item,
    source: string(item.source) || undefined, source_id: string(item.source_id) || undefined,
    name: string(item.name) || undefined, url: string(item.url) || undefined, source_url: string(item.source_url) || undefined,
    record: string(item.record) || undefined, locator: string(item.locator) || undefined,
    record_locator: string(item.record_locator) || undefined, retrieved_at: string(item.retrieved_at) || undefined,
    version: string(item.version) || undefined, sha256: string(item.sha256) || undefined })) : [];
  const sourceIds = Array.isArray(raw.source_ids) ? raw.source_ids.map(string).filter(Boolean) : [];
  if (!text || (!evidence.length && !sourceIds.length)) return null;
  const model = object(raw.model) && string(raw.model.id) && string(raw.model.connection)
    ? { id: string(raw.model.id), connection: string(raw.model.connection) } : undefined;
  if (raw.mode === "ai" && !model) return null;
  return { entity: { ...expected, label: string(raw.entity.label) || expected.label, official_name: string(raw.entity.official_name) || undefined }, text,
    mode: raw.mode as "source" | "ai", language: actualLanguage, requested_language: language,
    source_language: string(raw.source_language) || undefined, can_enhance: raw.can_enhance === true,
    evidence, source_ids: sourceIds, facts: Array.isArray(raw.facts) ? raw.facts : [], model,
    reason: string(raw.reason) || undefined, activities: Array.isArray(raw.activities) ? raw.activities : [] };
}
export const fetchOverview: OverviewLoader = async (entity, language, enhance, signal) => {
  const response = await fetch("/zebra/api/overview", { method: "POST", signal, credentials: "same-origin",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify({ id: entity.id, lang: language, enhance }) });
  if (!response.ok) throw new Error("entity_overview_unavailable");
  return response.json();
};
/** Source text arrives first. Failures and canceled/wrong-language enhancement keep that source. */
export async function loadOverview(entity: OverviewEntity, language: string, signal: AbortSignal,
  loader: OverviewLoader, update: (value: EntityOverviewData, enhancing: boolean) => void,
  initial?: EntityOverviewData | null, scope?: OverviewLoadScope): Promise<void> {
  if (signal.aborted) return;
  const revision = scope?.accountRevision() ?? 0;
  syncOverviewAccountRevision(revision);
  const current = () => !signal.aborted && (!scope || scope.accountRevision() === revision);
  const now = scope?.now ?? Date.now;
  try {
    // Always reverify current source/model eligibility before consulting a successful AI memo.
    const source = normalizeOverview(await loader(entity, language, false, signal), entity, language);
    if (!current() || !source || source.mode !== "source") return;
    update(source, source.can_enhance);
    if (!source.can_enhance) return;
    const key = scope ? await memoKey(source, revision).catch(() => undefined) : undefined;
    if (!current()) return;
    for (const [id, entry] of successfulAi) if (entry.expires <= now()) successfulAi.delete(id);
    const saved = key ? successfulAi.get(key) : undefined;
    if (saved && sameModel(source, saved.value)) {
      successfulAi.delete(key!); successfulAi.set(key!, saved);
      update(saved.value, false); return;
    }
    try {
      const enhanced = normalizeOverview(await loader(entity, language, true, signal), entity, language);
      if (!current()) return;
      const accepted = enhanced?.mode === "ai" && (!source.model || sameModel(source, enhanced)) ? enhanced : null;
      if (accepted && key) {
        successfulAi.delete(key); successfulAi.set(key, { value: accepted, expires: now() + MEMO_TTL });
        while (successfulAi.size > 24) successfulAi.delete(successfulAi.keys().next().value!);
      }
      update(accepted ?? source, false);
    } catch { if (current()) update(source, false); }
  } catch { /* A missing factual record never blocks or replaces search results. */ }
}
