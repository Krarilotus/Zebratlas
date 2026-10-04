"use client";

import { ZebraLoader } from "./ZebraLoader";
import { useState, type FormEvent } from "react";
import { changePassword, logoutAll, updateProfile } from "@/lib/zebra/client";
import type { Account, AccountState } from "@/lib/zebra/types";
import { useSetZebraLocale, useZebraCopy, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import styles from "./AccountProfile.module.css";
import { ProfileAvatar, useProfileAvatarVariation } from "./ProfileAvatar";
import { saveAvatarVariation } from "@/lib/zebra/avatar-preference";
import { profileDisplayName } from "@/lib/zebra/profile-avatar";

export function AccountProfile({ account, disabled, onUpdated, onSignedOut, onBusyChange }: {
  account: Account;
  disabled: boolean;
  onUpdated: (value: AccountState) => void;
  onSignedOut: () => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const copy = useZebraCopy();
  const locale = useZebraLocale();
  const setLocale = useSetZebraLocale();
  const [name, setName] = useState(account.user.display_name ?? "");
  const [language, setLanguage] = useState(account.user.locale || locale);
  const savedVariation = useProfileAvatarVariation(account.user.id);
  const [draftVariation, setDraftVariation] = useState<number | null>(null);
  const variation = draftVariation ?? savedVariation;
  const [operation, setOperation] = useState<"profile" | "password" | "sessions" | null>(null);
  const [message, setMessage] = useState<"profile" | "password" | "avatar" | "profile-avatar" | null>(null);
  const [error, setError] = useState("");
  const busy = disabled || operation !== null;
  const profileChanged = name !== (account.user.display_name ?? "") || language !== (account.user.locale || locale);
  const avatarChanged = variation !== savedVariation;
  const changed = profileChanged || avatarChanged;
  function begin(next: NonNullable<typeof operation>) { setOperation(next); onBusyChange(true); setMessage(null); setError(""); }
  function finish() { setOperation(null); onBusyChange(false); }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    begin("profile");
    try {
      const value = profileChanged ? await updateProfile({ display_name: name.trim() || null, locale: language }) : { state: "signed_in" as const, account };
      if (profileChanged) onUpdated(value);
      if (value.state === "signed_in") {
        setName(value.account.user.display_name ?? "");
        setLanguage(value.account.user.locale || locale);
        if (value.account.user.locale === "en" || value.account.user.locale === "de") setLocale(value.account.user.locale);
        const saved = !avatarChanged || saveAvatarVariation(account.user.id, variation);
        if (saved) { setDraftVariation(null); setMessage(profileChanged ? avatarChanged ? "profile-avatar" : "profile" : "avatar"); }
        else { setError(copy.avatarSaveError); if (profileChanged) setMessage("profile"); }
      }
    } catch (failure) { setError(failure instanceof Error ? failure.message : copy.accountError); }
    finally { finish(); }
  }
  async function password(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const form = event.currentTarget;
    const fields = new FormData(form);
    const next = String(fields.get("new_password") ?? "");
    if (next !== String(fields.get("confirm_password") ?? "")) { setMessage(null); setError(copy.passwordMismatch); return; }
    begin("password");
    try {
      await changePassword({ current_password: String(fields.get("current_password") ?? ""), new_password: next });
      form.reset(); setMessage("password");
    } catch (failure) { setError(failure instanceof Error ? failure.message : copy.accountError); }
    finally { finish(); }
  }
  async function endSessions() {
    if (busy) return;
    begin("sessions");
    try { await logoutAll(); onSignedOut(); }
    catch (failure) { setError(failure instanceof Error ? failure.message : copy.accountError); }
    finally { finish(); }
  }
  return <>
    <div className="z-profile">
      <p className="z-eyebrow">{copy.profile}</p>
      <form className={`z-form ${styles.profileForm}`} onSubmit={(event) => void save(event)}>
        <div className={styles.avatarRow}>
          <ProfileAvatar name={profileDisplayName({ ...account.user, display_name: name })} variation={variation} size={72} label={copy.profilePicture} />
          <div><button type="button" className="z-text-button" disabled={busy} onClick={() => { setDraftVariation((variation + 1) % 1000001); setMessage(null); }}>{copy.generatePicture}</button><p className="z-fine-print">{copy.avatarDeviceOnly}</p></div>
        </div>
        <label>{copy.displayName}<input autoComplete="name" maxLength={80} value={name} disabled={busy} onChange={(event) => { setName(event.target.value); setMessage(null); }} /></label>
        <div className="z-field-pair">
          <label>{copy.email}<input className={styles.email} type="email" autoComplete="email" readOnly value={account.user.email} /></label>
          <label>{copy.language}<select value={language} disabled={busy} onChange={(event) => { setLanguage(event.target.value); setMessage(null); }}>
            {!Object.hasOwn(copy.accountLanguages, language) && <option value={language}>{language}</option>}
            {Object.entries(copy.accountLanguages).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select></label>
        </div>
        <button type="submit" className="z-button z-button-primary" disabled={busy || !changed}>{operation === "profile" ? <><ZebraLoader />{copy.authBusy}</> : copy.save}<Icon name="check" size={15} /></button>
      </form>
    </div>
    {error && <p className="z-error" role="alert">{error}</p>}
    {message && <p className="z-match-note" role="status">{message === "profile" ? copy.profileSaved : message === "avatar" ? copy.avatarSaved : message === "profile-avatar" ? `${copy.profileSaved}. ${copy.avatarSaved}` : copy.passwordUpdated}</p>}
    <details className={`z-form-extra z-section ${styles.security}`}>
      <summary>{copy.accountSecurity}</summary>
      <form className="z-form" onSubmit={(event) => void password(event)}>
        <label>{copy.currentPassword}<input name="current_password" type="password" autoComplete="current-password" required maxLength={1024} disabled={busy} /></label>
        <label>{copy.newPassword}<input name="new_password" type="password" autoComplete="new-password" required minLength={10} maxLength={1024} disabled={busy} /><span className="z-fine-print">{copy.passwordHint}</span></label>
        <label>{copy.confirmPassword}<input name="confirm_password" type="password" autoComplete="new-password" required minLength={10} maxLength={1024} disabled={busy} /></label>
        <button type="submit" className="z-button" disabled={busy}>{operation === "password" ? <><ZebraLoader />{copy.authBusy}</> : copy.changePassword}<Icon name="arrow" size={15} /></button>
      </form>
      <div className="z-inline-actions"><button type="button" className="z-text-button" disabled={busy} onClick={() => void endSessions()}>{operation === "sessions" ? <><ZebraLoader />{copy.authBusy}</> : copy.signOutAll}</button><span className="z-fine-print">{copy.signOutAllHint}</span></div>
    </details>
  </>;
}
