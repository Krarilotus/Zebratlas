/** Preserve documented retrieval times; older journey adapters supply today's date for missing data. */
export function sourceDates<T>(mapped: T, raw: unknown): T {
  const dates = new Map<string, string>();
  function remember(key: string, date: string): void {
    // Reused identifiers with conflicting retrieval dates are not enough to attribute a date.
    const previous = dates.get(key);
    dates.set(key, previous !== undefined && previous !== date ? "" : date);
  }
  function collect(value: unknown, assertion?: string): void {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) { value.forEach(item => collect(item, assertion)); return; }
    const object = value as Record<string, unknown>;
    const scopedAssertion = [object.assertion, object.edge_id, object.key].find(v => typeof v === "string" && !!v) as string | undefined ?? assertion;
    const date = [object.retrieved_on, object.retrieved_at, object.fetched_at].find((v) => typeof v === "string" && !!v);
    if (typeof date === "string") {
      for (const key of [object.id, object.edge_id, object.key, object.record]) if (typeof key === "string") remember(key, date);
      const record = typeof object.record === "string" ? object.record : typeof object.id === "string" ? object.id : undefined;
      if (scopedAssertion && record) remember(`${scopedAssertion}#${record}`, date);
    }
    Object.values(object).forEach(item => collect(item, scopedAssertion));
  }
  collect(raw);
  function clean(value: unknown): void {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) { value.forEach(clean); return; }
    const object = value as Record<string, unknown>;
    if ("retrieved_on" in object) {
      if (typeof object.record === "string") {
        const key = typeof object.assertion === "string" ? `${object.assertion}#${object.record}` : object.record;
        // Never substitute an assertion's first-record timestamp for an independently scoped record.
        object.retrieved_on = dates.get(key) ?? dates.get(object.record) ?? "";
      } else object.retrieved_on = typeof object.id === "string" ? dates.get(object.id) ?? "" : "";
    }
    Object.values(object).forEach(clean);
  }
  clean(mapped);
  return mapped;
}
