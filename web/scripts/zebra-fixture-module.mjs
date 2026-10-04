// Load actual pure TypeScript/JSON dependencies in standalone Node fixtures.
import { readFileSync } from "node:fs";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import ts from "typescript";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const requireHere = createRequire(import.meta.url), loaded = new Map();
export function fixtureModule(name, from = root) {
  if (!name.startsWith("@/") && !name.startsWith(".")) return requireHere(name);
  let file = name.startsWith("@/") ? resolve(root, name.slice(2)) : resolve(from, name);
  if (extname(file) === ".json") return JSON.parse(readFileSync(file, "utf8"));
  if (!extname(file)) file += ".ts";
  if (loaded.has(file)) return loaded.get(file);
  const result = {}; loaded.set(file, result);
  const source = ts.transpileModule(readFileSync(file, "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, esModuleInterop: true } }).outputText;
  new Function("exports", "require", source)(result, dependency => fixtureModule(dependency, dirname(file)));
  return result;
}
