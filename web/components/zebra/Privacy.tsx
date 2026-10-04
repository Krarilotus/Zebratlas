"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";
import { submitPrivacyRequest } from "@/lib/zebra/client";
import { zebraHref } from "@/lib/zebra/locale";
import { useZebraCatalog, useZebraLocale } from "./Locale";

export function PrivacyNotice() {
  const { privacy } = useZebraCatalog();
  const locale = useZebraLocale();
  return <div className="z-section"><p>{privacy.profiles}</p><Link className="z-text-button" href={zebraHref("/zebra/request-removal", locale)}>{privacy.remove}</Link></div>;
}

function Controller() {
  const { privacy } = useZebraCatalog();
  const info = privacy.imprint;
  return <address><strong>{info.operator}</strong><br />{info.street}<br />{info.city}<br />{info.country}<br /><a href={`mailto:${info.email}`}>{info.email}</a></address>;
}

export function PrivacyInformation({ imprint = false }: { imprint?: boolean }) {
  const { privacy } = useZebraCatalog();
  const locale = useZebraLocale();
  return <div className="z-about">
    {imprint ? <><p>{privacy.imprint.lead}</p><h2>{privacy.imprint.operatorTitle}</h2><Controller /><p>{privacy.imprint.private}</p></> : <>
      <p>{privacy.lead}</p><PrivacyNotice />
      {privacy.sections.map((section) => <section className="z-section" key={section.title}><h2>{section.title}</h2><p>{section.body}</p></section>)}
      <h2>{privacy.controller}</h2><Controller />
    </>}
    <div className="z-inline-actions"><Link className="z-text-button" href={zebraHref("/zebra/request-removal", locale)}>{privacy.remove}</Link><Link className="z-text-button" href={zebraHref(imprint ? "/zebra/privacy" : "/zebra/imprint", locale)}>{imprint ? privacy.title : privacy.imprintTitle}</Link></div>
  </div>;
}

export function PrivacyRequestForm() {
  const { privacy } = useZebraCatalog();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [receipt, setReceipt] = useState<{ reference: string; respond_by: string } | null>(null);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    setBusy(true); setError(false);
    try {
      const reply = await submitPrivacyRequest({ email: String(data.get("email") ?? ""), concerns: String(data.get("concerns") ?? ""), type: String(data.get("type") ?? "") as "remove" | "correct" | "object" });
      setReceipt(reply);
    } catch { setError(true); } finally { setBusy(false); }
  }
  if (receipt) return <section className="z-section" role="status"><h2>{privacy.received}</h2><p>{privacy.receivedBody}</p><p>{privacy.reference}: <strong>{receipt.reference}</strong></p><p>{privacy.respondBy}: <time dateTime={receipt.respond_by}>{receipt.respond_by.slice(0, 10)}</time></p></section>;
  return <div className="z-about"><p>{privacy.requestLead}</p><p>{privacy.handling}</p><form className="z-form" method="post" onSubmit={(event) => void submit(event)}>
    <label htmlFor="privacy-email">{privacy.email}</label><input id="privacy-email" name="email" type="email" autoComplete="email" required maxLength={254} disabled={busy} />
    <fieldset disabled={busy}><legend>{privacy.type}</legend>{(["remove", "correct", "object"] as const).map((type) => <label key={type}><input name="type" type="radio" value={type} required />{privacy.types[type]}</label>)}</fieldset>
    <label htmlFor="privacy-concerns">{privacy.concerns}</label><p id="privacy-concerns-hint">{privacy.hint}</p><textarea id="privacy-concerns" name="concerns" required maxLength={2000} rows={6} aria-describedby="privacy-concerns-hint" disabled={busy} />
    {error && <p className="z-error" role="alert">{privacy.error}</p>}
    <button className="z-button z-button-primary" type="submit" disabled={busy}>{privacy.submit}</button>
  </form></div>;
}
