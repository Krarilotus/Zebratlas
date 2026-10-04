export type AccountLink = { flow: "verify-email" | "reset-password"; token: string };

/** Tokens stay in a fragment until hydrated, then only in component memory. */
export function consumeAccountLink(href: string, replace: (path: string) => void): AccountLink | null {
  const url = new URL(href);
  const flow = url.searchParams.get("flow");
  if (flow !== "verify-email" && flow !== "reset-password") return null;
  const token = new URLSearchParams(url.hash.slice(1)).get("token") ?? "";
  url.hash = "";
  // Also discard unsupported query tokens, so they are not carried into navigation.
  url.searchParams.delete("token");
  replace(`${url.pathname}${url.search}`);
  return { flow, token: token.length <= 2048 ? token : "" };
}
