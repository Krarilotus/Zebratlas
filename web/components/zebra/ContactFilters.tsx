"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";
import { useId } from "react";
import { useZebraLocale } from "./Locale";
import { CONTACT_ROLES, type ContactRole } from "./contact-filters";
import { contactCopy } from "./contact-copy";
import styles from "./ContactFilters.module.css";

export default function ContactFilters({ value, disabled, onChange }: { value: ContactRole; disabled: boolean; onChange(role: ContactRole): void }) {
  const id = useId(), text = contactCopy[zebraCopyLocale(useZebraLocale())];
  return <div className={styles.filters}><label htmlFor={id}>{text.looking}</label><select id={id} value={value} disabled={disabled} onChange={event => onChange(event.target.value as ContactRole)}>{CONTACT_ROLES.map(role => <option key={role} value={role}>{text[role]}</option>)}</select></div>;
}
