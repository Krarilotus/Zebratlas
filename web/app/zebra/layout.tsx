import type { Metadata, Viewport } from "next";
import { headers } from "next/headers";
import { ZebraLocaleProvider } from "@/components/zebra/Locale";
import { AccountProvider } from "@/components/zebra/AccountProvider";
import { getZebraCatalog, ZEBRA_LOCALE_META, resolveZebraLocale } from "@/lib/zebra/locale";
import "./zebra.css";

const metadata: Metadata = {
  title: { default: "Zebratlas", template: "%s · Zebratlas" },
  robots: { index: false },
  icons: { icon: [
    ...[16, 32, 48].map((size) => ({ url: `/zebra/mark-${size}.png`, type: "image/png", sizes: `${size}x${size}`, media: "(prefers-color-scheme: light)" })),
    ...[16, 32, 48].map((size) => ({ url: `/zebra/mark-${size}-dark.png`, type: "image/png", sizes: `${size}x${size}`, media: "(prefers-color-scheme: dark)" })),
  ] },
};
export async function generateMetadata(): Promise<Metadata> {
  const locale = resolveZebraLocale((await headers()).get("x-zebra-locale"));
  return { ...metadata, description: getZebraCatalog(locale).metadata.description };
}
export const viewport: Viewport = { width: "device-width", initialScale: 1, themeColor: "#f8fafb" };

export default async function ZebraLayout({ children }: { children: React.ReactNode }) {
  const locale = resolveZebraLocale((await headers()).get("x-zebra-locale"));
  return <html lang={locale} dir={ZEBRA_LOCALE_META[locale].dir}><body className="zebra-root"><ZebraLocaleProvider initialLocale={locale}><AccountProvider>{children}</AccountProvider></ZebraLocaleProvider></body></html>;
}
