"use client";

import { createContext, useContext, useEffect, useSyncExternalStore, type ReactNode } from "react";
import { account } from "@/lib/zebra/client";
import { ACCOUNT_CHANGED, accountNeedsRefresh, createAccountReader, forgetAccountIdentity, getAccountIdentity, subscribeAccountIdentity } from "@/lib/zebra/account-events";
import type { AccountState } from "@/lib/zebra/types";

const IdentityContext = createContext<AccountState | null>(null);
const serverIdentity = () => null;
export const useAccountIdentity = () => useContext(IdentityContext);
export function AccountProvider({ children }: { children: ReactNode }) {
  const identity = useSyncExternalStore(subscribeAccountIdentity, getAccountIdentity, serverIdentity);
  useEffect(() => {
    // The client account reader publishes only replies from the current session revision.
    const reader = createAccountReader(account, () => {});
    const changed = () => { if (accountNeedsRefresh()) reader.refresh(); else reader.cancel(); };
    window.addEventListener(ACCOUNT_CHANGED, changed);
    let mounted = true;
    void Promise.resolve().then(() => { if (mounted && accountNeedsRefresh()) reader.refresh(); });
    return () => { mounted = false; window.removeEventListener(ACCOUNT_CHANGED, changed); reader.dispose(); forgetAccountIdentity(); };
  }, []);
  return <IdentityContext.Provider value={identity}>{children}</IdentityContext.Provider>;
}
