"use client";

import { useEffect, useRef, useState, type FormEvent } from "react";
import { approveConnector, checkModelConnection, chooseModel, connectors, models, resetModel, revokeConnector } from "@/lib/zebra/client";
import type { ConnectorDevice, ModelConnectionCheck, ModelsResponse } from "@/lib/zebra/types";
import { Icon } from "./Icon";
import { useZebraCopy, useZebraLocale } from "./Locale";
import { ZebraLoader } from "./ZebraLoader";
import { getModelCopy } from "@/lib/zebra/model-copy";
import { canUseModel, connectedModels, connectionLabel, currentModel } from "@/lib/zebra/model-settings";
import styles from "./ModelSettings.module.css";

export function ModelSettings() {
  const text = getModelCopy(useZebraLocale());
  const copy = useZebraCopy();
  const [listing, setListing] = useState<ModelsResponse | null>(null);
  const [devices, setDevices] = useState<ConnectorDevice[] | null>(null);
  const [connection, setConnection] = useState("");
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [checkingConnection, setCheckingConnection] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [checked, setChecked] = useState<ModelConnectionCheck | null>(null);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void Promise.allSettled([models(controller.signal), connectors(controller.signal)]).then(([m, d]) => {
      if (controller.signal.aborted) return;
      if (m.status === "fulfilled") { setListing(m.value); setConnection(m.value.selected?.connection ?? ""); setModel(m.value.selected?.model ?? ""); }
      if (d.status === "fulfilled") setDevices(d.value);
      setLoaded(true);
    });
    return () => { controller.abort(); pending.current?.abort(); };
  }, []);
  async function reload(signal: AbortSignal) {
    const [m, d] = await Promise.allSettled([models(signal), connectors(signal)]);
    if (signal.aborted) return;
    if (m.status === "fulfilled") setListing(m.value);
    if (d.status === "fulfilled") setDevices(d.value);
  }
  async function change(action: () => Promise<unknown>, done: string) {
    if (pending.current) return;
    const controller = new AbortController(); pending.current = controller;
    setBusy(true); setCheckingConnection(false); setError(""); setMessage("");
    try { await action(); await reload(controller.signal); if (!controller.signal.aborted) setMessage(done); }
    catch (e) { if (!controller.signal.aborted) setError(e instanceof Error ? e.message : text.error); }
    finally { if (!controller.signal.aborted) setBusy(false); if (pending.current === controller) pending.current = null; }
  }
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (connection && !canUseModel(selected, checked, model)) return;
    void change(() => connection ? chooseModel({ connection, model: model || null }) : resetModel(), text.saved);
  }
  function pair(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const code = String(new FormData(event.currentTarget).get("user_code") || "").trim();
    void change(() => approveConnector(code), text.approved);
  }
  const selected = listing?.connections.find((c) => c.name === connection);
  const instanceDefault = listing?.connections.find((c) => c.name === listing.default);
  const activeDefault = !connection ? instanceDefault : undefined;
  const candidate = selected ?? activeDefault;
  const availableModels = connectedModels(candidate, checked);
  const active = listing ? currentModel(listing) : null;
  const activeVerified = !!active?.model && checked?.connected && checked.connection === active.connection && checked.models.includes(active.model);
  async function connect() {
    if (!candidate || pending.current) return;
    const controller = new AbortController(); pending.current = controller;
    setBusy(true); setCheckingConnection(true); setError(""); setMessage(""); setChecked(null);
    try {
      const value = await checkModelConnection(candidate.name, controller.signal);
      if (controller.signal.aborted) return;
      setChecked(value);
      if (!value.connected) setError(value.availability?.reason === "native-connector-required" ? text.nativeConnector : ["key-needed", "not_configured"].includes(value.availability?.reason || "") ? text.keyNeeded : text.checkFailed);
    } catch { if (!controller.signal.aborted) setError(text.checkFailed); }
    finally { if (!controller.signal.aborted) setBusy(false); if (pending.current === controller) pending.current = null; }
  }
  return <details className={`z-section z-model-settings ${styles.settings}`}>
    <summary>{text.title}</summary>
    {(!loaded || busy) && <p className="z-inline-loading" role="status"><ZebraLoader />{!loaded ? copy.loading : checkingConnection ? text.connecting : copy.authBusy}</p>}
    {error && <p className="z-error" role="alert">{error}</p>}
    {message && <p className="z-note" role="status">{message}</p>}
    {listing ? <form className="z-form" onSubmit={submit}>
      <label>{text.connection}<select value={connection} disabled={busy} onChange={(e) => { setConnection(e.target.value); setModel(""); setChecked(null); setError(""); setMessage(""); }}><option value="">{text.instanceDefault}{instanceDefault?.default_model ? ` · ${instanceDefault.default_model}` : ""}</option>{listing.connections.map((c) => <option value={c.name} key={c.name}>{connectionLabel(c)}</option>)}</select></label>
      {selected && <>
        <label>{text.model}<select value={availableModels.includes(model) ? model : ""} disabled={busy || !availableModels.length} onChange={(e) => setModel(e.target.value)}><option value="" disabled={!selected.default_model || !availableModels.includes(selected.default_model)}>{availableModels.length ? text.defaultModel : text.connectFirst}</option>{availableModels.map((name) => <option value={name} key={name}>{name}</option>)}</select></label>
      </>}
      {candidate && <div className="z-inline-actions"><button className="z-button" disabled={busy} type="button" onClick={() => void connect()}>{checked?.connected && checked.connection === candidate.name ? text.connected : text.connect}<Icon name="check" size={15} /></button>{checked?.connected && checked.connection === candidate.name && !availableModels.length && <span className="z-fine-print">{text.modelMissing}</span>}</div>}
      <p className="z-row-meta" role="status">{activeVerified ? text.currentModel : text.configuredModel}: {active?.label ? `${active.label} · ` : ""}<strong>{active?.model || text.unknownModel}</strong></p>
      <div className="z-inline-actions"><button className="z-button" disabled={busy || (!!connection && !canUseModel(selected, checked, model))} type="submit">{text.save}<Icon name="check" size={15} /></button>{listing.selected && <button className="z-text-button" disabled={busy} type="button" onClick={() => void change(async () => { await resetModel(); setConnection(""); setModel(""); setChecked(null); }, text.saved)}>{text.reset}</button>}</div>
    </form> : loaded && <p className="z-muted">{text.modelUnavailable}</p>}
    <section className="z-section"><h3>{text.devices}</h3>
      {devices ? devices.filter((d) => !d.revoked_at).length ? <ul className="z-simple-list">{devices.filter((d) => !d.revoked_at).map((device) => <li key={device.id}><div className="z-inline-actions"><strong>{device.label}</strong><button className="z-text-button" disabled={busy} onClick={() => void change(() => revokeConnector(device.id), text.saved)}>{text.revoke}</button></div></li>)}</ul> : <p className="z-muted">{text.empty}</p> : loaded && <p className="z-muted">{text.devicesUnavailable}</p>}
    </section>
    {devices && <form className="z-form" onSubmit={pair}><h3>{text.approve}</h3><p className="z-muted">{text.deviceNote}</p><label>{text.code}<input name="user_code" required maxLength={64} autoComplete="off" disabled={busy} /></label><button className="z-button" disabled={busy}>{text.approveButton}<Icon name="arrow" size={15} /></button></form>}
  </details>;
}
