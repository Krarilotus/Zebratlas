import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

// D17: every UI string lives in messages/*.json. This rule fails lint (and so `npm run build`) on
// letters written straight into JSX: text children, string expressions, and user-facing attributes.
// Strings without letters (punctuation such as "·") are allowed. Names and IDs that are never
// translated belong in data, not in JSX.
const LETTER = /\p{L}/u;
const UI_ATTRIBUTES = new Set(["aria-label", "aria-description", "alt", "title", "placeholder", "label", "aria-roledescription", "aria-valuetext"]);

const noLiteralUiText = {
  meta: {
    type: "problem",
    docs: { description: "UI text must come from the message catalogs (t(...))" },
    messages: { literal: 'Literal UI text "{{text}}": move it into messages/en.json and use t(...).' },
    schema: [],
  },
  create(context) {
    const report = (node, text) => context.report({ node, messageId: "literal", data: { text: text.trim().slice(0, 40) } });
    const isStringLiteral = (n) => (n.type === "Literal" && typeof n.value === "string") || (n.type === "TemplateLiteral" && n.expressions.length === 0);
    const valueOf = (n) => (n.type === "Literal" ? n.value : n.quasis.map((q) => q.value.cooked).join(""));
    return {
      JSXText(node) {
        if (LETTER.test(node.value)) report(node, node.value);
      },
      JSXExpressionContainer(node) {
        if (node.parent.type === "JSXAttribute") return;
        const e = node.expression;
        if (isStringLiteral(e) && LETTER.test(valueOf(e))) report(node, valueOf(e));
      },
      JSXAttribute(node) {
        const name = typeof node.name.name === "string" ? node.name.name : "";
        if (!UI_ATTRIBUTES.has(name) || !node.value) return;
        const v = node.value.type === "JSXExpressionContainer" ? node.value.expression : node.value;
        if (isStringLiteral(v) && LETTER.test(valueOf(v))) report(node, valueOf(v));
      },
    };
  },
};

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  {
    files: ["app/**/*.tsx", "components/**/*.tsx"],
    plugins: { atlas: { rules: { "no-literal-ui-text": noLiteralUiText } } },
    rules: { "atlas/no-literal-ui-text": "error" },
  },
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    "test-results/**",
    // Pinned upstream/bundled artifacts; the source adapter is linted in scripts/.
    "public/query-by-graph/*.js",
  ]),
]);

export default eslintConfig;
