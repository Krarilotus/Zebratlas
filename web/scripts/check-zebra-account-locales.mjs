// Contract fixtures only: every network request and cookie store is mocked.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const require = createRequire(import.meta.url);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const loaded = new Map();
const jar = new Map();
const cookieWrites = [];
const calls = [];
let nextReply = { status: 200, body: {} };
process.env.ACCOUNTS_API_URL = "http://accounts.fixture.invalid";
process.env.ZEBRA_BACKEND_URL = "http://atlas.fixture.invalid";
delete process.env.ZEBRA_LLM_KEY;
delete process.env.ZEBRA_LLM_CONNECTION;
globalThis.fetch = async (url, init = {}) => {
  calls.push({ url: String(url), init, body: init.body ? JSON.parse(init.body) : null });
  return new Response(JSON.stringify(nextReply.body), { status: nextReply.status, headers: { "content-type": "application/json", ...nextReply.headers } });
};
const mocks = {
  "server-only": {},
  "next/headers": {
    cookies: async () => ({ get: (key) => jar.has(key) ? { value: jar.get(key) } : undefined, set: (...args) => { cookieWrites.push(args); jar.set(args[0], args[1]); }, delete: (key) => jar.delete(key) }),
    headers: async () => new Headers({ origin: "http://web.fixture.invalid" }),
  },
  "next/cache": { refresh() {} },
  "next/navigation": { redirect() { throw new Error("Unexpected authentication redirect"); } },
  "@/lib/adapt": {},
  "@/lib/zebra/normalize": { sourceDates: (value) => value },
};
function load(name, from = root) {
  if (name.startsWith(".")) name = "@/" + relative(root, resolve(from, name)).replaceAll("\\", "/");
  if (Object.hasOwn(mocks, name)) return mocks[name];
  if (!name.startsWith("@/")) return require(name);
  if (loaded.has(name)) return loaded.get(name).exports;
  const file = resolve(root, name.slice(2) + ".ts");
  const source = readFileSync(file, "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, esModuleInterop: true } }).outputText;
  const unit = { exports: {} };
  loaded.set(name, unit);
  new Function("require", "module", "exports", compiled)((dependency) => load(dependency, dirname(file)), unit, unit.exports);
  return unit.exports;
}
const route = load("@/app/zebra/api/[operation]/route");
async function post(operation, payload, reply) {
  nextReply = reply;
  const response = await route.POST(new Request(`http://web.fixture.invalid/zebra/api/${operation}`, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(payload),
  }), { params: Promise.resolve({ operation }) });
  return { status: response.status, body: await response.json() };
}

const { LOCALES } = load("@/lib/i18n/config");
const { zebraCopyLocale } = load("@/lib/zebra/locale-policy");
for (const locale of LOCALES) {
  const delivery = zebraCopyLocale(locale);
  for (const operation of ["verify-email", "resend-verification", "forgot-password", "reset-password"]) {
    const payload = operation === "verify-email" ? { token: "fixture-token", locale: delivery } : operation === "reset-password" ? { token: "fixture-token", new_password: "fixture-password", locale: delivery } : { email: "fixture@example.invalid", locale: delivery };
    const response = await post(operation, payload, { status: 200, body: { accepted: true } });
    assert.ok(response.status === 200 || response.status === 202, `${locale} ${operation}: accepted delivery language`);
    assert.equal(calls.at(-1).body.locale, delivery);
  }
}
console.log("All twelve UI locales pass four mocked account-email routes with EN/DE delivery language");
