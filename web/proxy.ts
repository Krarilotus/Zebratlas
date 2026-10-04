import { preferredZebraLocale } from "@/lib/zebra/locale-policy";
// Locale lives in the URL (/de/...). The root always starts in English.
import { NextResponse, type NextRequest } from "next/server";

import { findLocale, isLocale } from "@/lib/i18n/config";

export function proxy(request: NextRequest) {
  // Covers Server Actions and route handlers, including the streaming assistant.
  if (!["GET", "HEAD", "OPTIONS"].includes(request.method)) {
    const origin = request.headers.get("origin");
    const host = request.headers.get("host");
    let allowed = false;
    try { allowed = !!origin && new URL(origin).host === host; } catch { /* deny */ }
    if (!allowed || ["cross-site", "same-site"].includes(request.headers.get("sec-fetch-site") ?? "")) {
      return NextResponse.json({ code: "forbidden", detail: "not allowed" }, { status: 403 });
    }
  }
  const nonce = Buffer.from(crypto.randomUUID()).toString("base64");
  const isDev = process.env.NODE_ENV === "development";
  // Inline styles are needed by Cytoscape and React style props; script execution stays nonce-only.
  const csp = `default-src 'self'; script-src 'self' 'nonce-${nonce}' 'strict-dynamic' 'wasm-unsafe-eval'${isDev ? " 'unsafe-eval'" : ""}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self'${isDev ? " ws: wss:" : ""}; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'`;
  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("x-nonce", nonce);
  requestHeaders.set("Content-Security-Policy", csp);
  const secure = (response: NextResponse) => {
    response.headers.set("Content-Security-Policy", csp);
    return response;
  };
  const { pathname } = request.nextUrl;
  if (pathname === "/") {
    return secure(NextResponse.redirect(new URL("/zebra?lang=en", request.url)));
  }
  // The parallel frontend keeps its own root layout and selected locale.
  if (pathname === "/zebra" || pathname.startsWith("/zebra/")) {
    requestHeaders.set("x-zebra-locale", preferredZebraLocale(request.nextUrl.searchParams.get("lang"), request.cookies.get("zebra_lang")?.value ?? request.cookies.get("lang")?.value, request.headers.get("accept-language")));
    return secure(NextResponse.next({ request: { headers: requestHeaders } }));
  }
  // Same-origin workspace handlers are APIs, not localized pages. Keep the security
  // checks above, but do not redirect their requests into the locale route tree.
  if (pathname === "/api" || pathname.startsWith("/api/")) {
    return secure(NextResponse.next({ request: { headers: requestHeaders } }));
  }
  const [, first = "", ...rest] = pathname.split("/");

  if (isLocale(first)) {
    return secure(NextResponse.next({ request: { headers: requestHeaders } }));
  }

  const url = request.nextUrl.clone();
  const wrongCase = findLocale(first);
  if (wrongCase) {
    url.pathname = `/${[wrongCase, ...rest].join("/")}`;
    return secure(NextResponse.redirect(url));
  }
  url.pathname = `/en${pathname === "/" ? "" : pathname}`;
  return secure(NextResponse.redirect(url));
}

export const config = {
  // Match dynamic paths containing dots too (condition identifiers can contain them).
  matcher: ["/((?!_next/static|_next/image|query-by-graph/|favicon.ico|robots.txt|sitemap.xml).*)"],
};
