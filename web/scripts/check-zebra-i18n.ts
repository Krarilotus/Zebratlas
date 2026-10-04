// The selected frontend has its own typed catalog registry; check every supported locale.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { messageVars } from "../lib/i18n/format.ts";

type Entry = { type: "object" | "array" | "string"; text?: string; length?: number };
const root = join(import.meta.dirname, "..", "messages", "zebra");
const errors: string[] = [];

function catalog(locale: string): Map<string, Entry> {
  const entries = new Map<string, Entry>();
  function visit(value: unknown, key: string): void {
    if (typeof value === "string") {
      entries.set(key, { type: "string", text: value });
      if (!value.trim()) errors.push(`${locale} ${key}: empty string`);
      try { messageVars(value); }
      catch (error) { errors.push(`${locale} ${key}: ${(error as Error).message}`); }
    } else if (Array.isArray(value)) {
      entries.set(key, { type: "array", length: value.length });
      value.forEach((item, index) => visit(item, `${key}.${index}`));
    } else if (value !== null && typeof value === "object") {
      entries.set(key, { type: "object" });
      for (const [name, item] of Object.entries(value)) visit(item, key ? `${key}.${name}` : name);
    } else {
      errors.push(`${locale} ${key}: expected catalog object, array, or string`);
    }
  }
  try { visit(JSON.parse(readFileSync(join(root, `${locale}.json`), "utf8")), ""); }
  catch (error) { errors.push(`${locale}: cannot read catalog (${(error as Error).message})`); }
  return entries;
}

const source = catalog("en");
for (const locale of ["de", "fr", "es", "it", "pt", "ar", "hi", "ja", "zh-Hans", "ru", "tr"]) {
const target = catalog(locale);
for (const [key, entry] of source) {
  const translation = target.get(key);
  if (!translation) { errors.push(`${locale} ${key}: missing`); continue; }
  if (entry.type !== translation.type) { errors.push(`${locale} ${key}: ${translation.type} differs from en ${entry.type}`); continue; }
  if (entry.type === "array" && entry.length !== translation.length) errors.push(`${locale} ${key}: array length differs from en`);
  if (entry.type === "string") {
    try {
      if (messageVars(entry.text!).join(",") !== messageVars(translation.text!).join(",")) errors.push(`${locale} ${key}: ICU variables differ from en`);
    } catch { /* Syntax error already recorded while loading. */ }
  }
}
for (const key of target.keys()) if (!source.has(key)) errors.push(`${locale} ${key}: not in en`);

}

// Sparse catalogs deliberately inherit English. Validate only authored overrides.
for (const locale of ["es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr"]) {
  for (const [key, entry] of catalog(locale)) {
    const original = source.get(key);
    if (!original) { errors.push(`${locale} ${key}: unknown override`); continue; }
    if (entry.type !== original.type) errors.push(`${locale} ${key}: incompatible override`);
    if (entry.type === "string" && messageVars(entry.text!).join(",") !== messageVars(original.text!).join(",")) errors.push(`${locale} ${key}: ICU variables differ`);
  }
}

if (errors.length) {
  console.error(`Zebra i18n check failed (${errors.length}):\n${errors.slice(0, 80).join("\n")}`);
  process.exit(1);
}
const strings = [...source.values()].filter(entry => entry.type === "string").length;
console.log(`Zebra i18n check passed: twelve locales, ${strings} strings, matching structure and ICU variables`);
