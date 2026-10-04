"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import {
  getZebraCatalog, resolveZebraLocale, ZEBRA_LOCALE_COOKIE, ZEBRA_LOCALE_META,
  type ZebraLocale,
} from "@/lib/zebra/locale";

type LocaleContextValue = { locale: ZebraLocale; setLocale: (locale: ZebraLocale) => void };
const LocaleContext = createContext<LocaleContextValue>({ locale: "en", setLocale: () => {} });

export function ZebraLocaleProvider({ initialLocale = "en", children }: {
  initialLocale?: ZebraLocale;
  children: React.ReactNode;
}) {
  const [locale, updateLocale] = useState<ZebraLocale>(initialLocale);
  const setLocale = useCallback((next: ZebraLocale) => {
    const resolved = resolveZebraLocale(next);
    updateLocale(resolved);
    document.cookie = `${ZEBRA_LOCALE_COOKIE}=${resolved}; Path=/zebra; Max-Age=31536000; SameSite=Lax`;
    const url = new URL(window.location.href);
    url.searchParams.set("lang", resolved);
    window.history.replaceState(window.history.state, "", `${url.pathname}${url.search}${url.hash}`);
  }, []);
  useEffect(() => { document.documentElement.lang = locale; document.documentElement.dir = ZEBRA_LOCALE_META[locale].dir; }, [locale]);
  const value = useMemo(() => ({ locale, setLocale }), [locale, setLocale]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}

export function useZebraLocale() { return useContext(LocaleContext).locale; }
export function useSetZebraLocale() { return useContext(LocaleContext).setLocale; }
export function useZebraCatalog() { return getZebraCatalog(useZebraLocale()); }
export function useZebraCopy() { return useZebraCatalog().copy; }
export function useZebraKindLabel() {
  const kinds = useZebraCopy().kinds;
  return (kind: string) => kinds[kind as keyof typeof kinds] ?? kind.replaceAll("_", " ");
}
