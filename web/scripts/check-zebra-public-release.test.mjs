import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { HISTORICAL_ZEBRA_RELEASE } from "../lib/zebra/published-release.ts";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(process.env.ZEBRA_TEST_WEB ? path.join(process.env.ZEBRA_TEST_WEB, "package.json") : import.meta.url);
const ts = require("typescript"), React = require("react"), { renderToStaticMarkup } = require("react-dom/server");

test("historical receipt stays inspectable without approving the artifact", () => {
  assert.equal(HISTORICAL_ZEBRA_RELEASE.status, "withheld");
  assert.equal(HISTORICAL_ZEBRA_RELEASE.revision, "e7de6d1b07c51e311f8c7b6feed2d1ff16ee6d9d");
  assert.equal(HISTORICAL_ZEBRA_RELEASE.sha256, "990e5b027ccc4f562df1ae11026a4065f4dd3d01e3c94145e2409078f6d09f9b");
  assert.equal(HISTORICAL_ZEBRA_RELEASE.manifestSha256, "8459a39e63343082f23eddab1e915a8d8e2369b2d4e099e7e61f059b6f4429ef");
});

for (const locale of ["en", "de"]) test(`${locale}: rendered About retains evidence and provides no old artifact action`, () => {
  const words = JSON.parse(fs.readFileSync(path.join(root, `messages/zebra/${locale}.json`), "utf8")).copy;
  const source = fs.readFileSync(path.join(root, "components/zebra/AboutData.tsx"), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } }).outputText;
  const fixtureModule = { exports: {} };
  const metadata = { visible: { recordsProof: {}, evidence: {}, nodes: {} }, loaded: { evidence: {}, nodes: {} }, measures: [], classes: {}, adapter: {} };
  const imports = (name) => name === "@/lib/zebra/published-release" ? { HISTORICAL_ZEBRA_RELEASE }
    : name === "./Locale" ? { useZebraCopy: () => words, useZebraLocale: () => locale, useZebraKindLabel: () => (kind) => kind }
    : name === "@/lib/zebra/about-data" ? { aboutMetadata: () => metadata }
    : name === "@/lib/zebra/client" ? { exploreSchema: () => { throw Error("No network in render check"); } }
    : name === "./ZebraLoader" ? { ZebraLoader: () => null }
    : name.endsWith(".css") ? { default: {} } : require(name);
  new Function("require", "module", "exports", compiled)(imports, fixtureModule, fixtureModule.exports);
  const html = renderToStaticMarkup(React.createElement(fixtureModule.exports.AboutData));
  assert.ok(html.includes(words.aboutData.publishedNote));
  assert.ok(html.includes(HISTORICAL_ZEBRA_RELEASE.revision));
  assert.ok(html.includes(HISTORICAL_ZEBRA_RELEASE.sha256));
  const links = [...html.matchAll(/href="([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(links, ["/zebra/api/schema"]);
  assert.ok(!links.some((url) => /huggingface|graph\.ttl|SHA256SUMS/.test(url)));
});
