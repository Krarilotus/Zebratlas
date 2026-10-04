import type { ContributionInput } from "./types";

export const CONTRIBUTION_KINDS = ["correction", "new_link", "missing_evidence", "outdated_contact", "other"] as const;
export type FormContributionKind = typeof CONTRIBUTION_KINDS[number];
type Node = { id?: string; label?: string };
export type ContributionContext = { subject?: Node; target?: Node; edge?: string; kind?: FormContributionKind };
const trim = (value: string | null | undefined, max: number) => {
  const clean = value?.trim(); return clean && Array.from(clean).length <= max ? clean : undefined;
};
const node = (params: URLSearchParams, prefix: string): Node | undefined => {
  const id = trim(params.get(`${prefix}_id`), 200), label = trim(params.get(`${prefix}_label`), 300);
  return id || label ? { ...(id ? { id } : {}), ...(label ? { label } : {}) } : undefined;
};
export function contributionContext(search: string): ContributionContext {
  const params = new URLSearchParams(search);
  const kind = params.get("kind");
  const edge = trim(params.get("edge"), 600);
  return { subject: node(params, "subject"), target: node(params, "target"), edge: edge?.split("|").length === 3 ? edge : undefined, kind: CONTRIBUTION_KINDS.includes(kind as FormContributionKind) ? kind as FormContributionKind : undefined };
}
/** Context contains public atlas identifiers, never a raw question or private account data. */
export function contributionHref(context: ContributionContext, locale: string): string {
  const params = new URLSearchParams({ lang: locale });
  for (const prefix of ["subject", "target"] as const) {
    for (const field of ["id", "label"] as const) {
      const value = trim(context[prefix]?.[field], field === "id" ? 200 : 300);
      if (value) params.set(`${prefix}_${field}`, value);
    }
  }
  if (context.kind && CONTRIBUTION_KINDS.includes(context.kind)) params.set("kind", context.kind);
  const edge = trim(context.edge, 600);
  if (edge?.split("|").length === 3) params.set("edge", edge);
  return `/zebra/contribute?${params}`;
}
export function contextLabel(node?: Node) { return node?.label || node?.id || ""; }
function selectedNode(label: string, context?: Node): Node | undefined {
  const clean = trim(label, 300);
  if (clean && clean === contextLabel(context)) return context;
  return clean ? { label: clean } : undefined;
}
export function buildContribution(form: FormData, context: ContributionContext, locale: string): ContributionInput {
  const text = (name: string, max: number) => { const raw = String(form.get(name) ?? "").trim(); const clean = trim(raw, max); if (raw && !clean) throw new Error("invalid_contribution"); return clean; };
  const kind = text("kind", 30) as FormContributionKind;
  const statement = text("statement", 2000), contact = text("contact", 200);
  const subject = selectedNode(text("subject", 300) ?? "", context.subject);
  const target = selectedNode(text("condition", 300) ?? "", context.target);
  const edge = kind !== "new_link" && String(form.get("subject") ?? "").trim() === contextLabel(context.subject) && String(form.get("condition") ?? "").trim() === contextLabel(context.target) ? context.edge : undefined;
  const other = kind === "other" ? text("kind_other", 300) : undefined;
  if (!CONTRIBUTION_KINDS.includes(kind) || !statement || !contact || /\s/.test(contact) || !/^[^@]+@[^@.]+(?:\.[^@.]+)+$/.test(contact)
    || (kind === "other" && !other) || (kind === "new_link" && (!subject || !target))
    || (kind === "correction" && !subject && !edge) || (kind === "outdated_contact" && !subject)
    || (kind === "missing_evidence" && !subject && !target && !edge)) throw new Error("invalid_contribution");
  const url = (name: string) => {
    const raw = String(form.get(name) ?? "").trim(); if (!raw) return undefined;
    const value = trim(raw, 2000); if (!value) throw new Error("invalid_contribution");
    let parsed: URL; try { parsed = new URL(value); } catch { throw new Error("invalid_contribution"); }
    if (!["https:", "http:"].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error("invalid_contribution");
    return value;
  };
  return { kind, ...(other ? { kind_other: other } : {}), subject: subject ?? {}, ...(target ? { target } : {}), ...(edge ? { edge } : {}), statement,
    evidence_url: url("evidence_url"), quote: text("quote", 1000), contact_url: url("contact_url"),
    contributor: { contact, name: text("name", 120), organisation: text("organisation", 200) }, lang: locale, found_via: { page: "/zebra/contribute" } };
}
