import "server-only";

import { cookies } from "next/headers";
import { INSECURE_SESSION_COOKIE, SESSION_COOKIE, sessionFromSetCookie } from "@/components/account/api";
import * as adapt from "@/lib/adapt";
import { CONDITION_SECTIONS, type ConditionDetail, type ConditionSection } from "./types";
import { sourceDates } from "./normalize";

export class HttpError extends Error {
  constructor(public readonly status: number, message: string, public readonly code?: string) { super(message); }
}
export const enc = encodeURIComponent;
export const HOSTED_MODEL_COOKIE = "zebra_hosted_model";
export const isHostedConnection = (value: unknown): value is string =>
  typeof value === "string" && ["hosted-free", "gemini-free", "gemini-lite-free", "openai-hosted", "kisski"].includes(value);
export async function hasAccountSession(request: Request): Promise<boolean> {
  return !!(await cookies()).get(secureRequest(request) ? SESSION_COOKIE : INSECURE_SESSION_COOKIE)?.value;
}
export const jsonResponse = (body: unknown, status = 200) => Response.json(body, { status, headers: { "cache-control": "private, no-store" } });

/** All destinations are supplied by our handlers; browser input can only fill encoded identifiers. */
export async function upstream(request: Request, path: string, init: { method?: string; body?: unknown; bytes?: Uint8Array; auth?: boolean; service?: "account" | "contribute"; timeout?: number } = {}): Promise<Response> {
  if (!path.startsWith("/api/") || path.includes("\\") || path.includes("..")) throw new HttpError(400, "Invalid API path.");
  const configured = (init.service === "account" ? process.env.ACCOUNTS_API_URL : init.service === "contribute" ? process.env.CONTRIB_API_URL : undefined)
    || process.env.ZEBRA_BACKEND_URL || process.env.ATLAS_API_URL || process.env.NEXT_PUBLIC_API_URL || "http://127.0.0.1:8000";
  let base: URL;
  try { base = new URL(configured); } catch { throw new HttpError(503, "The atlas connection is not configured correctly."); }
  if (!["http:", "https:"].includes(base.protocol) || base.username || base.password) throw new HttpError(503, "The atlas connection is not configured correctly.");
  const headers = new Headers({ accept: "application/json" });
  headers.set("x-atlas-lang", request.headers.get("x-zebra-locale") === "de" ? "de" : "en");
  const jar = await cookies();
  const token = jar.get(secureRequest(request) ? SESSION_COOKIE : INSECURE_SESSION_COOKIE)?.value;
  const authenticated = !!token && init.auth !== false;
  if (authenticated) headers.set("authorization", `Bearer ${token}`);
  // Operator overrides are anonymous defaults. Authenticated requests let the
  // backend resolve the account's saved choice and its configured fallback.
  if (!authenticated && process.env.ZEBRA_LLM_KEY) headers.set("x-llm-key", process.env.ZEBRA_LLM_KEY);
  if (!authenticated && process.env.ZEBRA_LLM_CONNECTION) headers.set("x-llm-connection", process.env.ZEBRA_LLM_CONNECTION);
  const hostedModel = jar.get(HOSTED_MODEL_COOKIE)?.value;
  if (!token && isHostedConnection(hostedModel)) { headers.set("x-llm-connection", hostedModel); headers.delete("x-llm-key"); }
  let body: BodyInit | undefined;
  if (init.bytes) { body = Buffer.from(init.bytes); headers.set("content-type", "application/octet-stream"); }
  else if (init.body !== undefined) { body = JSON.stringify(init.body); headers.set("content-type", "application/json"); }
  try {
    const response = await fetch(`${base.href.replace(/\/$/, "")}${path}`, {
      method: init.method ?? "GET", headers, body, cache: "no-store", redirect: "error",
      signal: AbortSignal.any([request.signal, AbortSignal.timeout(init.timeout ?? 35000)]),
    });
    if (authenticated && init.service === "account" && response.status === 401) await dropSession();
    return response;
  } catch {
    if (request.signal.aborted) throw new HttpError(499, "Request cancelled.");
    throw new HttpError(503, "The atlas is unavailable. Please try again.");
  }
}

export async function upstreamJson(request: Request, path: string, init?: Parameters<typeof upstream>[2]): Promise<unknown> {
  const response = await upstream(request, path, init);
  const text = await readBounded(response, 8 * 1024 * 1024);
  let body: unknown;
  try { body = text ? JSON.parse(text) : null; } catch { throw new HttpError(502, "The atlas returned an unreadable response."); }
  if (!response.ok) {
    const detail = body && typeof body === "object" && "detail" in body ? String(body.detail) : body && typeof body === "object" && "error" in body ? String(body.error).replaceAll("_", " ") : "This feature is unavailable on the atlas server.";
    const code = body && typeof body === "object" && "code" in body && typeof body.code === "string" ? body.code : undefined;
    throw new HttpError(response.status, detail.slice(0, 500), code);
  }
  return body;
}

