"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import { account, logout, savedItems, removeSaved, accountExportHref, conversations, conversation } from "@/lib/zebra/client";
import type { AccountState, SavedItem, ConversationSummary } from "@/lib/zebra/types";
import type { Conversation } from "@/components/account/types";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { Icon } from "./Icon";
import { ZebraLoader } from "./ZebraLoader";
import { PrivacyInformation, PrivacyNotice, PrivacyRequestForm } from "./Privacy";
import { ModelSettings } from "./ModelSettings";
import { zebraHref } from "@/lib/zebra/locale";
import { SavedDraft } from "./SavedDraft";
import { Brand, BrandText } from "./Brand";
import { AccountProfile } from "./AccountProfile";
import { AboutData } from "./AboutData";
import { ContributionForm } from "./ContributionForm";
import { AccountAuth, type AccountAuthMode } from "./AccountAuth";
import { consumeAccountLink, type AccountLink } from "@/lib/zebra/account-link";

type View = "contribute" | "saved" | "account" | "about" | "privacy" | "request-removal" | "imprint";
const safePath = (path: unknown) => typeof path === "string" && (path === "/zebra" || path.startsWith("/zebra/") || path.startsWith("/zebra?")) ? path : null;
export function Utility({ view }: { view: View }) {
  const copy = useZebraCopy();
  const locale = useZebraLocale();
  const [state, setState] = useState<AccountState | null>(null);
  const [items, setItems] = useState<SavedItem[]>([]);
  const [history, setHistory] = useState<ConversationSummary[]>([]);
  const [thread, setThread] = useState<Conversation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [authMode, setAuthMode] = useState<AccountAuthMode>("login");
  const [accountLink, setAccountLink] = useState<AccountLink | null>(null);
  const accountLinkRead = useRef(false);
  const [unavailable, setUnavailable] = useState(false);
  const [loadedView, setLoadedView] = useState<View | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    if (view === "account" || view === "saved") {
      if (view === "account" && !accountLinkRead.current) {
        void Promise.resolve().then(() => {
          if (controller.signal.aborted || accountLinkRead.current) return;
          accountLinkRead.current = true;
          const link = consumeAccountLink(window.location.href, (path) => window.history.replaceState(window.history.state, "", path));
          if (link) setAccountLink(link);
          const mode = new URL(window.location.href).searchParams.get("mode");
          if (mode === "verify-email" || mode === "forgot-password") setAuthMode(mode);
        });
      }
      void account(controller.signal).then(async (value) => {
        if (controller.signal.aborted) return;
        setState(value);
        if (value.state === "signed_in") {
          if (view === "saved") { const saved = await savedItems(controller.signal); if (!controller.signal.aborted) setItems(saved); }
          else { const recent = await conversations(controller.signal).catch(() => []); if (!controller.signal.aborted) setHistory(recent); }
        }
        if (!controller.signal.aborted) { setUnavailable(false); setLoadedView(view); }
      }).catch(() => { if (!controller.signal.aborted) { setUnavailable(true); setLoadedView(view); } });
    }
    return () => controller.abort();
  }, [view]);
  const heading = view === "contribute" ? copy.contributeTitle : view === "saved" ? copy.saved : view === "account" ? state?.state === "signed_in" && !accountLink ? copy.account : authMode === "login" ? copy.signIn : authMode === "signup" ? copy.signUp : authMode === "verify-email" ? copy.verifyEmail : authMode === "forgot-password" ? copy.forgotPassword : copy.resetPassword : view === "privacy" ? copy.privacy : view === "request-removal" ? copy.privacyRemoval : view === "imprint" ? copy.imprint : copy.about;
  return <section className={`z-utility z-utility-${view}`}>
    <Link href="/zebra" className="z-back"><Icon name="back" size={16} />{copy.search}</Link>
    <header className="z-utility-heading"><p className="z-eyebrow"><Brand name={copy.app} /></p><h1><BrandText text={heading} /></h1>{view === "contribute" && <p>{copy.contributeLead}</p>}{view === "account" && state?.state !== "signed_in" && <p>{copy.signInLead}</p>}</header>
    {error && <p className="z-error" role="alert">{error}</p>}
    {view === "privacy" && <PrivacyInformation />}
    {view === "imprint" && <PrivacyInformation imprint />}
    {view === "request-removal" && <PrivacyRequestForm />}
    {view === "contribute" && <ContributionForm />}
    {view === "saved" && <>
      {loadedView !== view ? <div className="z-inline-loading" role="status"><ZebraLoader />{copy.loading}</div> : unavailable ? <p className="z-empty-note">{copy.accountUnavailable}</p> : state?.state === "signed_out" ? <div className="z-section"><p>{copy.savedSignedOut}</p><Link className="z-button z-button-primary" href="/zebra/account">{copy.signIn}<Icon name="arrow" size={16} /></Link></div> : <>
        {!items.length && <p className="z-empty-note">{copy.savedEmpty}</p>}
        <ul className="z-saved-list">{items.map((item) => {
          const href = item.kind === "search" ? zebraHref(`/zebra?saved=${encodeURIComponent(item.id)}`, locale)
            : item.refs[0] ? zebraHref(`/zebra?q=${encodeURIComponent(item.refs[0])}&node=${encodeURIComponent(item.refs[0])}`, locale) : safePath(item.payload?.href);
          return <li key={item.id}><div><span className="z-eyebrow">{item.kind.replaceAll("_", " ")}</span>{href ? <Link href={href}>{item.title}</Link> : <strong>{item.title}</strong>}{typeof item.payload?.body === "string" && <p className="z-saved-body">{item.payload.body}</p>}{item.note && <p>{item.note}</p>}<span className="z-row-meta">{item.created_at.slice(0, 10)}</span>{item.kind === "message_draft" && <SavedDraft item={item} onUpdated={(updated) => setItems((current) => current.map((saved) => saved.id === updated.id ? updated : saved))} />}</div><button className="z-text-button" disabled={busy} onClick={() => { setBusy(true); void removeSaved(item.id).then(() => setItems((current) => current.filter((other) => other.id !== item.id))).catch((e: unknown) => setError(e instanceof Error ? e.message : copy.accountError)).finally(() => setBusy(false)); }}>{copy.remove}</button></li>;
        })}</ul>
      </>}
    </>}
    {view === "account" && <>
      {loadedView !== view && !accountLink ? <div className="z-inline-loading" role="status"><ZebraLoader />{copy.loading}</div> : unavailable && !accountLink ? <p className="z-empty-note">{copy.accountUnavailable}</p> : state?.state === "signed_in" && !accountLink ? <>
        <AccountProfile key={state.account.user.id} account={state.account} disabled={busy} onUpdated={setState} onBusyChange={setBusy} onSignedOut={() => { setState({ state: "signed_out" }); setHistory([]); setThread(null); }} />
        <ModelSettings />
        <div className="z-inline-actions"><Link href={zebraHref("/zebra/saved", locale)} className="z-text-button">{copy.saved}<Icon name="arrow" size={15} /></Link><a href={accountExportHref} className="z-text-button">{copy.exportAccount}<Icon name="arrow" size={15} /></a><button className="z-text-button" disabled={busy} onClick={() => { setBusy(true); void logout().then(() => { setState({ state: "signed_out" }); setHistory([]); setThread(null); }).catch((e: unknown) => setError(e instanceof Error ? e.message : copy.accountError)).finally(() => setBusy(false)); }}>{copy.signOut}</button></div>
        {!!history.length && <section className="z-section"><h2>{copy.questions}</h2><ul className="z-simple-list">{history.map((item) => <li key={item.id}><button className="z-text-button" onClick={() => void conversation(item.id).then(setThread).catch((e: unknown) => setError(e instanceof Error ? e.message : copy.accountError))}>{item.title}<Icon name="chevron" size={14} /></button></li>)}</ul></section>}
        {thread && <section className="z-section"><h3>{thread.title}</h3>{thread.messages.map((message) => <div className="z-conversation-message" key={message.id}><p className="z-eyebrow">{message.role}</p><p>{message.content}</p>{message.citations.map((citation, index) => typeof citation.url === "string" && /^https?:\/\//.test(citation.url) ? <a key={index} href={citation.url} target="_blank" rel="noopener noreferrer">{citation.label || citation.source || citation.id}</a> : null)}</div>)}</section>}
      </> : <AccountAuth key={accountLink?.flow ?? "auth"} initialMode={authMode} link={accountLink} onModeChange={setAuthMode} onSignedIn={(value) => { setAccountLink(null); setState(value); }} onReset={() => { setState({ state: "signed_out" }); setHistory([]); setThread(null); }} />}
    </>}
    {view === "about" && <div className="z-about"><h2>{copy.aboutLead}</h2><p><BrandText text={copy.aboutBody} /></p><div className="z-inline-actions"><Link href={zebraHref("/zebra/community", locale)} className="z-text-button">{copy.community}<Icon name="arrow" size={16} /></Link><Link href={zebraHref("/zebra/contribute", locale)} className="z-text-button">{copy.contribute}<Icon name="arrow" size={16} /></Link></div><PrivacyNotice /><AboutData /></div>}
  </section>;
}
