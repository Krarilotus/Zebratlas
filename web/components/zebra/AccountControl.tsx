"use client";

import type { Ref } from "react";
import { profileDisplayName, profileInitial } from "@/lib/zebra/profile-avatar";
import { useAccountIdentity } from "./AccountProvider";
import { ProfileAvatar } from "./ProfileAvatar";
import { useZebraCopy, useZebraLocale } from "./Locale";
import styles from "./AccountControl.module.css";

export function AccountControl({ className = "", onClick, expanded, controls, buttonRef }: {
  className?: string; onClick: () => void; expanded: boolean; controls: string; buttonRef?: Ref<HTMLButtonElement>;
}) {
  const copy = useZebraCopy(); const locale = useZebraLocale();
  const state = useAccountIdentity();
  const user = state?.state === "signed_in" ? state.account.user : null;
  const name = user ? profileDisplayName(user) : "";
  const initial = profileInitial(name, locale);
  const label = user ? `${copy.menu} · ${copy.account} · ${name}` : state?.state === "signed_out" ? `${copy.menu} · ${copy.signIn}` : copy.menu;
  return <button ref={buttonRef} type="button" className={`${styles.control} ${className}`} onClick={onClick} aria-expanded={expanded} aria-controls={controls} aria-label={label} title={label}>
    {user ? <span className={styles.picture}><ProfileAvatar name={name} userId={user.id} size={36} decorative /><span className={styles.initial} aria-hidden="true">{initial}</span></span> : <svg width="23" height="23" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" aria-hidden="true"><circle cx="12" cy="8" r="3.2" /><path d="M5 21v-3a7 7 0 0 1 14 0v3" /></svg>}
  </button>;
}
