import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
function load(path, dependencies = {}) {
  const compiledUnit = { exports: {} };
  const source = ts.transpileModule(readFileSync(new URL(path, import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
  new Function('exports', 'require', 'module', source)(compiledUnit.exports, name => dependencies[name], compiledUnit);
  return compiledUnit.exports;
}
const config = load('../lib/i18n/config.ts');
const { preferredZebraLocale, zebraCopyLocale } = load('../lib/zebra/locale-policy.ts', { '@/lib/i18n/config': config });
assert.equal(config.LOCALES.length, 12);
for (const locale of config.LOCALES) {
  assert.equal(preferredZebraLocale(locale, 'de', 'en'), locale);
  assert.ok(config.LOCALE_META[locale].name);
}
assert.equal(preferredZebraLocale(null, 'fr', 'de'), 'fr');
assert.equal(preferredZebraLocale('unsupported', 'pt', 'de'), 'pt');
assert.equal(preferredZebraLocale(null, null, 'fr-CA;q=0.3, ja;q=0.9'), 'ja');
assert.equal(preferredZebraLocale(null, null, 'zh-TW'), 'zh-Hans');
assert.equal(preferredZebraLocale(null, null, null), 'en');
assert.equal(config.LOCALE_META.ar.dir, 'rtl');
const { installMenuDismissal } = load('../lib/zebra/menu-dismissal.ts');
const handlers = new Map(); let closed = 0, focused = 0;
const inside = {}, outside = {};
const document = { activeElement: outside, addEventListener: (key, fn) => handlers.set(key, fn), removeEventListener: key => handlers.delete(key) };
const root = { contains: node => node === inside };
const stop = installMenuDismissal(root, { focus: () => focused++ }, () => closed++, document);
handlers.get('keydown')({ key: 'Escape' }); assert.equal(focused, 0);
document.activeElement = inside;
handlers.get('keydown')({ key: 'Escape', preventDefault() {}, stopPropagation() {} });
assert.equal(focused, 1); assert.equal(closed, 2); stop(); assert.equal(handlers.size, 0);
const provider = readFileSync(new URL('../components/zebra/Locale.tsx', import.meta.url), 'utf8');
assert.match(provider, /url\.searchParams\.set\("lang", resolved\)/);
assert.match(provider, /url\.pathname.*url\.search.*url\.hash/);
assert.doesNotMatch(provider, /\.focus\(/);
console.log('Locale priority, twelve choices, RTL, Escape/focus and URL preservation checks passed');

for (const locale of config.LOCALES) assert.equal(zebraCopyLocale(locale), locale === "de" ? "de" : "en");
const authView = readFileSync(new URL("../components/zebra/AccountAuth.tsx", import.meta.url), "utf8");
assert.match(authView, /deliveryLocale = zebraCopyLocale\(locale\)/);
for (const action of ["verifyEmail", "resendVerification", "forgotPassword", "resetPassword"]) assert.match(authView, new RegExp(action + "\\([^;\\n]*deliveryLocale"));
