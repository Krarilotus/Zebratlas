// Message catalogs: messages/<locale>.json (core, web-ux) and messages/<namespace>/<locale>.json
// (other owners). en is the source; every other catalog has exactly its keys and ICU variables
// (scripts/check-i18n.ts fails the build otherwise).
import ar from "@/messages/ar.json";
import de from "@/messages/de.json";
import en from "@/messages/en.json";
import es from "@/messages/es.json";
import fr from "@/messages/fr.json";
import hi from "@/messages/hi.json";
import it from "@/messages/it.json";
import ja from "@/messages/ja.json";
import pt from "@/messages/pt.json";
import ru from "@/messages/ru.json";
import tr from "@/messages/tr.json";
import zhHans from "@/messages/zh-Hans.json";
import arAccount from "@/messages/account/ar.json";
import deAccount from "@/messages/account/de.json";
import enAccount from "@/messages/account/en.json";
import esAccount from "@/messages/account/es.json";
import frAccount from "@/messages/account/fr.json";
import hiAccount from "@/messages/account/hi.json";
import itAccount from "@/messages/account/it.json";
import jaAccount from "@/messages/account/ja.json";
import ptAccount from "@/messages/account/pt.json";
import ruAccount from "@/messages/account/ru.json";
import trAccount from "@/messages/account/tr.json";
import zhHansAccount from "@/messages/account/zh-Hans.json";
import arSearch from "@/messages/search/ar.json";
import deSearch from "@/messages/search/de.json";
import enSearch from "@/messages/search/en.json";
import esSearch from "@/messages/search/es.json";
import frSearch from "@/messages/search/fr.json";
import hiSearch from "@/messages/search/hi.json";
import itSearch from "@/messages/search/it.json";
import jaSearch from "@/messages/search/ja.json";
import ptSearch from "@/messages/search/pt.json";
import ruSearch from "@/messages/search/ru.json";
import trSearch from "@/messages/search/tr.json";
import zhHansSearch from "@/messages/search/zh-Hans.json";
import enContribute from "@/messages/contribute/en.json";
// ask namespace (ask agent): loaded by components/ask/strings.ts; registered here for check-i18n.
export { default as askSourceCatalog } from "@/messages/ask/en.json";
// Query card owns its namespace; register it so the build checks all 12 catalogs.
export { default as queryGraphSourceCatalog } from "@/messages/query-graph/en.json";

import type { Locale } from "@/lib/i18n/config";

/** Contribute namespace (components/contribute/i18n.ts loads all locales server-side). */
export type ContributeCatalog = typeof enContribute;

/** Core strings (web-ux) plus namespaces owned by other agents (account: the accounts agent). */
type CoreCatalog = Omit<typeof en, "_meta"> & { _meta?: Record<string, unknown> };
export type Catalog = Omit<CoreCatalog, "backend"> & { backend: CoreCatalog["backend"] & { search: typeof enSearch }; account: typeof enAccount };

type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: K extends "_meta"
    ? never
    : T[K] extends string
      ? `${P}${K}`
      : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];

/** Every translatable key, e.g. "card.howWeKnow". */
export type MessageKey = Leaves<Catalog>;

const merge = (core: CoreCatalog, account: typeof enAccount, search: typeof enSearch): Catalog => ({ ...core, backend: { ...core.backend, search }, account });

export const CATALOGS: Record<Locale, Catalog> = {
  en: merge(en, enAccount, enSearch),
  de: merge(de, deAccount, deSearch),
  es: merge(es, esAccount, esSearch),
  fr: merge(fr, frAccount, frSearch),
  pt: merge(pt, ptAccount, ptSearch),
  it: merge(it, itAccount, itSearch),
  "zh-Hans": merge(zhHans, zhHansAccount, zhHansSearch),
  ja: merge(ja, jaAccount, jaSearch),
  hi: merge(hi, hiAccount, hiSearch),
  ar: merge(ar, arAccount, arSearch),
  ru: merge(ru, ruAccount, ruSearch),
  tr: merge(tr, trAccount, trSearch),
};

export function lookup(catalog: Catalog, key: MessageKey): string {
  let node: unknown = catalog;
  for (const part of key.split(".")) node = (node as Record<string, unknown> | undefined)?.[part];
  if (typeof node === "string") return node;
  // Fall back to the English source so a missing string never blanks the screen.
  let fallback: unknown = CATALOGS.en;
  for (const part of key.split(".")) fallback = (fallback as Record<string, unknown> | undefined)?.[part];
  return typeof fallback === "string" ? fallback : key;
}

/** Is `key` a string in the English source catalog (server message keys are checked before use)? */
export function hasKey(key: string): key is MessageKey {
  let node: unknown = CATALOGS.en;
  for (const part of key.split(".")) node = (node as Record<string, unknown> | undefined)?.[part];
  return typeof node === "string";
}
