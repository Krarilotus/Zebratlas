"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";
import dynamic from "next/dynamic";
import { useEffect, useId, useRef } from "react";
import { installMenuDismissal } from "@/lib/zebra/menu-dismissal";
import { useZebraLocale } from "./Locale";
import { networkCopy } from "./network-copy";
import styles from "./NetworkDirectory.module.css";

const NetworkDirectoryPanel = dynamic(() => import("./NetworkDirectoryPanel"));
export function NetworkDirectory({ open, onToggle, onClose }: { open: boolean; onToggle(): void; onClose(): void }) {
  const text = networkCopy[zebraCopyLocale(useZebraLocale())];
  const root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null), id = useId();
  useEffect(() => {
    if (!open || !root.current || !trigger.current) return;
    return installMenuDismissal(root.current, trigger.current, onClose, document);
  }, [open, onClose]);
  function closeExplicitly() {
    const restore = root.current?.contains(document.activeElement);
    onClose(); if (restore) trigger.current?.focus({ preventScroll: true });
  }
  return <div ref={root} className={styles.root}>
    <button ref={trigger} type="button" className={styles.trigger} aria-label={text.title} title={text.title} aria-expanded={open} aria-controls={id} onClick={onToggle}>
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 10.5v6M12 7.5v.1" /></svg>
    </button>
    {open && <NetworkDirectoryPanel id={id} onClose={closeExplicitly} />}
  </div>;
}
