"use client";
import Image from "next/image";
import { useEffect, useId, useRef } from "react";
import { ZEBRA_LOCALES, ZEBRA_LOCALE_META } from "@/lib/zebra/locale";
import { installMenuDismissal } from "@/lib/zebra/menu-dismissal";
import { useSetZebraLocale, useZebraCopy, useZebraLocale } from "./Locale";
import styles from "./LanguagePicker.module.css";

/** Flags are illustrative cues; native language names are the actual controls. */

export function LanguagePicker({ open, onToggle, onClose }: { open: boolean; onToggle(): void; onClose(): void }) {
  const locale = useZebraLocale(), setLocale = useSetZebraLocale(), copy = useZebraCopy();
  const root = useRef<HTMLDivElement>(null), button = useRef<HTMLButtonElement>(null), id = useId();
  useEffect(() => {
    if (!open || !root.current || !button.current) return;
    return installMenuDismissal(root.current, button.current, onClose, document);
  }, [open, onClose]);
  return <div ref={root} className={styles.root}>
    <button ref={button} type="button" className={styles.trigger} aria-label={`${copy.language}: ${ZEBRA_LOCALE_META[locale].name}`} aria-expanded={open} aria-controls={id} onClick={onToggle}>
      <Image src={`/zebra/language-cues/${locale}.svg`} width={24} height={16} alt="" aria-hidden="true" unoptimized />
    </button>
    {open && <div id={id} className={styles.panel} aria-label={copy.language}>
      {ZEBRA_LOCALES.map((language) => <button key={language} type="button" lang={language} dir="ltr" aria-pressed={locale === language} onClick={() => { setLocale(language); onClose(); button.current?.focus({ preventScroll: true }); }}>
        <Image src={`/zebra/language-cues/${language}.svg`} width={24} height={16} alt="" aria-hidden="true" unoptimized /><span dir={ZEBRA_LOCALE_META[language].dir}>{ZEBRA_LOCALE_META[language].name}</span>
      </button>)}
    </div>}
  </div>;
}
