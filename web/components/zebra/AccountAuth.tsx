"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale-policy";

import { ZebraLoader } from "./ZebraLoader";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { auth, forgotPassword, resendVerification, resetPassword, verifyEmail, ZebraApiError } from "@/lib/zebra/client";
import type { AccountState, EmailActionResult } from "@/lib/zebra/types";
import type { AccountLink } from "@/lib/zebra/account-link";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";

export type AccountAuthMode = "login" | "signup" | "verify-email" | "forgot-password" | "reset-password";
export function AccountAuth({ link, initialMode = "login", onSignedIn, onModeChange, onReset }: {
  link: AccountLink | null;
  initialMode?: AccountAuthMode;
  onSignedIn: (value: AccountState) => void;
  onModeChange: (mode: AccountAuthMode) => void;
  onReset: () => void;
}) {
  const copy = useZebraCopy();
  const locale = useZebraLocale();
  const deliveryLocale = zebraCopyLocale(locale);
  const [mode, setMode] = useState<AccountAuthMode>(link?.flow ?? initialMode);
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<"verification" | "required" | "reset" | "verified" | "password-reset" | null>(null);
  const [linkRejected, setLinkRejected] = useState(false);
  const [error, setError] = useState("");
  const verification = useRef<Promise<EmailActionResult> | null>(null);
  const translateError = (failure: unknown) => failure instanceof ZebraApiError
    ? failure.code === "invalid_token" ? copy.accountLinkInvalid
      : failure.code === "mail_unavailable" ? copy.accountMailUnavailable
      : failure.status === 429 ? copy.accountTryLater
      : failure.status === 401 ? copy.signInFailed
      : copy.accountError
    : copy.accountError;
  useEffect(() => { onModeChange(mode); }, [mode, onModeChange]);
  useEffect(() => {
    if (link?.flow !== "verify-email") return;
    if (!link.token) return;
    // Reuse the one consumption request when development Strict Mode replays effects.
    verification.current ??= verifyEmail(link.token, deliveryLocale);
    let alive = true;
    void verification.current.then(() => { if (alive) setNotice("verified"); })
      .catch((failure: unknown) => { if (alive) { setLinkRejected(failure instanceof ZebraApiError && failure.code === "invalid_token"); setError(failure instanceof ZebraApiError && failure.code === "invalid_token" ? copy.accountLinkInvalid : copy.accountError); } });
    return () => { alive = false; };
  }, [link, deliveryLocale, copy.accountError, copy.accountLinkInvalid]);
  function switchMode(next: AccountAuthMode) {
    setMode(next); setNotice(null); setError("");
    const url = new URL(window.location.href);
    url.searchParams.delete("flow"); url.hash = "";
    window.history.replaceState(window.history.state, "", `${url.pathname}${url.search}`);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const form = event.currentTarget;
    const fields = new FormData(form);
    const nextPassword = String(fields.get("new_password") ?? "");
    if (mode === "reset-password" && nextPassword !== String(fields.get("confirm_password") ?? "")) { setError(copy.passwordMismatch); return; }
    setBusy(true); setError(""); setNotice(null);
    try {
      if (mode === "forgot-password") { await forgotPassword(email.trim(), deliveryLocale); setNotice("reset"); }
      else if (mode === "verify-email") { await resendVerification(email.trim(), deliveryLocale); setNotice("verification"); }
      else if (mode === "reset-password") {
        if (!link?.token) { setError(copy.accountLinkInvalid); return; }
        await resetPassword(link.token, nextPassword, deliveryLocale); form.reset(); onReset(); setNotice("password-reset");
      } else {
        const value = await auth(mode, { email: email.trim(), password: String(fields.get("password") ?? ""), display_name: String(fields.get("display_name") ?? ""), locale: deliveryLocale });
        if (value.state === "verification_required") { setMode("verify-email"); setNotice("verification"); }
        else if (value.state === "signed_in") onSignedIn(value);
      }
    } catch (failure) {
      if (failure instanceof ZebraApiError && failure.code === "email_unverified") { setMode("verify-email"); setNotice("required"); }
      else { if (failure instanceof ZebraApiError && failure.code === "invalid_token") setLinkRejected(true); setError(translateError(failure)); }
    } finally { setBusy(false); }
  }
  const invalidLink = !!link && (!link.token || linkRejected) && (mode === "verify-email" || mode === "reset-password");
  const verified = notice === "verified";
  const complete = verified || notice === "password-reset";
  const verifying = mode === "verify-email" && !!link?.token && !linkRejected && !notice && !error;
  return <>
    {(error || invalidLink) && <p className="z-error" role="alert">{error || copy.accountLinkInvalid}</p>}
    {verifying ? <p className="z-match-note" role="status"><ZebraLoader /> {copy.verifyingEmail}</p> : complete ? <div className="z-form" role="status">
      <p>{verified ? copy.emailVerified : copy.passwordResetComplete}</p>
      <button type="button" className="z-button z-button-primary" onClick={() => switchMode("login")}>{copy.signIn}<Icon name="arrow" size={16} /></button>
    </div> : <>
      {notice && <p className="z-match-note" role="status">{notice === "reset" ? copy.passwordResetSent : notice === "required" ? copy.verificationRequired : copy.verificationSent}</p>}
      <form key={mode} className="z-form" onSubmit={(event) => void submit(event)}>
        {mode === "signup" && <label>{copy.displayName}<input name="display_name" autoComplete="name" maxLength={80} disabled={busy} /></label>}
        {mode !== "reset-password" && <label>{copy.email}<input name="email" type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} required maxLength={254} disabled={busy} /></label>}
        {(mode === "login" || mode === "signup") && <label>{copy.password}<input name="password" type="password" autoComplete={mode === "login" ? "current-password" : "new-password"} required minLength={mode === "signup" ? 10 : 1} maxLength={1024} disabled={busy} />{mode === "signup" && <span className="z-fine-print">{copy.passwordHint}</span>}</label>}
        {mode === "reset-password" && <>
          <label>{copy.newPassword}<input name="new_password" type="password" autoComplete="new-password" required minLength={10} maxLength={1024} disabled={busy || invalidLink} /><span className="z-fine-print">{copy.passwordHint}</span></label>
          <label>{copy.confirmPassword}<input name="confirm_password" type="password" autoComplete="new-password" required minLength={10} maxLength={1024} disabled={busy || invalidLink} /></label>
        </>}
        <button type="submit" className="z-button z-button-primary" disabled={busy || (mode === "reset-password" && invalidLink)}>{busy ? <><ZebraLoader />{copy.authBusy}</> : mode === "login" ? copy.signIn : mode === "signup" ? copy.signUp : mode === "verify-email" ? copy.resendVerification : mode === "forgot-password" ? copy.sendResetLink : copy.resetPassword}<Icon name="arrow" size={16} /></button>
        {mode === "login" && <button type="button" className="z-text-button" disabled={busy} onClick={() => switchMode("forgot-password")}>{copy.forgotPassword}</button>}
        {mode === "reset-password" && invalidLink && <button type="button" className="z-text-button" onClick={() => switchMode("forgot-password")}>{copy.sendResetLink}</button>}
        <button type="button" className="z-text-button" disabled={busy} onClick={() => switchMode(mode === "login" ? "signup" : "login")}>{mode === "login" ? copy.switchSignup : copy.switchSignin}</button>
        {mode === "login" || mode === "signup" ? <p className="z-fine-print">{copy.free}</p> : null}
      </form>
    </>}
  </>;
}