export async function readBounded(response: Response | Request, max: number): Promise<string> {
  return new TextDecoder().decode(await readBytes(response, max));
}
export async function readBytes(response: Response | Request, max: number): Promise<Uint8Array> {
  const declared = Number(response.headers.get("content-length"));
  if (declared > max) throw new HttpError(413, "The upload or response is too large.");
  const reader = response.body?.getReader();
  if (!reader) return new Uint8Array();
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    while (true) {
      const part = await reader.read();
      if (part.done) break;
      size += part.value.length;
      if (size > max) { await reader.cancel(); throw new HttpError(413, "The upload or response is too large."); }
      chunks.push(part.value);
    }
  } finally { reader.releaseLock(); }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) { bytes.set(chunk, at); at += chunk.length; }
  return bytes;
}
export async function bodyObject(request: Request): Promise<Record<string, unknown>> {
  let value: unknown;
  try { value = JSON.parse(await readBounded(request, 64 * 1024)); } catch (error) {
    if (error instanceof HttpError) throw error;
    throw new HttpError(400, "Invalid JSON request.");
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new HttpError(400, "Invalid request.");
  return value as Record<string, unknown>;
}
export function boundedString(value: unknown, name: string, max = 200, required = true): string {
  if (typeof value !== "string" || value.length > max || (required && !value.trim())) throw new HttpError(400, `Please provide a valid ${name}.`);
  return value;
}
export function idParam(request: Request): string {
  const id = boundedString(new URL(request.url).searchParams.get("id"), "identifier", 512);
  if (/[/\\\x00-\x1f]/.test(id) || id === "." || id === "..") throw new HttpError(400, "Invalid identifier.");
  return id;
}
export function secureRequest(request: Request): boolean {
  return new URL(request.url).protocol === "https:" || request.headers.get("x-forwarded-proto") === "https";
}
export async function storeSession(request: Request, response: Response): Promise<void> {
  const session = sessionFromSetCookie(response.headers.getSetCookie());
  if (!session) throw new HttpError(502, "The atlas did not create a session. Please try again.");
  const secure = secureRequest(request);
  const jar = await cookies();
  jar.delete(HOSTED_MODEL_COOKIE);
  jar.set(secure ? SESSION_COOKIE : INSECURE_SESSION_COOKIE, session.token, { httpOnly: true, secure, sameSite: "lax", path: "/", maxAge: session.maxAge });
}
export async function dropSession(): Promise<void> {
  const jar = await cookies();
  jar.delete(SESSION_COOKIE); jar.delete(INSECURE_SESSION_COOKIE); jar.delete(HOSTED_MODEL_COOKIE);
}
export async function conditionBundle(request: Request, id: string, lang: string, sections: ConditionSection[] = ["summary", "connections", "gaps"]): Promise<ConditionDetail> {
  const paths = CONDITION_SECTIONS;
  // Reading details and prefilling a message must never spend model tokens. The
  // summary endpoint generates text; use the existing factual disease record.
  const disease = sections.includes("summary") ? upstreamJson(request, `/api/disease/${enc(id)}`) : Promise.resolve(null);
  const values = await Promise.allSettled([
    ...paths.map((part) => !sections.includes(part) ? Promise.resolve(null) : part === "summary" ? disease : upstreamJson(request, `/api/condition/${enc(id)}/${part}?lang=${enc(lang)}${part === "connections" ? "&limit=12&explain=0" : part === "jobs" ? "&limit=12" : ""}`)),
    disease,
  ]);
  const value = (i: number): unknown => values[i].status === "fulfilled" ? values[i].value : null;
  const unavailable = paths.filter((part, i) => sections.includes(part) && values[i].status === "rejected");
  const requested = values.filter((_, i) => i < paths.length ? sections.includes(paths[i]) : sections.includes("summary"));
  if (requested.every((v) => v.status === "rejected")) throw (requested[0] as PromiseRejectedResult).reason;
  return sourceDates({
    id, summary: adapt.summary(id, null, value(6)), connections: value(1) ? adapt.connections(value(1), id) : null,
    related: value(2) && typeof value(2) === "object" && "communities" in (value(2) as object) ? value(2) as ConditionDetail["related"] : value(2) ? adapt.related(value(2), id) : null, questions: value(3) ? adapt.questions(value(3)) : [],
    gaps: value(4) ? adapt.gaps(value(4), id) : null, unavailable, loadedSections: sections.filter((part) => !unavailable.includes(part)),
    jobs: value(5) as ConditionDetail["jobs"],
    initiatives: (value(6) as { already_working_on_this?: ConditionDetail["initiatives"] } | null)?.already_working_on_this ?? (value(5) as { already_working_on_this?: ConditionDetail["initiatives"] } | null)?.already_working_on_this ?? [],
  }, values.map((v) => v.status === "fulfilled" ? v.value : null));
}
