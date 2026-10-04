import { isMessageRef, makeMessageRenderer, messageKey, messageLocale } from "./render-message.ts";
import type { MessageRef } from "../types.ts";

const DISPLAY_FIELDS = new Set(["text", "statement", "label", "name", "title", "subject", "description", "summary", "note", "notes", "why", "reason", "message", "detail", "next_questions", "enrollment_note", "subtitle"]);
const ORIGINAL_FIELDS = new Set(["quote", "provenance", "raw", "source_record", "parameters"]);

/** A presentation copy. Original assertions/quotes and message references remain available. */
export function presentApiMessages(input: unknown, catalog: unknown, locale: string): unknown {
  const render = makeMessageRenderer(catalog, locale);
  const known = (value: unknown): value is MessageRef => isMessageRef(value) && messageKey(catalog, value.key) !== undefined;
  const walk = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(walk);
    if (typeof value !== "object" || value === null) return value;
    const source = value as Record<string, unknown>;
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(source)) result[key] = ORIGINAL_FIELDS.has(key) || key === "msg" || key.endsWith("_msg") ? child : walk(child);
    const originals: Record<string, unknown> = {};
    const replace = (key: string, text: unknown, lang: string) => {
      originals[key] = source[key];
      result[key] = text;
      result.display_lang = lang;
    };
    for (const [key, message] of Object.entries(source)) {
      if (!key.endsWith("_msg")) continue;
      const field = key.slice(0, -4);
      if (!DISPLAY_FIELDS.has(field)) continue;
      if (known(message)) replace(field, render(message), messageLocale(catalog, message.key, locale));
      else if (Array.isArray(message) && message.every(known)) {
        const langs = new Set(message.map((m) => messageLocale(catalog, m.key, locale)));
        replace(field, message.map(render), langs.size === 1 ? [...langs][0] : "en");
      }
    }
    if (known(source.msg)) {
      const field = ["text", "message", "description", "statement", "detail"].find((k) => typeof source[k] === "string") ?? "text";
      const lang = messageLocale(catalog, source.msg.key, locale);
      replace(field, render(source.msg), lang);
      if (field === "text" && typeof source.lang === "string") result.lang = lang;
    }
    // A standalone message is still a reference, rather than changing its schema into a string.
    if (isMessageRef(source)) return source;
    if (Object.keys(originals).length) result.display_original = originals;
    const childLanguages = (child: unknown): string[] => Array.isArray(child) ? child.flatMap(childLanguages)
      : typeof child === "object" && child !== null && "display_lang" in child && typeof child.display_lang === "string" ? [child.display_lang] : [];
    const langs = new Set(Object.values(result).flatMap(childLanguages));
    if (typeof result.display_lang === "string") langs.add(result.display_lang);
    if (langs.size) result.display_lang = langs.size === 1 ? [...langs][0] : "en";
    return result;
  };
  return walk(input);
}
