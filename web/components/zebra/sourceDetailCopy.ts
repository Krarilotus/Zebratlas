import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";
export const sourceDetailCopy = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).sourceDetailWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["sourceDetailWords"]>;
