import type { AccountState } from "./types";

export const ACCOUNT_CHANGED = "zebra-account-changed";
let identity: AccountState | null = null;
let revision = 0;
let needsRefresh = true;
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((changed) => changed());
export const getAccountIdentity = () => typeof window === "undefined" ? null : identity;
export const getAccountRevision = () => revision;
export const accountNeedsRefresh = () => needsRefresh;
export function subscribeAccountIdentity(changed: () => void) { listeners.add(changed); return () => { listeners.delete(changed); }; }
export function isAccountIdentity(value: unknown): value is AccountState {
  if (!value || typeof value !== "object" || "error" in value || "detail" in value || !("state" in value)) return false;
  if (value.state === "signed_out") return true;
  if (value.state !== "signed_in" || !("account" in value) || !value.account || typeof value.account !== "object" || !("user" in value.account)) return false;
  const user = value.account.user;
  return !!user && typeof user === "object" && "id" in user && typeof user.id === "string" && !!user.id && "email" in user && typeof user.email === "string";
}
export function rememberAccountIdentity(value: AccountState, expectedRevision: number) {
  if (typeof window === "undefined" || expectedRevision !== revision || !isAccountIdentity(value)) return;
  identity = value; needsRefresh = false; notify();
}
/** This cache belongs to the mounted Zebra layout, never to browser storage or another session. */
export function forgetAccountIdentity() { identity = null; needsRefresh = true; revision++; notify(); }
export function accountChanged(value?: AccountState) {
  if (typeof window === "undefined") return;
  if (value && !isAccountIdentity(value)) return;
  revision++; needsRefresh = !value;
  if (value) { identity = value; notify(); }
  window.dispatchEvent(new Event(ACCOUNT_CHANGED));
}

/** Loading and transport failures preserve the last verified identity; stale replies are ignored. */
export function createAccountReader<T>(read: (signal: AbortSignal) => Promise<T>, publish: (value: T | null) => void) {
  let current: AbortController | null = null;
  let closed = false;
  return {
    refresh() {
      if (closed) return;
      current?.abort();
      const next = new AbortController(); current = next;
      void read(next.signal).then((value) => { if (!closed && !next.signal.aborted) publish(value); })
        .catch(() => {});
    },
    cancel() { current?.abort(); },
    dispose() { closed = true; current?.abort(); },
  };
}
