// Pure shared renderer: API message references work in server and client views.
import { formatMessage, type Vars } from "./format.ts";
import type { Message, MessageParam, MessageRef } from "../types.ts";

export function catalogText(catalog: unknown, key: string): string | undefined {
  let node: unknown = catalog;
  for (const part of key.split(".")) node = (node as Record<string, unknown> | undefined)?.[part];
  return typeof node === "string" ? node : undefined;
}

export function isMessageRef(value: unknown): value is MessageRef {
  return typeof value === "object" && value !== null && "key" in value && typeof value.key === "string" && "fallback" in value && typeof value.fallback === "string";
}

export function messageKey(catalog: unknown, key: string): string | undefined {
  const backend = `backend.${key}`;
  if (catalogText(catalog, backend) !== undefined) return backend;
  return catalogText(catalog, key) !== undefined ? key : undefined;
}

export function messageLocale(catalog: unknown, key: string, locale: string, original = "en"): string {
  const captured = messageKey(catalog, key);
  const fallback = (catalog as { _meta?: { english_fallback_keys?: string[] } })?._meta?.english_fallback_keys;
  return captured && !fallback?.includes(captured) ? locale : original;
}

export function makeMessageRenderer(catalog: unknown, locale: string) {
  let lists: Intl.ListFormat | undefined;
  const render = (message: Message): string => {
    if (typeof message === "string") return message;
    const key = messageKey(catalog, message.key);
    if (!key) return message.fallback;
    const parameter = (v: MessageParam): string | number => Array.isArray(v)
      ? (lists ??= new Intl.ListFormat(locale, { style: "long", type: "conjunction" })).format(v.map((item) => String(parameter(item))))
      : isMessageRef(v) ? render(v) : typeof v === "boolean" ? String(v) : v ?? "";
    const vars: Vars = Object.fromEntries(Object.entries(message.params ?? {}).map(([k, v]) => [k, parameter(v)]));
    // These selectors mirror the Rust branches; they are derived, never new scientific facts.
    if (message.key === "ask.term.label") vars.plain_state = typeof message.params?.plain === "string" ? "present" : "absent";
    if (message.key === "ask.fact.gene_identity") vars.aliases_count = Array.isArray(message.params?.aliases) ? message.params.aliases.length : 0;
    if (["contribute.check.sample_found", "contribute.check.sample_not_found"].includes(message.key)) {
      const kind = message.params?.kind;
      if (kind === "licence" || kind === "formats" || kind === "identifiers") vars.kind = render({ key: `contribute.check_kind.${kind}`, fallback: kind });
    }
    return formatMessage(catalogText(catalog, key)!, vars, messageLocale(catalog, message.key, locale));
  };
  return render;
}

/** Keep the original diagnostic in the response; display a stable, localizable reason. */
export function askFallbackReason(reason: string): MessageRef {
  const code = reason === "no model requested" ? "no_model"
    : reason === "no facts given" ? "no_facts"
    : reason.startsWith("validator:") ? "validator"
    : reason.startsWith("provider:") ? "provider" : "other";
  return { key: `ask.validation.${code}`, fallback: reason };
}
