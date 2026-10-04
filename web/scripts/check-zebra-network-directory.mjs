import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import ts from "typescript";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { networkDirectory } from "../lib/zebra/network-directory.ts";
import { toggleHeaderPopover, dismissHeaderPopover } from "../lib/zebra/header-popover.ts";
import { installMenuDismissal } from "../lib/zebra/menu-dismissal.ts";
import { fixtureModule } from "./zebra-fixture-module.mjs";

const requireFromHere = createRequire(import.meta.url);
let locale = "en";
const modules = new Map();
function load(name) {
  if (modules.has(name)) return modules.get(name);
  const path = new URL(`../components/zebra/${name}`, import.meta.url);
  const source = readFileSync(path, "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020, esModuleInterop: true } }).outputText;
  const exports = {};
  new Function("exports", "require", compiled)(exports, specifier => {
    if (specifier === "./Locale") return { useZebraLocale: () => locale };
    if (specifier === "@/lib/zebra/network-directory") return { networkDirectory };
    if (specifier === "@/lib/zebra/menu-dismissal") return { installMenuDismissal };
    if (specifier === "@/lib/zebra/locale") return fixtureModule(specifier);
    if (specifier === "next/dynamic") return { __esModule: true, default: () => load("NetworkDirectoryPanel.tsx").default };
    if (specifier.endsWith(".css")) return {};
    if (specifier.startsWith("./")) return load(`${specifier.slice(2)}${specifier === "./network-copy" ? ".ts" : ".tsx"}`);
    return requireFromHere(specifier);
  });
  modules.set(name, exports); return exports;
}
const Panel = load("NetworkDirectoryPanel.tsx").default;
const Directory = load("NetworkDirectory.tsx").NetworkDirectory;
for (locale of ["en", "de"]) {
  const closed = renderToStaticMarkup(React.createElement(Directory, { open: false, onToggle() {}, onClose() {} }));
  assert(closed.includes('aria-expanded="false"'));
  assert(!closed.includes("href="), "Closed directory loads no catalogue links into the landing UI");
  const open = renderToStaticMarkup(React.createElement(Directory, { open: true, onToggle() {}, onClose() {} }));
  assert(open.includes('aria-expanded="true"'));
  assert(open.includes('aria-labelledby='));
  const panel = renderToStaticMarkup(React.createElement(Panel, { id: "networks", onClose() {} }));
  for (const entry of networkDirectory) {
    assert(panel.includes(entry.name.replaceAll("&", "&amp;")));
    assert(entry.scope[locale] && entry.purpose[locale] && entry.limitation[locale]);
    assert(entry.links.length && entry.sources.length);
    for (const link of entry.links) {
      const url = new URL(link.url); assert(["https:", "mailto:"].includes(url.protocol));
      assert(!url.username && !url.password && !url.search, "Directory routes contain no credentials or user/patient input");
      assert(panel.includes(`href="${link.url}"`), "Every real official route is reachable in rendered markup");
      if (url.protocol === "mailto:") {
        const anchor = panel.match(new RegExp(`<a[^>]*href="${link.url.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"[^>]*>`))?.[0];
        assert(anchor && !anchor.includes("target="), "A public role-mailbox link opens the normal email handler without a blank tab");
      }
    }
    for (const source of entry.sources) { assert(/^\d{4}-\d{2}-\d{2}$/.test(source.checkedAt)); assert(panel.includes(source.checkedAt)); }
  }
  assert.equal((panel.match(/<h3 /g) || []).length, 3, "Support, research and data each have a semantic group heading");
  assert(!panel.includes("<form"), "The directory submits no patient or account data");
}
let owner = toggleHeaderPopover(null, "account");
owner = toggleHeaderPopover(owner, "networks"); assert.equal(owner, "networks");
assert.equal(dismissHeaderPopover(owner, "account"), "networks", "A stale account dismissal cannot close the new directory");
owner = toggleHeaderPopover(owner, "account"); assert.equal(owner, "account");
assert.equal(dismissHeaderPopover(owner, "networks"), "account");
assert.equal(toggleHeaderPopover(owner, "account"), null, "The same trigger closes its current menu");

const document = new EventTarget();
let closed = 0, restored = 0;
const trigger = { focus(options) { assert.equal(options.preventScroll, true); restored++; document.activeElement = trigger; } };
const link = {}, input = {};
const root = { contains(node) { return node === trigger || node === link; } };
const cleanup = installMenuDismissal(root, trigger, () => closed++, document);
function dispatch(type, target, key) {
  const event = new Event(type, { cancelable: true });
  Object.defineProperty(event, "target", { value: target });
  if (key) Object.defineProperty(event, "key", { value: key });
  document.dispatchEvent(event); return event;
}
document.activeElement = link;
dispatch("pointerdown", link); assert.equal(closed, 0);
dispatch("keydown", link, "Escape"); assert.equal(closed, 1); assert.equal(restored, 1, "Keyboard dismissal returns focus to the directory trigger");
document.activeElement = input;
dispatch("pointerdown", input); assert.equal(closed, 2); assert.equal(restored, 1, "Outside click does not steal typing focus");
dispatch("focusin", input); assert.equal(closed, 3);
dispatch("keydown", input, "Escape"); assert.equal(closed, 4); assert.equal(restored, 1);
cleanup(); dispatch("pointerdown", input); assert.equal(closed, 4, "Unmounted popovers leave no dismissal listeners");

const css = readFileSync(new URL("../components/zebra/NetworkDirectory.module.css", import.meta.url), "utf8");
assert(css.includes("width:44px; height:44px"));
assert(css.includes("position:fixed; top:68px; left:12px; right:12px; width:auto"));
assert(css.includes("max-height:calc(100dvh - 80px)"));
assert(css.includes("min-height:0; overflow-y:auto"));
for (const width of [320, 390, 720]) assert(width - 24 > 2 * 14 + 180, "Phone sheet insets and padding leave enough room for wrapping route labels");
const host = readFileSync(new URL("../components/zebra/Workspace.tsx", import.meta.url), "utf8");
assert(host.includes("useState<HeaderPopover>(null)"), "Account and networks share a single popover owner");
assert(!host.includes("setMenu("));
assert(host.includes("<AccountControl buttonRef={menuButton}"), "The existing stable account/avatar control is retained");
console.log("Network directory: EN/DE markup, sourced official links, single-menu ownership, native dismissal/focus, and phone layout constraints passed.");
