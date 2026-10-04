// Fails the build when a catalog drifts from the English source: missing or extra keys, ICU
// syntax errors, different variables, or a plural without "other". Run: node scripts/check-i18n.ts
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { messageVars } from "../lib/i18n/format.ts";

const LOCALES = ["en", "de", "es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr"];
const ROOT = join(import.meta.dirname, "..", "messages");

type Tree = { [k: string]: string | Tree };

function leaves(tree: Tree, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [k, v] of Object.entries(tree)) {
    if (k === "_meta") continue;
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === "string") out.set(key, v);
    else for (const [kk, vv] of leaves(v, key)) out.set(kk, vv);
  }
  return out;
}

const load = (file: string): Tree => JSON.parse(readFileSync(file, "utf8")) as Tree;

// The core catalog lives in messages/<locale>.json, namespaces in messages/<ns>/<locale>.json.
const sets: { name: string; file: (l: string) => string }[] = [{ name: "core", file: (l) => join(ROOT, `${l}.json`) }];
// Only namespaces lib/i18n/messages.ts imports are part of the app; a folder another agent is still
// writing is reported, not failed, until it is registered there.
const registry = readFileSync(join(import.meta.dirname, "..", "lib", "i18n", "messages.ts"), "utf8");
for (const entry of readdirSync(ROOT)) {
  if (!statSync(join(ROOT, entry)).isDirectory()) continue;
  if (!registry.includes(`@/messages/${entry}/`)) {
    console.warn(`i18n: messages/${entry}/ is not registered in lib/i18n/messages.ts yet; skipped`);
    continue;
  }
  sets.push({ name: entry, file: (l) => join(ROOT, entry, `${l}.json`) });
}

const errors: string[] = [];
for (const set of sets) {
  const source = leaves(load(set.file("en")));
  for (const [key, msg] of source) {
    try {
      messageVars(msg);
    } catch (e) {
      errors.push(`${set.name}/en ${key}: ${(e as Error).message}`);
    }
  }
  for (const locale of LOCALES.filter((l) => l !== "en")) {
    let target: Map<string, string>;
    try {
      target = leaves(load(set.file(locale)));
    } catch (e) {
      errors.push(`${set.name}/${locale}: cannot read (${(e as Error).message})`);
      continue;
    }
    for (const [key, msg] of source) {
      const tr = target.get(key);
      if (tr === undefined) {
        errors.push(`${set.name}/${locale} ${key}: missing`);
        continue;
      }
      try {
        const want = messageVars(msg).join(",");
        const got = messageVars(tr).join(",");
        if (want !== got) errors.push(`${set.name}/${locale} ${key}: variables {${got}} differ from en {${want}}`);
      } catch (e) {
        errors.push(`${set.name}/${locale} ${key}: ${(e as Error).message}`);
      }
    }
    for (const key of target.keys()) if (!source.has(key)) errors.push(`${set.name}/${locale} ${key}: not in en`);
  }
}

if (errors.length > 0) {
  console.error(`i18n check failed (${errors.length}):\n${errors.slice(0, 80).join("\n")}`);
  process.exit(1);
}
console.log(`i18n check passed: ${sets.length} catalog set(s) × ${LOCALES.length} locales`);
