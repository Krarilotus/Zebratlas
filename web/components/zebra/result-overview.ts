import type { Connection, Sentence, SourceRecord } from "@/lib/types";
import type { ConditionDetail, ExploreResponse, ExploreResult, GraphEvidence, JobItem } from "@/lib/zebra/types";

export type ActionResult = ExploreResult;
export type FindingRow = { result: ActionResult; conditionId?: string; sources: GraphEvidence[]; connection?: Connection; job?: JobItem };
export type FindingSection = "support" | "resources" | "biology" | "evidence";
/** A source-provided subtype refines presentation without replacing the canonical result kind. */
export function findingKind(result: ExploreResult): string {
  const key = result.kind === "asset" ? "asset_kind" : result.kind === "organisation" ? "organisation_kind" : result.kind === "study" ? "study_kind" : undefined;
  return (key ? result.facts?.find(fact => fact.key === key)?.value : undefined) || result.kind;
}
export function findingSection(kind: string): FindingSection {
  if (["patient_group", "researcher", "person", "expert_centre", "organisation", "funder"].includes(kind)) return "support";
  if (["grant", "funding_call", "asset", "model", "cell_line", "biobank", "dataset", "study", "trial", "registry", "natural_history", "observational", "expanded_access", "outcome_measure", "therapy", "programme", "drug"].includes(kind)) return "resources";
  return ["disease", "gene", "phenotype", "pathway", "mechanism"].includes(kind) ? "biology" : "evidence";
}
export function isContactAction(result: ExploreResult): boolean {
  return ["patient_group", "researcher", "person", "expert_centre", "organisation", "funder"].includes(findingKind(result))
    && /contact|email/i.test(result.official_action?.action || result.how_to_get?.route || "");
}
export function safeSourceLink(value?: string): string | undefined {
  const href = safeActionUrl(value);
  if (!href || href.startsWith("mailto:")) return;
  const hostname = new URL(href).hostname;
  return hostname.includes(".") && !/^(?:localhost|127\.|0\.|10\.|192\.168\.|172\.(?:1[6-9]|2\d|3[01])\.)/.test(hostname) && !hostname.endsWith(".local") ? href : undefined;
}
export function sourceCaption(source: GraphEvidence, fallback: string): string {
  const href = safeSourceLink(source.url);
  if (/^(?:source:|cache[/:]|[A-Za-z]:[\\/]|\/)|[\\/].*[\\/]/.test(source.source)) return href ? new URL(href).hostname.replace(/^www\./, "") : fallback;
  return source.source || (href ? new URL(href).hostname.replace(/^www\./, "") : fallback);
}

export function safeActionUrl(value?: string | null): string | undefined {
  if (!value) return;
  if (/^mailto:[^\s?<>]+@[^\s?<>]+$/i.test(value)) return value;
  try { const url = new URL(value); return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : undefined; } catch { return; }
}
export function evidenceOf(sources: SourceRecord[]): GraphEvidence[] {
  return sources.map(source => ({ ...source, record: source.record || source.id, retrieved_at: source.retrieved_on }));
}
export function supportedSentence(sentence: Sentence, sources: SourceRecord[]): boolean {
  return !!sentence.text && sentence.cites.length > 0 && sentence.cites.every(cite => sources.some(source => source.id === cite));
}
/** Only explicitly resolved conditions or one-hop conditions of an explicitly resolved gene.
 * Search ranking alone does not select an arbitrary diagnosis as the user's context. */
export function contextAnchors(data: ExploreResponse): { conditions: string[]; gene?: string } {
  const explicit = data.interpretation.entities;
  const direct = explicit.filter(entity => entity.kind === "disease").map(entity => entity.id);
  const gene = explicit.find(entity => entity.kind === "gene")?.id;
  const causal = gene ? data.graph.edges.filter(edge => (edge.source === gene || edge.target === gene) && /(?:causes|caused_by|causal|disease_gene|associated_with_gene)/i.test(edge.relation)).map(edge => edge.source === gene ? edge.target : edge.source).filter(id => data.graph.nodes.some(node => node.id === id && node.kind === "disease")) : [];
  return { conditions: [...new Set([...direct, ...causal])].slice(0, 2), gene };
}
export function connectionRow(connection: Connection, conditionId: string): FindingRow {
  const action = connection.channels.find(channel => ["contact_form", "email", "study_contact"].includes(channel.kind) && safeActionUrl(channel.url)) || connection.channels.find(channel => safeActionUrl(channel.url));
  const sources = evidenceOf(connection.sources);
  return { conditionId, connection, sources, result: {
    id: connection.id, label: connection.name, kind: connection.kind, reason: connection.why.text, score: 0, evidence: sources,
    match: connection.match, relation_kind: connection.edges.some(edge => edge.kind === "hypothesis") ? "hypothesis" : connection.edges.some(edge => edge.kind === "inferred") ? "inferred" : connection.edges.length && connection.edges.every(edge => edge.kind === "observed" || edge.kind === "extracted") ? "observed" : undefined,
    status: connection.status, official_action: action ? { action: action.kind, url: action.url } : undefined,
    facts: [connection.affiliation ? { key: "affiliation", value: connection.affiliation } : null, connection.sponsor ? { key: "sponsor", value: connection.sponsor } : null, connection.countries.length ? { key: "country", value: connection.countries.join(", ") } : null].filter((fact): fact is { key: string; value: string } => !!fact),
  } };
}
export function jobRow(job: JobItem, conditionId?: string): FindingRow {
  const sources: GraphEvidence[] = job.source ? [{ source: job.source.name, url: job.source.url, record: job.source.record, retrieved_at: job.source.retrieved_at, sha256: job.source.sha256 }] : [];
  return { job, conditionId, sources, result: { id: job.id, label: job.what.label, kind: job.what.kind, reason: job.via.reason || "", score: 0, evidence: sources, holder: job.holder || undefined, facts: job.facts, match: job.match, how_to_get: job.how_to_get } };
}
export function supplementalRows(detail: ConditionDetail): FindingRow[] {
  const connections = [...(detail.connections?.exact || []), ...(detail.connections?.related || [])].map(connection => connectionRow(connection, detail.id));
  const jobs = detail.jobs?.jobs.flatMap(job => job.items.map(item => jobRow(item, detail.id))) || [];
  return [...connections, ...jobs].filter((row, index, rows) => rows.findIndex(other => other.result.id === row.result.id) === index);
}
