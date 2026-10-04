import { ZEBRA_LOCALES, type ZebraLocale } from "./locale";

export function overviewRequest(body: Record<string, unknown>): { id: string; lang: ZebraLocale; enhance: boolean } | null {
  if (Object.keys(body).some(key => !["id", "lang", "enhance"].includes(key))) return null;
  if (typeof body.id !== "string" || body.id.length > 256 || body.id.trim() !== body.id || !/^[A-Za-z][A-Za-z0-9_-]*:[A-Za-z0-9._-]+$/.test(body.id)) return null;
  if (typeof body.lang !== "string" || !ZEBRA_LOCALES.includes(body.lang as ZebraLocale)) return null;
  if (body.enhance !== undefined && typeof body.enhance !== "boolean") return null;
  return { id: body.id, lang: body.lang as ZebraLocale, enhance: body.enhance === true };
}
