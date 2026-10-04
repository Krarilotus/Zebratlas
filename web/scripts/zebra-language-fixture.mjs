// Test fixtures load the actual language helpers and catalogs; no translation logic is mocked.
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
const web = new URL("../", import.meta.url);
export function languagePrelude(includeErrors = false) {
  const source = readFileSync(new URL("lib/zebra/locale.ts", web), "utf8");
  const declarations = [...source.matchAll(/import (\w+) from "@\/messages\/zebra\/([^"]+)";/g)]
    .map(([, name, file]) => `const ${name} = ${readFileSync(new URL(`messages/zebra/${file}`, web), "utf8")};`).join("\n");
  const config = stripTypeScriptTypes(readFileSync(new URL("lib/i18n/config.ts", web), "utf8"), { mode: "transform" });
  const locale = stripTypeScriptTypes(source.replace(/^import .*;\r?$/gm, "").replace(/^export \{.*\} from .*;\r?$/gm, ""), { mode: "transform" });
  if (!includeErrors) return config + "\n" + declarations + "\n" + locale + "\n";
  const errors = stripTypeScriptTypes(readFileSync(new URL("lib/zebra/error-copy.ts", web), "utf8").replace(/^import .*;\r?$/gm, ""), { mode: "transform" });
  return `import { formatMessage } from ${JSON.stringify(new URL("lib/i18n/format.ts", web).href)};\n` + config + "\n" + declarations + "\n" + locale + "\n" + errors + "\n";
}
