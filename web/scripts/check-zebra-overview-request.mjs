import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const locales = ["en", "de", "es", "fr", "pt", "it", "zh-Hans", "ja", "hi", "ar", "ru", "tr"];
const exports = {};
const code = ts.transpileModule(readFileSync(new URL("../lib/zebra/overview-request.ts", import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
runInNewContext(code, { exports, require: id => { assert.equal(id, "./locale"); return { ZEBRA_LOCALES: locales }; } });
const { overviewRequest } = exports;
for (const lang of locales) {
  assert.equal(JSON.stringify(overviewRequest({ id: "MONDO:0012812", lang })), JSON.stringify({ id: "MONDO:0012812", lang, enhance: false }));
  assert.equal(overviewRequest({ id: "HGNC:11444", lang, enhance: true }).enhance, true);
}
for (const value of [
  {}, { id: "HGNC:11444", lang: "xx" }, { id: "HGNC:11444", lang: "en", enhance: "yes" },
  { id: "HGNC:11444", lang: "en", query: "private original question" },
  { id: "HGNC:11444", lang: "en", document: "private attachment" },
  ...["STXBP1", " HGNC:11444", "https://other.example/entity", "HGNC:11444\n", "HGNC:" + "1".repeat(256)].map(id => ({ id, lang: "en" })),
]) assert.equal(overviewRequest(value), null, "Only a canonical ID, interface language and explicit enhancement flag enter this route");
console.log("PASS overview request: all 12 interface languages, canonical IDs, no private search or document fields");
