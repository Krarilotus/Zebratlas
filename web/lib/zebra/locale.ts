import en from "@/messages/zebra/en.json";
import de from "@/messages/zebra/de.json";

import { LOCALES, LOCALE_META, matchTag } from "@/lib/i18n/config";
import es from "@/messages/zebra/es.json";
import fr from "@/messages/zebra/fr.json";
import pt from "@/messages/zebra/pt.json";
import it from "@/messages/zebra/it.json";
import zhHans from "@/messages/zebra/zh-Hans.json";
import ja from "@/messages/zebra/ja.json";
import hi from "@/messages/zebra/hi.json";
import ar from "@/messages/zebra/ar.json";
import ru from "@/messages/zebra/ru.json";
import tr from "@/messages/zebra/tr.json";

export const ZEBRA_LOCALES = LOCALES;
export const ZEBRA_LOCALE_META = LOCALE_META;
export type ZebraLocale = typeof ZEBRA_LOCALES[number];
export const ZEBRA_LOCALE_COOKIE = "zebra_lang";
export type BriefTaskId = "F01" | "F02" | "F03" | "F04" | "F05" | "F06" | "F07" | "F08" | "F09" | "F10" | "F11" | "F12";
export type BriefTask = { id: BriefTaskId; title: string; ask: string; questions: string[] };
export type ZebraCatalog = Omit<typeof en, "briefTasks"> & { briefTasks: BriefTask[] };
export type ZebraCopy = typeof en.copy;

// JSON imports retain the full key shape; every UI value stays a general string.
const catalogs: Record<ZebraLocale, ZebraCatalog> = {
  en: { ...en, briefTasks: en.briefTasks as BriefTask[] },
  de: { ...de, briefTasks: de.briefTasks as BriefTask[] },
  "es": { ...en, copy: { ...en.copy, ...es.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "fr": { ...en, copy: { ...en.copy, ...fr.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "pt": { ...en, copy: { ...en.copy, ...pt.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "it": { ...en, copy: { ...en.copy, ...it.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "zh-Hans": { ...en, copy: { ...en.copy, ...zhHans.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "ja": { ...en, copy: { ...en.copy, ...ja.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "hi": { ...en, copy: { ...en.copy, ...hi.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "ar": { ...en, copy: { ...en.copy, ...ar.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "ru": { ...en, copy: { ...en.copy, ...ru.copy }, briefTasks: en.briefTasks as BriefTask[] },
  "tr": { ...en, copy: { ...en.copy, ...tr.copy }, briefTasks: en.briefTasks as BriefTask[] },

};

export function resolveZebraLocale(value?: string | null): ZebraLocale {
  return matchTag(value?.trim().replaceAll("_", "-") ?? "") ?? "en";
}

export function getZebraCatalog(locale: ZebraLocale = "en"): ZebraCatalog { return catalogs[locale]; }
export function getZebraCopy(locale: ZebraLocale = "en"): ZebraCopy { return catalogs[locale].copy; }
export function getZebraTasks(locale: ZebraLocale = "en"): BriefTask[] { return catalogs[locale].briefTasks; }
export function getZebraBriefWords(locale: ZebraLocale = "en") { return catalogs[locale].briefWords; }
export function getZebraSourceWords(locale: ZebraLocale = "en") { return catalogs[locale].sourceWords; }
export function getZebraKindLabel(kind: string, locale: ZebraLocale = "en"): string {
  const kinds = catalogs[locale].copy.kinds;
  return kinds[kind as keyof typeof kinds] ?? kind.replaceAll("_", " ");
}

/** Keep the selected interface language on navigation and shared links within /zebra. */
export function zebraHref(path: string, locale: ZebraLocale = "en"): string {
  if (path !== "/zebra" && !path.startsWith("/zebra/") && !path.startsWith("/zebra?") && !path.startsWith("/zebra#")) return path;
  const url = new URL(path, "https://zebratlas.invalid");
  url.searchParams.set("lang", locale);
  return `${url.pathname}${url.search}${url.hash}`;
}

export { preferredZebraLocale } from "./locale-policy";

export { zebraCopyLocale } from "./locale-policy";
