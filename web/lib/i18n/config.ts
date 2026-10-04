// Supported UI languages (D17). The URL segment is the locale code; names are endonyms and are
// never translated. English is the default; non-English catalogs await native-speaker review.

export const LOCALES = ["en", "de", "es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr"] as const;
export type Locale = (typeof LOCALES)[number];
export const DEFAULT_LOCALE: Locale = "en";
export const LOCALE_COOKIE = "lang";

export type Direction = "ltr" | "rtl";

export const LOCALE_META: Record<Locale, { name: string; dir: Direction }> = {
  en: { name: "English", dir: "ltr" },
  de: { name: "Deutsch", dir: "ltr" },
  es: { name: "Español", dir: "ltr" },
  fr: { name: "Français", dir: "ltr" },
  pt: { name: "Português", dir: "ltr" },
  it: { name: "Italiano", dir: "ltr" },
  "zh-Hans": { name: "简体中文", dir: "ltr" },
  ja: { name: "日本語", dir: "ltr" },
  hi: { name: "हिन्दी", dir: "ltr" },
  ar: { name: "العربية", dir: "rtl" },
  ru: { name: "Русский", dir: "ltr" },
  tr: { name: "Türkçe", dir: "ltr" },
};

export function isLocale(value: string | undefined): value is Locale {
  return value !== undefined && (LOCALES as readonly string[]).includes(value);
}

/** Case-insensitive lookup, so "/zh-hans/..." still finds "zh-Hans". */
export function findLocale(value: string | undefined): Locale | undefined {
  if (!value) return undefined;
  const lower = value.toLowerCase();
  return LOCALES.find((l) => l.toLowerCase() === lower);
}

/** Maps a BCP 47 tag (from Accept-Language or the API) onto a supported locale. */
export function matchTag(tag: string): Locale | undefined {
  const exact = findLocale(tag);
  if (exact) return exact;
  const base = tag.toLowerCase().split("-")[0];
  // Only Simplified Chinese ships; Traditional readers get it rather than English.
  if (base === "zh") return "zh-Hans";
  return findLocale(base);
}

/** Picks the best supported locale from an Accept-Language header. */
export function negotiate(acceptLanguage: string | null | undefined): Locale {
  if (!acceptLanguage) return DEFAULT_LOCALE;
  const ranked = acceptLanguage
    .split(",")
    .map((part) => {
      const [tag, ...params] = part.trim().split(";");
      const q = params.map((p) => p.trim()).find((p) => p.startsWith("q="));
      return { tag: tag.trim(), q: q ? Number(q.slice(2)) || 0 : 1 };
    })
    .filter((x) => x.tag && x.tag !== "*" && x.q > 0)
    .sort((a, b) => b.q - a.q);
  for (const { tag } of ranked) {
    const hit = matchTag(tag);
    if (hit) return hit;
  }
  return DEFAULT_LOCALE;
}
