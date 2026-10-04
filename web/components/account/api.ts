// Server-side client for the accounts API (/api/account/* on the atlas server, crate atlas-accounts).
// The browser never talks to that API directly: the session token lives in an http-only cookie on
// this site, and server components / server actions forward it. Without a server (mock mode) or
// with a server that has no account routes, everything reports "unavailable" and the Save buttons
// disappear: accounts are optional (H1).
import "server-only";

import { cookies, headers } from "next/headers";
import { cache } from "react";

import type { Account, Conversation, ConversationSummary, ErrorCode, SavedItem, SavedKind } from "@/components/account/types";

const API_URL = (process.env.ACCOUNTS_API_URL || process.env.NEXT_PUBLIC_API_URL)?.replace(/\/$/, "");
const TIMEOUT_MS = 10000;

export const SESSION_COOKIE = "__Host-atlas_session";
export const INSECURE_SESSION_COOKIE = "atlas_session";

export type ApiReply = { status: number; body: unknown; setCookies: string[] };

/** Calls the accounts API with the reader's session. `null` = no server or no account routes. */
export async function accountRequest(
  path: string,
  init: { method?: string; body?: unknown; token?: string | null } = {},
): Promise<ApiReply | null> {
  if (!API_URL) return null;
  const h = new Headers();
  const token = init.token === undefined ? await sessionToken() : init.token;
  if (token) h.set("authorization", `Bearer ${token}`);
  if (init.body !== undefined) h.set("content-type", "application/json");
  // Per-client sign-in limits on the API key off the reader's address, not this server's.
  const incoming = await headers();
  const client = incoming.get("x-forwarded-for") ?? incoming.get("x-real-ip");
  if (client) h.set("x-forwarded-for", client);
  try {
    const res = await fetch(`${API_URL}/api/account${path}`, {
      method: init.method ?? "GET",
      headers: h,
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
      cache: "no-store",
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    const text = await res.text();
    const trimmed = text.trim();
    // A server without the account routes answers 404 with an empty body.
    if (res.status === 404 && !trimmed.startsWith("{")) return null;
    let body: unknown = null;
    if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
      try {
        body = JSON.parse(trimmed);
      } catch {
        body = null;
      }
    }
    return { status: res.status, body, setCookies: res.headers.getSetCookie() };
  } catch {
    return null;
  }
}

export async function sessionToken(): Promise<string | undefined> {
  const name = await requestIsHttps() ? SESSION_COOKIE : INSECURE_SESSION_COOKIE;
  return (await cookies()).get(name)?.value;
}

/** Maps an API reply onto an error code with a translated message (account.error.*). */
export function errorOf(reply: ApiReply | null): ErrorCode {
  if (!reply) return "unavailable";
  const code = (reply.body as { code?: string } | null)?.code;
  switch (reply.status) {
    case 401:
      return code === "bad_credentials" ? "bad_credentials" : "unauthorized";
    case 403:
      return "forbidden";
    case 404:
      return "not_found";
    case 409:
      return "conflict";
    case 422:
      return "invalid";
    case 429:
      return "rate_limited";
    case 503:
      return "unavailable";
    default:
      return "generic";
  }
}

export type AccountState = { state: "unavailable" } | { state: "signed_out" } | { state: "signed_in"; account: Account };

/** Once per request: is the account service there, and who is signed in? */
export const getAccountState = cache(async (): Promise<AccountState> => {
  // Account controls are private request state even when no API is configured at build time.
  // Reading cookies first prevents a static signed-out header/save option from being baked in.
  const token = await sessionToken();
  if (!API_URL) return { state: "unavailable" };
  if (!token) {
    const status = await accountRequest("/status", { token: null });
    const enabled = status?.status === 200 && (status.body as { enabled?: boolean } | null)?.enabled === true;
    return enabled ? { state: "signed_out" } : { state: "unavailable" };
  }
  const me = await accountRequest("/me", { token });
  if (me?.status === 200) return { state: "signed_in", account: me.body as Account };
  if (me?.status === 401) return { state: "signed_out" };
  return { state: "unavailable" };
});

export async function listSaved(kind?: SavedKind): Promise<SavedItem[] | null> {
  const reply = await accountRequest(`/saved?limit=1000${kind ? `&kind=${kind}` : ""}`);
  if (reply?.status !== 200) return null;
  return (reply.body as { items: SavedItem[] }).items;
}

export const savedKey = (kind: SavedKind, ref: string | undefined) => `${kind}|${ref ?? ""}`;

/** The reader's saved items keyed by kind + first atlas id, so N Save buttons cost one request. */
export const getSavedIndex = cache(async (): Promise<Map<string, SavedItem>> => {
  const index = new Map<string, SavedItem>();
  const state = await getAccountState();
  if (state.state !== "signed_in") return index;
  for (const item of (await listSaved()) ?? []) {
    const key = savedKey(item.kind, item.refs[0]);
    if (!index.has(key)) index.set(key, item);
  }
  return index;
});

export async function listConversations(): Promise<ConversationSummary[] | null> {
  const reply = await accountRequest("/conversations?limit=1000");
  if (reply?.status !== 200) return null;
  return (reply.body as { conversations: ConversationSummary[] }).conversations;
}

export async function getConversation(id: string): Promise<Conversation | null> {
  const reply = await accountRequest(`/conversations/${encodeURIComponent(id)}`);
  if (reply?.status !== 200) return null;
  return reply.body as Conversation;
}

/** Session token and lifetime from the API's Set-Cookie, to re-issue on this site. */
export function sessionFromSetCookie(setCookies: string[]): { token: string; maxAge: number } | null {
  for (const c of setCookies) {
    const [pair, ...attrs] = c.split(";").map((s) => s.trim());
    const name = [SESSION_COOKIE, INSECURE_SESSION_COOKIE].find((n) => pair.startsWith(`${n}=`));
    if (!name) continue;
    const token = pair.slice(name.length + 1);
    if (!token) return null;
    const maxAgeAttr = attrs.find((a) => a.toLowerCase().startsWith("max-age="));
    const maxAge = maxAgeAttr ? Number(maxAgeAttr.split("=")[1]) : 30 * 24 * 3600;
    return { token, maxAge: Number.isFinite(maxAge) && maxAge > 0 ? maxAge : 30 * 24 * 3600 };
  }
  return null;
}

/** Only https pages get a `Secure` cookie, so a plain-http demo on a LAN address still works. */
export async function requestIsHttps(): Promise<boolean> {
  const h = await headers();
  const origin = h.get("origin");
  if (origin) return origin.startsWith("https://");
  return h.get("x-forwarded-proto") === "https";
}
