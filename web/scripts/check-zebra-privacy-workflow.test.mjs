import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire, stripTypeScriptTypes } from "node:module";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { languagePrelude } from "./zebra-language-fixture.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(process.env.ZEBRA_TEST_WEB ? path.join(process.env.ZEBRA_TEST_WEB, "package.json") : import.meta.url);
const ts = require("typescript"), React = require("react"), { renderToStaticMarkup } = require("react-dom/server");
async function isolatedModule(relative, prefix = "") {
  const source = fs.readFileSync(path.join(root, relative), "utf8").replace(/^import .*;\r?$/gm, "");
  const overview = fs.readFileSync(path.join(root, "lib/zebra/overview-request.ts"), "utf8");
  const overviewCode = relative.includes("route.ts") ? stripTypeScriptTypes(overview.replace(/^import .*;\r?$/gm, ""), { mode: "transform" }) + "\n" : "";
  return import(`data:text/javascript;base64,${Buffer.from(languagePrelude(relative.includes("route.ts")) + overviewCode + prefix + stripTypeScriptTypes(source, { mode: "transform" })).toString("base64")}`);
}

test("anonymous client sends private concerns only in a fixed POST body", async () => {
  const client = await isolatedModule("lib/zebra/client.ts", "const getAccountRevision = () => 0;\n");
  const original = globalThis.fetch, calls = [];
  globalThis.fetch = async (...args) => { calls.push(args); return Response.json({ reference: "pr_0123456789abcdef0123", received_at: "2026-10-04T10:00:00Z", respond_by: "2026-11-03T10:00:00Z" }); };
  try {
    await client.submitPrivacyRequest({ email: "private@example.invalid", concerns: "Jane's profile needs correction", type: "correct", details: "must not leave browser", lang: "de" });
    assert.equal(calls[0][0], "/zebra/api/privacy?lang=en");
    assert.equal(calls[0][1].method, "POST");
    assert.equal(calls[0][1].cache, "no-store");
    assert.deepEqual(JSON.parse(calls[0][1].body), { email: "private@example.invalid", concerns: "Jane's profile needs correction", type: "correct" });
  } finally { globalThis.fetch = original; }
});

