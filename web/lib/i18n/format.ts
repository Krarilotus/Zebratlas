// A small ICU MessageFormat subset: {name}, {n, plural, =0 {..} one {..} other {..}} with #, and
// {key, select, a {..} other {..}}. Enough for short UI strings; no quoting rules (catalogs avoid
// literal braces). Shared by the server, client components and scripts/check-i18n.mjs (a copy).

export type Vars = Record<string, string | number>;

type Node =
  | { t: "text"; v: string }
  | { t: "var"; name: string }
  | { t: "hash" }
  | { t: "plural"; name: string; options: Record<string, Node[]> }
  | { t: "select"; name: string; options: Record<string, Node[]> };

const cache = new Map<string, Node[]>();

function parse(src: string): Node[] {
  const hit = cache.get(src);
  if (hit) return hit;
  let i = 0;

  function message(inPlural: boolean): Node[] {
    const out: Node[] = [];
    let text = "";
    const flush = () => {
      if (text) out.push({ t: "text", v: text });
      text = "";
    };
    while (i < src.length) {
      const c = src[i];
      if (c === "}") break;
      if (c === "#" && inPlural) {
        flush();
        out.push({ t: "hash" });
        i++;
        continue;
      }
      if (c === "{") {
        flush();
        i++;
        out.push(argument());
        continue;
      }
      text += c;
      i++;
    }
    flush();
    return out;
  }

  function readUntil(stops: string): string {
    let s = "";
    while (i < src.length && !stops.includes(src[i])) s += src[i++];
    return s.trim();
  }

  function argument(): Node {
    const name = readUntil(",}");
    if (src[i] === "}") {
      i++;
      return { t: "var", name };
    }
    i++; // ,
    const type = readUntil(",}");
    if (src[i] !== ",") throw new Error(`ICU: expected options for {${name}, ${type}} in "${src}"`);
    i++;
    const options: Record<string, Node[]> = {};
    for (;;) {
      while (i < src.length && /\s/.test(src[i])) i++;
      if (src[i] === "}") {
        i++;
        break;
      }
      const key = readUntil("{");
      if (src[i] !== "{") throw new Error(`ICU: missing "{" after option ${key} in "${src}"`);
      i++;
      options[key] = message(type === "plural");
      if (src[i] !== "}") throw new Error(`ICU: unclosed option ${key} in "${src}"`);
      i++;
    }
    if (!("other" in options)) throw new Error(`ICU: {${name}, ${type}} needs an "other" option in "${src}"`);
    if (type === "plural") return { t: "plural", name, options };
    if (type === "select") return { t: "select", name, options };
    throw new Error(`ICU: unsupported type ${type} in "${src}"`);
  }

  const nodes = message(false);
  if (i < src.length) throw new Error(`ICU: unbalanced "}" in "${src}"`);
  cache.set(src, nodes);
  return nodes;
}

function render(nodes: Node[], vars: Vars, locale: string, count?: number): string {
  let out = "";
  for (const n of nodes) {
    switch (n.t) {
      case "text":
        out += n.v;
        break;
      case "hash":
        out += count === undefined ? "#" : new Intl.NumberFormat(locale).format(count);
        break;
      case "var": {
        const v = vars[n.name];
        out += v === undefined ? `{${n.name}}` : typeof v === "number" ? new Intl.NumberFormat(locale).format(v) : v;
        break;
      }
      case "plural": {
        const value = Number(vars[n.name] ?? 0);
        const branch = n.options[`=${value}`] ?? n.options[new Intl.PluralRules(locale).select(value)] ?? n.options.other;
        out += render(branch, vars, locale, value);
        break;
      }
      case "select": {
        const key = String(vars[n.name] ?? "other");
        out += render(n.options[key] ?? n.options.other, vars, locale, count);
        break;
      }
    }
  }
  return out;
}

export function formatMessage(message: string, vars: Vars | undefined, locale: string): string {
  if (!vars && !message.includes("{")) return message;
  return render(parse(message), vars ?? {}, locale);
}

/** Variable names a message uses (for the catalog check). */
export function messageVars(message: string): string[] {
  const names = new Set<string>();
  const walk = (nodes: Node[]) => {
    for (const n of nodes) {
      if (n.t === "var") names.add(n.name);
      if (n.t === "plural" || n.t === "select") {
        names.add(n.name);
        Object.values(n.options).forEach(walk);
      }
    }
  };
  walk(parse(message));
  return [...names].sort();
}
