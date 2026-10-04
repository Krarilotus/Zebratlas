import { sessionToken } from "@/components/account/api";

const endpoints: Record<string, { path: string; method: string }> = {
  suggestions: { path: "/api/query-graph/suggestions", method: "GET" },
  preview: { path: "/api/query-graph/preview", method: "POST" },
  plan: { path: "/api/ask/query", method: "POST" },
  sparql: { path: "/api/ask/query/mcp", method: "POST" },
};

async function forward(request: Request, context: { params: Promise<{ operation: string }> }) {
  const { operation } = await context.params;
  const endpoint = endpoints[operation];
  if (!endpoint || request.method !== endpoint.method) return new Response(null, { status: 404 });
  const origin = request.headers.get("origin");
  let sameOrigin = !origin;
  if (origin) {
    try {
      const parsed = new URL(origin);
      // Next may normalize Request.url to localhost behind its internal server.
      // Validate against the original Host, as the outer CSRF middleware does.
      sameOrigin = ["http:", "https:"].includes(parsed.protocol) && parsed.host === request.headers.get("host");
    } catch { sameOrigin = false; }
  }
  if (request.headers.get("sec-fetch-site") === "cross-site" || !sameOrigin) return new Response(null, { status: 403 });
  const base = process.env.NEXT_PUBLIC_API_URL?.replace(/\/$/, "");
  if (!base) return new Response(null, { status: 503 });
  const headers = new Headers();
  const token = await sessionToken();
  if (token) headers.set("authorization", `Bearer ${token}`);
  const body = request.method === "POST" ? await request.text() : undefined;
  if (body && new TextEncoder().encode(body).length > 65536) return new Response(null, { status: 413 });
  if (body !== undefined) headers.set("content-type", "application/json");
  try {
    const response = await fetch(`${base}${endpoint.path}${request.method === "GET" ? new URL(request.url).search : ""}`, {
      method: endpoint.method, headers, body, cache: "no-store", signal: AbortSignal.timeout(30000), redirect: "error",
    });
    return new Response(response.body, { status: response.status, headers: { "content-type": "application/json", "cache-control": "no-store" } });
  } catch { return new Response(null, { status: 503 }); }
}

export const GET = forward;
export const POST = forward;