test("actual proxy keeps privacy submissions anonymous, bounded and nonpublic", async () => {
  const server = await isolatedModule("lib/zebra/server.ts");
  const calls = [], receipt = { reference: "pr_0123456789abcdef0123", received_at: "2026-10-04T10:00:00Z", respond_by: "2026-11-03T10:00:00Z" };
  globalThis.__privacyWorkflow = { ...server, upstreamJson: async (...args) => { calls.push(args); return { ...receipt, email: "private@example.invalid", concerns: "Jane", trace: { person: "Jane" } }; } };
  const route = await isolatedModule("app/zebra/api/[operation]/route.ts", "const { HttpError, bodyObject, boundedString, conditionBundle, dropSession, enc, idParam, jsonResponse, readBounded, readBytes, storeSession, upstream, upstreamJson } = globalThis.__privacyWorkflow;\n");
  const context = { params: Promise.resolve({ operation: "privacy" }) };
  const post = (body) => new Request("https://zebratlas.invalid/zebra/api/privacy", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const valid = { email: "private@example.invalid", concerns: "Jane's public profile", type: "remove" };
  try {
    assert.equal((await route.GET(new Request("https://zebratlas.invalid/zebra/api/privacy"), context)).status, 405);
    assert.equal(calls.length, 0);
    for (const type of ["remove", "correct", "object"]) {
      const response = await route.POST(post({ ...valid, type }), context);
      assert.equal(response.status, 202);
      assert.equal(response.headers.get("cache-control"), "private, no-store");
      assert.deepEqual(await response.json(), receipt);
      assert.equal(calls.at(-1)[1], "/api/privacy/requests");
      assert.deepEqual(calls.at(-1)[2], { method: "POST", body: { ...valid, type }, service: "contribute", auth: false });
    }
    for (const body of [{ ...valid, details: "unexpected" }, { ...valid, lang: "de" }, { ...valid, type: "publish" }, { ...valid, type: ["remove"] }, { ...valid, concerns: "界".repeat(667) }, { ...valid, email: "bad email" }, { ...valid, concerns: "" }]) {
      assert.equal((await route.POST(post(body), context)).status, 400);
    }
    assert.equal(calls.length, 3);
  } finally { delete globalThis.__privacyWorkflow; }
});

test("rendered form handler submits the selected private workflow, not a contribution", async () => {
  const catalog = JSON.parse(fs.readFileSync(path.join(root, "messages/zebra/en.json"), "utf8"));
  const source = fs.readFileSync(path.join(root, "components/zebra/Privacy.tsx"), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } }).outputText;
  const calls = [], fixtureModule = { exports: {} };
  const imports = (name) => name === "react" ? { ...React, useState: (initial) => [initial, () => {}] }
    : name === "./Locale" ? { useZebraCatalog: () => catalog, useZebraLocale: () => "en" }
    : name === "@/lib/zebra/client" ? { submitPrivacyRequest: async (body) => { calls.push(body); return { reference: "pr_0123456789abcdef0123", respond_by: "2026-11-03T10:00:00Z" }; } }
    : name === "@/lib/zebra/locale" ? { zebraHref: (href) => href }
    : name === "next/link" ? { default: "a" } : require(name);
  new Function("require", "module", "exports", compiled)(imports, fixtureModule, fixtureModule.exports);
  const findForm = (element) => element?.type === "form" ? element : React.Children.toArray(element?.props?.children).map(findForm).find(Boolean);
  const form = findForm(fixtureModule.exports.PrivacyRequestForm());
  const original = globalThis.FormData;
  globalThis.FormData = class { constructor(form) { this.values = form; } get(key) { return this.values[key] ?? null; } };
  let prevented = false;
  try {
    await form.props.onSubmit({ preventDefault: () => { prevented = true; }, currentTarget: { email: "private@example.invalid", concerns: "Jane's entry", type: "object", details: "not allowed", lang: "de" } });
    assert.ok(prevented);
    assert.deepEqual(calls, [{ email: "private@example.invalid", concerns: "Jane's entry", type: "object" }]);
  } finally { globalThis.FormData = original; }
});

for (const locale of ["en", "de"]) test(`${locale}: actual legal views expose controller and private removal form`, () => {
  const catalog = JSON.parse(fs.readFileSync(path.join(root, `messages/zebra/${locale}.json`), "utf8"));
  const source = fs.readFileSync(path.join(root, "components/zebra/Privacy.tsx"), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } }).outputText;
  const fixtureModule = { exports: {} };
  const imports = (name) => name === "./Locale" ? { useZebraCatalog: () => catalog, useZebraLocale: () => locale }
    : name === "@/lib/zebra/client" ? { submitPrivacyRequest: () => { throw Error("No live requests in render check"); } }
    : name === "@/lib/zebra/locale" ? { zebraHref: (href, lang) => `${href}?lang=${lang}` }
    : name === "next/link" ? { default: ({ children, ...props }) => React.createElement("a", props, children) } : require(name);
  new Function("require", "module", "exports", compiled)(imports, fixtureModule, fixtureModule.exports);
  const html = renderToStaticMarkup(React.createElement(fixtureModule.exports.PrivacyInformation));
  assert.ok(html.includes(catalog.privacy.profiles));
  assert.ok(html.includes(`href="/zebra/request-removal?lang=${locale}"`));
  assert.ok(html.includes(`mailto:${catalog.privacy.imprint.email}`));
  assert.ok(html.includes(catalog.privacy.imprint.operator));
  const form = renderToStaticMarkup(React.createElement(fixtureModule.exports.PrivacyRequestForm));
  assert.ok(form.includes('method="post"'));
  for (const field of ["email", "concerns", "type"]) assert.ok(form.includes(`name="${field}"`));
  for (const type of ["remove", "correct", "object"]) assert.ok(form.includes(`value="${type}"`));
  assert.ok(!form.includes('name="details"') && !form.includes('name="lang"'));
  assert.ok(form.includes(catalog.privacy.handling));
  const imprint = renderToStaticMarkup(React.createElement(fixtureModule.exports.PrivacyInformation, { imprint: true }));
  assert.ok(imprint.includes(catalog.privacy.imprint.street));
});
