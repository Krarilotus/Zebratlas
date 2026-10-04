import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../components/zebra/Graph.module.css", import.meta.url), "utf8");
const base = css.match(/\.graph\s*\{([^}]+)/)?.[1] ?? "";
const dark = css.match(/\.graph\[data-theme="dark"\][^{]*\{([^}]+)/)?.[1] ?? "";
function palette(block) {
  const values = {};
  for (const [, key, value] of block.matchAll(/--g-label-([a-z]+):([^;]+)/g)) {
    const hex = value.match(/#[\da-f]{6}/i)?.[0];
    if (hex) values[key] = hex;
  }
  return values;
}
function luminance(hex) {
  const channels = hex.slice(1).match(/../g).map(value => parseInt(value, 16) / 255)
    .map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4);
  return .2126 * channels[0] + .7152 * channels[1] + .0722 * channels[2];
}
function contrast(a, b) {
  const left = luminance(a), right = luminance(b);
  return (Math.max(left, right) + .05) / (Math.min(left, right) + .05);
}
for (const [name, colors] of [["light", palette(base)], ["dark", palette(dark)]]) {
  for (const [text, surface] of [["ink", "surface"], ["muted", "surface"], ["active", "surface"], ["active", "hover"], ["ink", "focus"]]) {
    assert.ok(colors[text] && colors[surface], `${name}: missing ${text}/${surface} palette`);
    assert.ok(contrast(colors[text], colors[surface]) >= 4.5, `${name}: insufficient ${text}/${surface} text contrast`);
  }
}
// Text emphasis must not change the actual graph geometry, shared zoom layer,
// or dot hit areas. A focused name remains a real, visible button label.
for (const [, selector, body] of css.matchAll(/([^{}]+)\{([^{}]+)\}/g)) {
  if (selector.includes(":hover") || selector.includes(":focus-visible")) {
    assert.ok(!/(?:^|;)\s*(?:transform|width|height)\s*:/.test(body), `Geometry-changing emphasis: ${selector}`);
  }
}
const dot = css.match(/\.nodeDot\s*\{([^}]+)/)?.[1] ?? "";
assert.match(dot, /width:\s*6px/); assert.match(dot, /height:\s*6px/);
assert.match(css, /\.node:hover strong,\.node:focus-visible strong\s*\{[^}]*font-size:14px/);
assert.match(css, /\.node\s*\{[^}]*pointer-events:\s*auto/);
assert.match(css, /@media\(forced-colors:active\)/);
console.log("Zebra graph label contrast, focus and unchanged geometry checks passed.");
