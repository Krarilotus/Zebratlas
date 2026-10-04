import { matchTag, negotiate, type Locale } from "@/lib/i18n/config";
/** Explicit URL, saved choice, browser preferences, then English. No IP lookup. */
export function preferredZebraLocale(url?: string | null, saved?: string | null, browser?: string | null): Locale {
  return matchTag(url?.trim() ?? "") ?? matchTag(saved?.trim() ?? "") ?? negotiate(browser);
}

/** Existing local copy and account email actions support EN/DE only. */
export function zebraCopyLocale(locale: Locale): "en" | "de" { return locale === "de" ? "de" : "en"; }
