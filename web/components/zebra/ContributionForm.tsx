"use client";

import { ZebraLoader } from "./ZebraLoader";
import { useEffect, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { account, submitContribution } from "@/lib/zebra/client";
import { buildContribution, contributionContext, contextLabel, CONTRIBUTION_KINDS, type FormContributionKind } from "@/lib/zebra/contribution-form";
import type { Contribution } from "@/components/contribute/types";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import styles from "./ContributionForm.module.css";

const subscribeLocation = (changed: () => void) => { window.addEventListener("popstate", changed); return () => window.removeEventListener("popstate", changed); };
const locationSearch = () => window.location.search;
export function ContributionForm() {
  const copy = useZebraCopy(), locale = useZebraLocale();
  const search = useSyncExternalStore(subscribeLocation, locationSearch, () => "");
  const context = contributionContext(search);
  const [chosenKind, setKind] = useState<FormContributionKind | null>(null);
  const kind = chosenKind ?? context.kind ?? "correction";
  const [subject, setSubject] = useState<string | null>(null), [target, setTarget] = useState<string | null>(null);
  const [email, setEmail] = useState<string | null>(null), [name, setName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [submitted, setSubmitted] = useState<Contribution | null>(null);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void account(controller.signal).then((value) => {
      if (controller.signal.aborted || value.state !== "signed_in") return;
      setEmail((current) => current ?? value.account.user.email);
      setName((current) => current ?? value.account.user.display_name ?? "");
    }).catch(() => {});
    return () => { controller.abort(); pending.current?.abort(); };
  }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (pending.current) return;
    setError("");
    let input; try { input = buildContribution(new FormData(event.currentTarget), context, locale); }
    catch { setError(copy.contributionInvalid); return; }
    const controller = new AbortController(); pending.current = controller; setBusy(true);
    try {
      const value = await submitContribution(input, controller.signal);
      if (!controller.signal.aborted) setSubmitted(value);
    } catch (failure) {
      if (!controller.signal.aborted) setError(failure instanceof Error && failure.message !== "invalid_contribution_response" ? failure.message : copy.contributionError);
    } finally { if (!controller.signal.aborted) setBusy(false); if (pending.current === controller) pending.current = null; }
  }
  if (submitted) return <div className="z-submitted" role="status"><Icon name="check" size={28} /><h2>{copy.submitted}</h2><p>{copy.contributionPending}</p><dl className="z-dl"><dt>{copy.submissionId}</dt><dd>{submitted.id}</dd></dl>{!!submitted.checks?.checks.length && <details className={`z-form-extra ${styles.checks}`}><summary>{copy.details}</summary>{submitted.checks.checks.map((check, index) => <p key={index}>{check.name}: {check.status}</p>)}</details>}</div>;
  return <form className={`z-form ${styles.form}`} onSubmit={(event) => void submit(event)} aria-busy={busy} aria-describedby={error ? "contribution-error" : undefined}>
    <fieldset disabled={busy}>
      <label>{copy.contributionKind}<select name="kind" value={kind} onChange={(event) => { setKind(event.target.value as FormContributionKind); setError(""); }}>{CONTRIBUTION_KINDS.map((value) => <option key={value} value={value}>{copy.contributionKinds[value]}</option>)}</select></label>
      {kind === "other" && <label>{copy.contributionOther}<input name="kind_other" required maxLength={300} /></label>}
      <div className={kind === "new_link" ? "z-field-pair" : styles.context}>
        <label>{kind === "new_link" ? copy.contributionNewSubject : copy.contributionAbout}<input name="subject" value={subject ?? contextLabel(context.subject)} onChange={(event) => setSubject(event.target.value)} required={kind === "new_link" || kind === "outdated_contact" || (kind === "correction" && !context.edge)} maxLength={300} autoComplete="off" /></label>
        {kind === "new_link" && <label>{copy.condition}<input name="condition" value={target ?? contextLabel(context.target)} onChange={(event) => setTarget(event.target.value)} required maxLength={300} autoComplete="off" /></label>}
        {kind !== "new_link" && <input type="hidden" name="condition" value={target ?? contextLabel(context.target)} />}
      </div>
      <label>{copy.statement}<textarea name="statement" required rows={3} maxLength={2000} /></label>
      <label>{copy.evidenceUrl}<input name="evidence_url" type="url" inputMode="url" maxLength={2000} /><span className="z-fine-print">{copy.contributionSourceOptional}</span></label>
      <label>{copy.contributionEmail}<input name="contact" type="email" autoComplete="email" required maxLength={200} value={email ?? ""} onChange={(event) => setEmail(event.target.value)} /><span className="z-fine-print">{copy.contributionEmailHint}</span></label>
      <details className={`z-form-extra ${styles.optional}`}><summary>{copy.contributionOptionalDetails}</summary><label>{copy.quote}<textarea name="quote" rows={2} maxLength={1000} /></label><label>{copy.contactUrl}<input name="contact_url" type="url" inputMode="url" maxLength={2000} /></label><label>{copy.contributor}<input name="name" autoComplete="name" maxLength={120} value={name ?? ""} onChange={(event) => setName(event.target.value)} /></label></details>
    </fieldset>
    {error && <p id="contribution-error" className="z-error" role="alert">{error}</p>}
    <button className="z-button z-button-primary" type="submit" disabled={busy}>{busy ? <><ZebraLoader />{copy.submitting}</> : copy.submitReview}<Icon name="arrow" size={16} /></button>
  </form>;
}
