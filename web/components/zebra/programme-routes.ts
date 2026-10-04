import type { Initiative, InitiativeAction, InitiativesResponse } from "@/lib/zebra/types";
export const OUTREACH_PROGRAMMES = ["Simons Searchlight", "Every Cure", "CZI Rare As One", "NORD / IAMRARE"] as const;
const patterns = [/searchlight/i, /every\s*cure/i, /rare\s*as\s*one|chan\s*zuckerberg|\bczi\b/i, /\bnord\b|iamrare/i];
export function programmePriority(name: string): number { const index = patterns.findIndex(pattern => pattern.test(name)); return index < 0 ? patterns.length : index; }
export function missingProgrammes(data: InitiativesResponse): string[] {
  const records = [...data.initiatives, ...data.references];
  return OUTREACH_PROGRAMMES.filter((_, index) => !records.some(record => patterns[index].test(record.initiative)));
}
export function programmeActions(record: Initiative): InitiativeAction[] {
  const actions = record.official_actions ?? (record.official_action ? [record.official_action] : []);
  return actions.filter((action, index) => actions.findIndex(other => other.url === action.url && other.action === action.action) === index);
}
export function programmeUrl(value?: string): string | undefined {
  if (!value) return;
  if (/^mailto:[^\s?<>]+@[^\s?<>]+$/i.test(value)) return value;
  try { const url = new URL(value); return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password ? url.href : undefined; } catch { return; }
}
export function closedAction(action: InitiativeAction): boolean { return /closed|expired|ended/i.test(action.availability ?? ""); }
