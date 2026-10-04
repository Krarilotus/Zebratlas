import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";
export const resultCopy = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).resultWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["resultWords"]>;
