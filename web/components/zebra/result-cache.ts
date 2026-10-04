/** Response identities are weak keys: cached Back reuses its original source snapshot,
 * while a new search response cannot reuse metadata from a different snapshot. */
export class ResultSnapshotCache<T> {
  private readonly snapshots = new WeakMap<object, Map<string, T>>();
  get(response: object, scope: string): T | undefined { return this.snapshots.get(response)?.get(scope); }
  set(response: object, scope: string, value: T): void {
    const entries = this.snapshots.get(response) || new Map<string, T>();
    entries.set(scope, value);
    if (entries.size > 4) entries.delete(entries.keys().next().value!);
    this.snapshots.set(response, entries);
  }
}
