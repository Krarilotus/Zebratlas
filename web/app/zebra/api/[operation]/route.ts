import * as adapt from "@/lib/adapt";
import { cookies } from "next/headers";
import { SAVED_KINDS } from "@/components/account/types";
import { HOSTED_MODEL_COOKIE, HttpError, bodyObject, boundedString, conditionBundle, dropSession, enc, hasAccountSession, idParam, isHostedConnection, jsonResponse, readBounded, readBytes, secureRequest, storeSession, upstream, upstreamJson } from "@/lib/zebra/server";
import { sourceDates } from "@/lib/zebra/normalize";
import { CONDITION_SECTIONS, EXPLORE_INTENTS, type ConditionSection } from "@/lib/zebra/types";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";
type Context = { params: Promise<{ operation: string }> };

async function handle(request: Request, context: Context): Promise<Response> {
  try {
    const { operation } = await context.params;
    const url = new URL(request.url);
    const method = request.method;
    if (method === "GET") {
      if (operation === "privacy" || operation === "lookup") return jsonResponse({ detail: "Use POST for this request." }, 405);
      if (operation === "schema") return jsonResponse(await upstreamJson(request, "/api/explore/schema"));
      if (operation === "query-suggestions") {
        const allowed = ["node", "class", "relation", "direction", "target_class", "q", "offset", "limit"];
        if ([...url.searchParams.keys()].some(key => !allowed.includes(key))) throw new HttpError(400, "Invalid suggestion parameters.");
        const params = new URLSearchParams();
        for (const key of allowed.filter(key => !["offset", "limit"].includes(key))) if (url.searchParams.has(key)) params.set(key, boundedString(url.searchParams.get(key), key, key === "node" ? 512 : 128, key !== "q"));
        if (params.has("direction") && !["incoming", "outgoing"].includes(params.get("direction") || "")) throw new HttpError(400, "Invalid suggestion direction.");
        const offset = Number(url.searchParams.get("offset") || 0);
        if (!Number.isInteger(offset) || offset < 0 || offset > 1000000) throw new HttpError(400, "Invalid suggestion offset.");
        if (url.searchParams.has("limit") && url.searchParams.get("limit") !== "5") throw new HttpError(400, "Suggestion pages contain five records.");
        params.set("offset", String(offset)); params.set("limit", "5");
        return jsonResponse(await upstreamJson(request, `/api/query-graph/suggestions?${params}`, { timeout: 10000 }));
      }
      if (operation === "community") {
        const limit = Math.max(1, Math.min(100, Number(url.searchParams.get("limit")) || 60));
        return jsonResponse(await upstreamJson(request, `/api/community?limit=${limit}`));
      }
      if (operation === "jobs") {
        const kind = url.searchParams.get("kind") || "condition";
        if (kind !== "condition" && kind !== "gene") throw new HttpError(400, "Invalid resource type.");
        return jsonResponse(await upstreamJson(request, `/api/${kind}/${enc(idParam(request))}/jobs?limit=12`));
      }
      if (operation === "initiatives") return jsonResponse(await upstreamJson(request, "/api/initiatives"));
      if (operation === "models" || operation === "connectors") {
        const connection = operation === "models" && url.searchParams.has("connection") ? boundedString(url.searchParams.get("connection"), "connection", 200) : null;
        try {
          return jsonResponse(await upstreamJson(request, `/api/account/connectors${operation === "models" ? "/models" : ""}${connection ? `?connection=${enc(connection)}` : ""}`, { service: "account", timeout: 6000 }));
        } catch (error) {
          if (operation !== "models" || !(error instanceof HttpError) || error.status !== 404) throw error;
          const listing = await upstreamJson(request, `/api/llm/connections${connection ? `?connection=${enc(connection)}` : ""}`, { timeout: 10000 });
          if (connection) return jsonResponse(listing);
          const saved = (await cookies()).get(HOSTED_MODEL_COOKIE)?.value;
          const configured = (listing as { connections?: { name: string; default_model?: string | null }[] }).connections?.find((item) => item.name === saved);
          const selected = !await hasAccountSession(request) && isHostedConnection(saved) && configured?.default_model ? { connection: saved, model: configured.default_model } : null;
          return jsonResponse({ ...(listing as object), selected });
        }
      }
      if (operation === "condition") {
        const lang = url.searchParams.get("lang") || "en";
        if (!/^[a-z]{2}(?:-[A-Za-z]{2,8})?$/.test(lang)) throw new HttpError(400, "Invalid language.");
        const sections = (url.searchParams.get("sections") || "summary,connections,gaps").split(",");
        if (!sections.length || sections.length > CONDITION_SECTIONS.length || sections.some((section) => !CONDITION_SECTIONS.includes(section as ConditionSection))) throw new HttpError(400, "Invalid detail sections.");
        return jsonResponse(await conditionBundle(request, idParam(request), lang, [...new Set(sections)] as ConditionSection[]));
      }
      if (operation === "provenance" || operation === "verify") {
        const id = idParam(request);
        const raw = await upstreamJson(request, `/api/${operation}/${enc(id)}`);
        return jsonResponse(operation === "provenance" ? adapt.provenance(raw, id) : adapt.verify(raw, id));
      }
      if (operation === "integrity") {
        const raw = await upstreamJson(request, "/api/integrity");
        return jsonResponse(adapt.integrity(raw, null));
      }
      if (operation === "account") {
        const status = await upstreamJson(request, "/api/account/status", { service: "account" }) as { enabled?: boolean };
        if (!status.enabled) throw new HttpError(503, "Accounts are currently unavailable.");
        try { return jsonResponse({ state: "signed_in", account: await upstreamJson(request, "/api/account/me", { service: "account" }) }); }
        catch (error) { if (error instanceof HttpError && error.status === 401) return jsonResponse({ state: "signed_out" }); throw error; }
      }
      if (operation === "saved" || operation === "conversations") {
        const id = url.searchParams.has("id") ? idParam(request) : null;
        const raw = await upstreamJson(request, `/api/account/${operation}${id ? `/${enc(id)}` : "?limit=200"}`, { service: "account" }) as Record<string, unknown>;
        return jsonResponse(id ? raw : raw[operation === "saved" ? "items" : "conversations"]);
      }
      if (operation === "contribute") return jsonResponse(await upstreamJson(request, `/api/contribute/${enc(idParam(request))}`, { service: "contribute" }));
      if (operation === "export") {
        const account = url.searchParams.get("format") === "account";
        const res = await upstream(request, account ? "/api/account/export" : `/api/export.ttl?condition=${enc(idParam(request))}`, { service: account ? "account" : undefined });
        if (!res.ok) throw new HttpError(res.status, "The export could not be prepared.");
        const text = await readBounded(res, 16 * 1024 * 1024);
        return new Response(text, { headers: { "content-type": account ? "application/json" : "text/turtle; charset=utf-8", "content-disposition": `attachment; filename="${account ? "zebratlas-account.json" : "zebratlas.ttl"}"`, "cache-control": "private, no-store" } });
      }
    }
    if (method === "POST") {
      if (operation === "document") {
        const bytes = await readBytes(request, 5 * 1024 * 1024);
        const document = await upstreamJson(request, "/api/explore/document", { method: "POST", bytes, timeout: 45000 }) as Record<string, unknown>;
        return jsonResponse({ ...document, name: "document" });
      }
      const body = await bodyObject(request);
      if (operation === "privacy") {
        if (Object.keys(body).some((key) => !["email", "concerns", "type"].includes(key))) throw new HttpError(400, "Invalid privacy request.");
        const email = boundedString(body.email, "email address", 254).trim();
        const concerns = boundedString(body.concerns, "request details", 2000).trim();
        if (Buffer.byteLength(email) > 254 || Buffer.byteLength(concerns) > 2000 || /[\x00-\x1f\x7f]/.test(email) || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new HttpError(400, "Invalid privacy request.");
        if (typeof body.type !== "string" || !["remove", "correct", "object"].includes(body.type)) throw new HttpError(400, "Invalid privacy request type.");
        const reply = await upstreamJson(request, "/api/privacy/requests", { method: "POST", body: { email, concerns, type: body.type }, service: "contribute", auth: false }) as Record<string, unknown>;
        if (!reply || typeof reply.reference !== "string" || !/^pr_[a-f0-9]{20}$/.test(reply.reference) || typeof reply.received_at !== "string" || typeof reply.respond_by !== "string" || !Number.isFinite(Date.parse(reply.received_at)) || !Number.isFinite(Date.parse(reply.respond_by))) throw new HttpError(502, "The request receipt could not be verified.");
        return jsonResponse({ reference: reply.reference, received_at: reply.received_at, respond_by: reply.respond_by }, 202);
      }
      if (operation === "lookup" && "query" in body) {
        if (Object.keys(body).some(key => !["query", "limit"].includes(key))) throw new HttpError(400, "Invalid lookup request.");
        const query = boundedString(body.query, "search term", 512);
        if (Buffer.byteLength(query) > 512 || /[\x00-\x1f\x7f]/.test(query)) throw new HttpError(400, "Invalid search term.");
        const limit = body.limit ?? 10;
        if (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1 || limit > 10) throw new HttpError(400, "Invalid lookup limit.");
        return jsonResponse(await upstreamJson(request, "/api/explore/lookup", { method: "POST", body: { query, limit }, auth: false, timeout: 10000 }));
      }
      if (operation === "lookup") {
        if (Object.keys(body).some((key) => key !== "q")) throw new HttpError(400, "Invalid indexed lookup request.");
        const query = boundedString(body.q, "search term", 128);
        if (Buffer.byteLength(query) > 128 || /[\x00-\x1f\x7f]/.test(query)) throw new HttpError(400, "Invalid search term.");
        return jsonResponse(await upstreamJson(request, "/api/search/lookup", { method: "POST", body: { q: query, limit: 6 }, auth: false, timeout: 2500 }));
      }
      if (operation === "lookup") {
        if (Object.keys(body).some(key => !["query", "limit"].includes(key))) throw new HttpError(400, "Invalid lookup request.");
        const query = boundedString(body.query, "search term", 512);
        const limit = body.limit ?? 10;
        if (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1 || limit > 10) throw new HttpError(400, "Invalid lookup limit.");
        return jsonResponse(await upstreamJson(request, "/api/explore/lookup", { method: "POST", body: { query, limit }, auth: false, timeout: 10000 }));
      }
      if (operation === "sparql") {
        if (Object.keys(body).some((key) => !["sparql", "query", "focus", "semantic_focus", "limit", "reasoning"].includes(key))) throw new HttpError(400, "Invalid query request.");
        const sparql = boundedString(body.sparql, "SPARQL query", 16384);
        const query = boundedString(body.query ?? "", "query caption", 32000, false);
        if (Buffer.byteLength(sparql) > 16384 || Buffer.byteLength(query) > 32000) throw new HttpError(400, "The query is too large.");
        for (const field of ["focus", "semantic_focus"] as const) {
          const ids = body[field];
          if (ids !== undefined && (!Array.isArray(ids) || ids.length > 16 || new Set(ids).size !== ids.length || ids.some((id) => typeof id !== "string" || !id.trim() || Buffer.byteLength(id) > 256))) throw new HttpError(400, "Invalid focus identifiers.");
        }
        const limit = body.limit;
        if (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1 || limit > 160) throw new HttpError(400, "Invalid result limit.");
        if (typeof body.reasoning !== "boolean") throw new HttpError(400, "Invalid reasoning setting.");
        return jsonResponse(await upstreamJson(request, "/api/explore/sparql", { method: "POST", body: { sparql, query, focus: body.focus, semantic_focus: body.semantic_focus, limit, reasoning: body.reasoning }, timeout: 30000 }));
      }
      if (["verify-email", "resend-verification", "forgot-password", "reset-password"].includes(operation)) {
        const fields = operation === "verify-email" ? ["token", "locale"] : operation === "reset-password" ? ["token", "new_password", "locale"] : ["email", "locale"];
        if (Object.keys(body).some((key) => !fields.includes(key))) throw new HttpError(400, "Invalid account request.");
        const payload: Record<string, string> = {};
        if (fields.includes("token")) payload.token = boundedString(body.token, "token", 2048);
        if (fields.includes("new_password")) payload.new_password = boundedString(body.new_password, "new password", 1024);
        if (fields.includes("email")) payload.email = boundedString(body.email, "email address", 254);
        if (body.locale !== undefined) {
          if (body.locale !== "en" && body.locale !== "de") throw new HttpError(400, "Invalid language.");
          payload.locale = body.locale;
        }
        const reply = await upstreamJson(request, `/api/account/${operation}`, { method: "POST", body: payload, service: "account", auth: false });
        if (operation === "reset-password") await dropSession();
        return jsonResponse(reply, operation === "resend-verification" || operation === "forgot-password" ? 202 : 200);
      }
      if (operation === "password") {
        if (Object.keys(body).some((key) => !["current_password", "new_password"].includes(key))) throw new HttpError(400, "Invalid password request.");
        const current_password = boundedString(body.current_password, "current password", 1024);
        const new_password = boundedString(body.new_password, "new password", 1024);
        const res = await upstream(request, "/api/account/password", { method: "POST", body: { current_password, new_password }, service: "account" });
        const raw = await readBounded(res, 65536);
        let reply: { detail?: string; error?: string; ended_other_sessions?: number };
        try { reply = JSON.parse(raw); } catch { throw new HttpError(502, "The account server returned an unreadable response."); }
        if (!res.ok) throw new HttpError(res.status, reply.detail || reply.error?.replaceAll("_", " ") || "The password could not be changed.");
        await storeSession(request, res);
        return jsonResponse(reply);
      }
      if (operation === "logout-all") {
        if (Object.keys(body).length) throw new HttpError(400, "Invalid sign-out request.");
        const reply = await upstreamJson(request, "/api/account/logout-all", { method: "POST", body: {}, service: "account" });
        await dropSession();
        return jsonResponse(reply);
      }
      if (operation === "connectors") {
        const code = boundedString(body.user_code, "pairing code", 64);
        return jsonResponse(await upstreamJson(request, "/api/account/connectors/approve", { method: "POST", body: { user_code: code }, service: "account" }));
      }
      if (operation === "search") {
        const query = boundedString(body.query ?? "", "search", 32000, !body.plan);
        if (Buffer.byteLength(query) > 32000) throw new HttpError(400, "Please keep searches under 32 KB.");
        if (body.mode !== undefined && body.mode !== "knowledge" && body.mode !== "community") throw new HttpError(400, "Invalid search mode.");
        const limit = body.limit === undefined ? 20 : Number(body.limit);
        if (!Number.isInteger(limit) || limit < 1 || limit > 40) throw new HttpError(400, "Invalid result limit.");
        if (body.plan !== undefined) {
          if (!body.plan || typeof body.plan !== "object" || Array.isArray(body.plan)) throw new HttpError(400, "Invalid search plan.");
          const plan = body.plan as Record<string, unknown>;
          if (!Array.isArray(plan.focus) || plan.focus.length > 8 || plan.focus.some((id) => typeof id !== "string" || id.length > 256)) throw new HttpError(400, "Invalid focus identifiers.");
          if (!EXPLORE_INTENTS.includes(plan.intent as typeof EXPLORE_INTENTS[number])) throw new HttpError(400, "Invalid search intent.");
          if (!plan.filters || typeof plan.filters !== "object" || Array.isArray(plan.filters)) throw new HttpError(400, "Invalid search filters.");
          const filters = plan.filters as Record<string, unknown>;
          for (const key of ["country", "kind"]) if (filters[key] != null && (typeof filters[key] !== "string" || (filters[key] as string).length > 100)) throw new HttpError(400, "Invalid search filter.");
          if (filters.recruiting != null && typeof filters.recruiting !== "boolean") throw new HttpError(400, "Invalid recruiting filter.");
        }
        return jsonResponse(await upstreamJson(request, "/api/explore", { method: "POST", body: { query, mode: body.mode || "knowledge", limit, plan: body.plan }, timeout: 95000 }));
      }
      if (operation === "auth") {
        const mode = body.mode;
        if (mode !== "login" && mode !== "signup") throw new HttpError(400, "Invalid authentication request.");
        const email = boundedString(body.email, "email address", 254);
        const password = boundedString(body.password, "password", 1024);
        const payload: Record<string, unknown> = { email, password };
        if (mode === "signup") { if (body.display_name) payload.display_name = boundedString(body.display_name, "name", 80); if (body.locale) payload.locale = boundedString(body.locale, "language", 35); }
        const res = await upstream(request, `/api/account/${mode}`, { method: "POST", body: payload, service: "account", auth: false });
        const raw = await readBounded(res, 65536);
        let account: { state?: string; code?: string; detail?: string };
        try { account = JSON.parse(raw); } catch { throw new HttpError(502, "The account server returned an unreadable response."); }
        if (!res.ok) throw new HttpError(res.status, account?.detail || "Sign-in failed. Please check your details.", account?.code);
        if (mode === "signup" && res.status === 202 && account.state === "verification_required") return jsonResponse({ state: "verification_required", code: "verification_sent" }, 202);
        await storeSession(request, res);
        return jsonResponse({ state: "signed_in", account });
      }
      if (operation === "logout") {
        await upstreamJson(request, "/api/account/logout", { method: "POST", body: {}, service: "account" });
        await dropSession(); return new Response(null, { status: 204 });
      }
      if (operation === "saved") {
        if (!SAVED_KINDS.includes(body.kind as typeof SAVED_KINDS[number])) throw new HttpError(400, "Invalid saved item type.");
        boundedString(body.title, "title", 200);
        if (!Array.isArray(body.refs) || body.refs.length > 500 || body.refs.some((v) => typeof v !== "string" || v.length > 200)) throw new HttpError(400, "Invalid saved references.");
        return jsonResponse(await upstreamJson(request, "/api/account/saved", { method: "POST", body, service: "account" }), 201);
      }
      if (operation === "contribute") {
        if (!["new_link", "correction", "missing_evidence", "outdated_contact", "data_source", "other"].includes(String(body.kind))) throw new HttpError(400, "Invalid contribution type.");
        boundedString(body.statement, "statement", 2000);
        const reply = await upstreamJson(request, "/api/contribute", { method: "POST", body, service: "contribute" }) as { contribution?: unknown };
        return jsonResponse(reply.contribution ?? reply, 202);
      }
      if (operation === "message") {
        const condition = boundedString(body.condition, "condition", 512);
        const card = boundedString(body.connection, "connection", 512);
        const kind = body.kind === "proposal" ? "proposal" : "message";
        const raw = await upstreamJson(request, "/api/message", { method: "POST", body: { condition, card, lang: body.lang || "en", role: body.sender === "group" ? "group_leader" : "parent", kind: kind === "proposal" ? "partner_proposal" : "outreach" }, timeout: 65000 });
        return jsonResponse(sourceDates(adapt.message(raw, kind), raw));
      }
    }
    if (method === "PATCH" && operation === "account") {
      const body = await bodyObject(request);
      if (Object.keys(body).some((key) => !["display_name", "locale"].includes(key))) throw new HttpError(400, "Invalid profile fields.");
      if (body.display_name !== undefined && body.display_name !== null) boundedString(body.display_name, "name", 80, false);
      if (body.locale !== undefined && body.locale !== null) boundedString(body.locale, "language", 35, false);
      return jsonResponse({ state: "signed_in", account: await upstreamJson(request, "/api/account/me", { method: "PATCH", body, service: "account" }) });
    }
    if (method === "PATCH" && operation === "saved") {
      const body = await bodyObject(request);
      if (Object.keys(body).some((key) => !["title", "payload", "note"].includes(key))) throw new HttpError(400, "Invalid saved item fields.");
      if (body.title !== undefined) boundedString(body.title, "title", 200);
      if (body.note !== undefined && body.note !== null) boundedString(body.note, "note", 4000, false);
      if (body.payload !== undefined && (!body.payload || typeof body.payload !== "object" || Array.isArray(body.payload))) throw new HttpError(400, "Invalid saved item content.");
      return jsonResponse(await upstreamJson(request, `/api/account/saved/${enc(idParam(request))}`, { method: "PATCH", body, service: "account" }));
    }
    if (method === "PUT" && operation === "models") {
      const body = await bodyObject(request);
      const connection = boundedString(body.connection, "connection", 200);
      const model = body.model == null ? null : boundedString(body.model, "model", 200);
      try {
        const result = await upstreamJson(request, "/api/account/connectors/model", { method: "PUT", body: { connection, model }, service: "account" });
        (await cookies()).delete(HOSTED_MODEL_COOKIE);
        return jsonResponse(result);
      } catch (error) {
        if (!(error instanceof HttpError) || error.status !== 404) throw error;
        if (await hasAccountSession(request)) throw new HttpError(503, "Account model settings are unavailable on this server.");
        if (!isHostedConnection(connection)) throw new HttpError(400, "This server supports selecting hosted models only.");
        const check = await upstreamJson(request, `/api/llm/connections?connection=${enc(connection)}`, { timeout: 10000 }) as { connected: boolean; models: string[] };
        if (!check.connected || !check.models.length || (model && !check.models.includes(model))) throw new HttpError(400, "The model connection is unavailable.");
        const listing = await upstreamJson(request, "/api/llm/connections", { timeout: 10000 }) as { connections?: { name: string; default_model?: string | null }[] };
        const defaultModel = listing.connections?.find((item) => item.name === connection)?.default_model;
        if (!defaultModel || !check.models.includes(defaultModel) || (model && model !== defaultModel)) throw new HttpError(400, "This server supports the connection's configured default model only.");
        (await cookies()).set(HOSTED_MODEL_COOKIE, connection, { httpOnly: true, secure: secureRequest(request), sameSite: "lax", path: "/", maxAge: 60 * 60 * 24 * 30 });
        return jsonResponse({ selected: { connection, model: defaultModel } });
      }
    }
    if (method === "DELETE" && operation === "models") {
      try {
        const result = await upstreamJson(request, "/api/account/connectors/model", { method: "DELETE", service: "account" });
        (await cookies()).delete(HOSTED_MODEL_COOKIE);
        return jsonResponse(result);
      } catch (error) {
        if (!(error instanceof HttpError) || error.status !== 404) throw error;
        (await cookies()).delete(HOSTED_MODEL_COOKIE);
        return jsonResponse({ selected: null });
      }
    }
    if (method === "DELETE" && operation === "connectors") return jsonResponse(await upstreamJson(request, `/api/account/connectors/${enc(idParam(request))}`, { method: "DELETE", service: "account" }));
    if (method === "DELETE" && operation === "saved") {
      await upstreamJson(request, `/api/account/saved/${enc(idParam(request))}`, { method: "DELETE", service: "account" });
      return new Response(null, { status: 204 });
    }
    return jsonResponse({ detail: "Unknown endpoint or method." }, 404);
  } catch (error) {
    return jsonResponse({ detail: error instanceof HttpError ? error.message : "The request could not be completed.", ...(error instanceof HttpError && error.code ? { code: error.code } : {}) }, error instanceof HttpError ? error.status : 500);
  }
}
export const GET = handle;
export const POST = handle;
export const DELETE = handle;
export const PUT = handle;
export const PATCH = handle;
