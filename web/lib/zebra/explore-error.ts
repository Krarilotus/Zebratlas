export type ExploreErrorKind = "quota" | "provider" | "invalid" | "unsupported" | "unknown";
export type ExploreErrorPresentation = { kind: ExploreErrorKind; retryable: boolean; modelSettings: boolean };
const object = (value: unknown): Record<string, unknown> | null => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;

/** Compatibility with the old provider fault wrapper. Nothing from this payload is returned for display. */
function providerFault(message: unknown): Record<string, unknown> | null {
  if (typeof message !== "string" || message.length > 16384) return null;
  const start = message.indexOf("{");
  if (start < 0) return null;
  let depth = 0, quoted = false, escaped = false;
  for (let i = start; i < message.length; i++) {
    const character = message[i];
    if (quoted) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quoted = false;
    } else if (character === '"') quoted = true;
    else if (character === "{") depth++;
    else if (character === "}" && --depth === 0) {
      try { return object(object(JSON.parse(message.slice(start, i + 1)))?.error); } catch { return null; }
    }
  }
  return null;
}

/** Classify only explicit API codes or a structured legacy provider fault. Raw diagnostics never enter UI copy. */
export function classifyExploreError(error: unknown): ExploreErrorPresentation {
  const fields = object(error);
  const code = fields?.code;
  const legacy = providerFault(typeof error === "string" ? error : fields?.message);
  const legacyMessage = typeof legacy?.message === "string" ? legacy.message.toLowerCase() : "";
  const legacyQuota = legacy?.code === 429 && ["free-models-per-day", "free-models-per-minute", "quota exceeded", "daily request limit"].some(marker => legacyMessage.includes(marker));
  if (code === "model_rate_limited" || legacyQuota) return { kind: "quota", retryable: false, modelSettings: true };
  if (["model_unavailable", "model_not_configured", "model_request_failed", "model_auth_failed"].includes(String(code ?? ""))) return { kind: "provider", retryable: false, modelSettings: true };
  if (code === "unsupported_question") return { kind: "unsupported", retryable: false, modelSettings: false };
  if (fields?.status === 400 || ["invalid_sparql", "invalid_query", "unsupported_question"].includes(String(code ?? ""))) return { kind: "invalid", retryable: true, modelSettings: false };
  return { kind: "unknown", retryable: true, modelSettings: false };
}
