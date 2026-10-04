// D44: one theme source. Fails when a colour value or an inline style appears outside design/tokens.css:
// hex colours, rgb()/hsl()/oklch(), the white/black keywords (CSS values and Tailwind classes such as
// bg-white), and style={...} in components. Run: node scripts/check-colors.ts (part of `npm run build`).
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const WEB = join(import.meta.dirname, "..");
const ROOTS = ["app", "components", "lib", "design"];
const TOKENS = join("design", "tokens.css");
// Places that cannot read CSS variables; each mirrors tokens.css and says so on the same line.
const MIRROR = "mirrors tokens.css";

const RULES: [RegExp, string][] = [
  [/#[0-9a-fA-F]{3,8}\b/, "hex colour"],
  [/\b(?:rgba?|hsla?|oklch|oklab|lab|lch)\(/, "colour function"],
  [/(?:[:\s"'`]|^)(?:white|black)(?:[;\s"'`]|$)/, "white/black keyword"],
  [/\b(?:bg|text|border|fill|stroke|ring|outline|from|to|via)-(?:white|black)\b/, "white/black class"],
  [/\bstyle=\{/, "inline style"],
];

function* files(dir: string): Generator<string> {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) yield* files(p);
    else if (/\.(tsx?|css|mjs)$/.test(name)) yield p;
  }
}

const errors: string[] = [];
for (const root of ROOTS) {
  for (const file of files(join(WEB, root))) {
    const rel = relative(WEB, file);
// The selected Astra UI owns its captured palette and graph styles.
    // Keep the legacy D44 guard for every other application surface.
    if (rel === TOKENS || /^(app|components|lib)[\\/]zebra[\\/]/.test(rel)) continue;
    readFileSync(file, "utf8")
      .split(/\r?\n/)
      .forEach((line, i) => {
        if (line.includes(MIRROR)) return;
        // Skip comments and SVG id-like hashes in URLs ("#card-…"), which are not colours.
        const code = line.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
        for (const [re, what] of RULES) {
          const m = re.exec(code);
          if (!m) continue;
          // "#card-…" and other anchors are not colours: the token after # must be all hex digits.
          if (what === "hex colour" && /[^0-9a-fA-F]/.test(/#([\w-]+)/.exec(code.slice(m.index))![1])) continue;
          errors.push(`${rel}:${i + 1}: ${what}: ${line.trim().slice(0, 120)}`);
        }
      });
  }
}

if (errors.length > 0) {
  console.error(`colour check failed (${errors.length}): every colour comes from design/tokens.css (D44)\n${errors.join("\n")}`);
  process.exit(1);
}
console.log("colour check passed: no raw colours or inline styles outside design/tokens.css");
