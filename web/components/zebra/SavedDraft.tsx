"use client";

import { ZebraLoader } from "./ZebraLoader";
import { useState, type FormEvent } from "react";
import type { SavedItem } from "@/lib/zebra/types";
import { updateSaved } from "@/lib/zebra/client";
import { useZebraCopy } from "./Locale";
import { Icon } from "./Icon";

export function SavedDraft({ item, onUpdated }: { item: SavedItem; onUpdated: (item: SavedItem) => void }) {
  const copy = useZebraCopy();
  const [subject, setSubject] = useState(typeof item.payload.subject === "string" ? item.payload.subject : item.title);
  const [body, setBody] = useState(typeof item.payload.body === "string" ? item.payload.body : "");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  async function save(event: FormEvent) {
    event.preventDefault(); setBusy(true); setNotice("");
    try { const updated = await updateSaved(item.id, { title: subject, payload: { ...item.payload, subject, body } }); onUpdated(updated); setNotice(copy.savedItem); }
    catch (e) { setNotice(e instanceof Error ? e.message : copy.accountError); }
    finally { setBusy(false); }
  }
  async function copyText() { try { await navigator.clipboard.writeText(`${subject}\n\n${body}`); setNotice(copy.copied); } catch (e) { setNotice(e instanceof Error ? e.message : copy.accountError); } }
  function download() { const url = URL.createObjectURL(new Blob([`${subject}\n\n${body}`], { type: "text/plain;charset=utf-8" })); const anchor = document.createElement("a"); anchor.href = url; anchor.download = "zebratlas-research-brief.txt"; anchor.click(); URL.revokeObjectURL(url); }
  return <details className="z-saved-editor"><summary>{copy.editDraft}</summary><form className="z-form" onSubmit={(event) => void save(event)}><label>{copy.subject}<input value={subject} maxLength={200} onChange={(event) => { setSubject(event.target.value); setNotice(""); }} /></label><label>{copy.message}<textarea value={body} rows={12} maxLength={50000} onChange={(event) => { setBody(event.target.value); setNotice(""); }} /></label><div className="z-inline-actions"><button className="z-button z-button-primary" disabled={busy}>{busy ? <><ZebraLoader />{copy.authBusy}</> : copy.save}<Icon name="check" size={15} /></button><button type="button" className="z-text-button" onClick={() => void copyText()}>{copy.copy}</button><button type="button" className="z-text-button" onClick={download}>{copy.export}</button></div>{notice && <p className="z-row-meta" role="status">{notice}</p>}</form></details>;
}
