import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import test from "node:test";
import { languagePrelude } from "./zebra-language-fixture.mjs";
import { documentRequest, lookupRequest, publicSearchUrl } from "../lib/zebra/search-privacy.ts";
import { suggestionContext } from "../lib/zebra/search-suggestions.ts";

async function isolatedModule(path, prefix = "") {
  const source = (await readFile(new URL(path, import.meta.url), "utf8")).replace(/^import .*;\r?$/gm, "");
  const js = stripTypeScriptTypes(source, { mode: "transform" });
  const overview = await readFile(new URL("../lib/zebra/overview-request.ts", import.meta.url), "utf8");
  const overviewCode = path.includes("route.ts") ? stripTypeScriptTypes(overview.replace(/^import .*;\r?$/gm, ""), { mode: "transform" }) + "\n" : "";
  return import(`data:text/javascript;base64,${Buffer.from(languagePrelude(path.includes("route.ts")) + overviewCode + prefix + js).toString("base64")}`);
}

test("private words and filenames never appear in client request URLs", async () => {
  const privacyUrl = new URL("../lib/zebra/search-privacy.ts", import.meta.url).href;
  const client = await isolatedModule("../lib/zebra/client.ts", `import { documentRequest, lookupRequest } from ${JSON.stringify(privacyUrl)}; const accountChanged = () => {}; const getAccountRevision = () => 0;\n`);
  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, options) => {
    calls.push({ url, options });
    return Response.json(new URL(url, "https://zebratlas.invalid").pathname.endsWith("document") ? { text: "fixture", name: "document" } : { results: [] });
  };
  try {
    await client.lookupEntities("Jane", new AbortController().signal);
    await client.lookupEntities("private@example.invalid", new AbortController().signal);
    const file = new File(["synthetic text"], "Jane-clinical-letter.pdf", { type: "application/pdf" });
    const result = await client.extractDocument(file);
    assert.equal(result.name, file.name);
    assert.deepEqual(calls.map(({ url }) => url), ["/zebra/api/lookup?lang=en", "/zebra/api/lookup?lang=en", "/zebra/api/document?lang=en"]);
    assert.ok(calls.every(({ options }) => options.method === "POST" && options.cache === "no-store"));
    assert.deepEqual(JSON.parse(calls[0].options.body), { q: "Jane" });
    assert.equal(calls[2].options.body, file);
  } finally { globalThis.fetch = originalFetch; }
});

test("public deep links require an exact returned canonical identifier", () => {
  for (const query of ["Jane", "patient-123", "STXBP1", "private@example.invalid", "MONDO:0012812"]) {
    assert.equal(publicSearchUrl("/zebra?q=old-private&node=private&lang=de", query, []), "/zebra?lang=de");
  }
  assert.equal(publicSearchUrl("/zebra?lang=en", "MONDO:0012812", ["MONDO:0012812"]), "/zebra?lang=en&q=MONDO%3A0012812");
  assert.equal(publicSearchUrl("/zebra?lang=en", "Jane", ["Jane"]), "/zebra?lang=en");
  assert.equal(publicSearchUrl("/zebra?lang=en", "", ["HGNC:11444"], "HGNC:11444"), "/zebra?lang=en&node=HGNC%3A11444");
});

test("lookup context respects byte bounds and controls", () => {
  assert.ok(suggestionContext("STXBP1"));
  assert.ok(suggestionContext("Jane"));
  assert.equal(suggestionContext("界".repeat(43)), null);
  assert.equal(suggestionContext("ab\u0000cd"), null);
  assert.equal(suggestionContext("letter\nprivate"), null);
});

test("request helpers use POST bodies and never filename metadata", () => {
  assert.equal(lookupRequest("private")[0], "lookup");
  const file = new File(["fixture"], "private-name.txt");
  const [path, options] = documentRequest(file);
  assert.equal(path, "document");
  assert.equal(options.method, "POST");
  assert.equal(options.body, file);
  assert.equal(new Headers(options.headers).has("x-filename"), false);
});

test("route keeps lookup private end to end and enforces its bounds", async () => {
  const server = await isolatedModule("../lib/zebra/server.ts");
  const calls = [];
  globalThis.__zebraPrivacyFixture = { ...server, upstreamJson: async (...args) => { calls.push(args); return { results: [] }; } };
  const route = await isolatedModule("../app/zebra/api/[operation]/route.ts", "const { HttpError, bodyObject, boundedString, conditionBundle, dropSession, enc, idParam, jsonResponse, readBounded, readBytes, storeSession, upstream, upstreamJson } = globalThis.__zebraPrivacyFixture;\n");
  const context = { params: Promise.resolve({ operation: "lookup" }) };
  const post = (q) => new Request("https://zebratlas.invalid/zebra/api/lookup", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ q }) });
  assert.equal((await route.GET(new Request("https://zebratlas.invalid/zebra/api/lookup?q=Jane"), context)).status, 405);
  assert.equal(calls.length, 0);
  assert.equal((await route.POST(post("Jane"), context)).status, 200);
  assert.equal(calls[0][1], "/api/search/lookup");
  assert.deepEqual(calls[0][2], { method: "POST", body: { q: "Jane", limit: 6 }, auth: false, timeout: 2500 });
  for (const q of ["", "a".repeat(129), "界".repeat(43), "ab\u0000cd"]) assert.equal((await route.POST(post(q), context)).status, 400);
  assert.equal(calls.length, 1);
  const docContext = { params: Promise.resolve({ operation: "document" }) };
  const response = await route.POST(new Request("https://zebratlas.invalid/zebra/api/document", { method: "POST", body: "fixture" }), docContext);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).name, "document");
  assert.equal(calls[1][1], "/api/explore/document");
  delete globalThis.__zebraPrivacyFixture;
});
