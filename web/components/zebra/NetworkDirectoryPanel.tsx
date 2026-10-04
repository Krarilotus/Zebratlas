"use client";
import { zebraCopyLocale } from "@/lib/zebra/locale";
import { networkDirectory } from "@/lib/zebra/network-directory";
import { useZebraLocale } from "./Locale";
import { networkCopy } from "./network-copy";
import { Icon } from "./Icon";
import styles from "./NetworkDirectory.module.css";

export default function NetworkDirectoryPanel({ id, onClose }: { id: string; onClose(): void }) {
  const locale = useZebraLocale(), text = networkCopy[zebraCopyLocale(locale)];
  return <section id={id} className={styles.panel} aria-labelledby={`${id}-title`}>
    <header className={styles.heading}><h2 id={`${id}-title`}>{text.title}</h2><button type="button" onClick={onClose} aria-label={text.close}><Icon name="close" size={18} /></button></header>
    <div className={styles.content}>{(["support", "research", "data"] as const).map(group => <section className={styles.group} key={group} aria-labelledby={`${id}-${group}`}>
      <h3 id={`${id}-${group}`}>{text[group]}</h3><ul>{networkDirectory.filter(entry => entry.group === group).map(entry => <li key={entry.id}>
        <div className={styles.identity}><h4>{entry.name}</h4><p className={styles.scope}><span>{text[entry.region]}</span>{entry.scope[zebraCopyLocale(locale)]}</p><p className={styles.purpose}>{entry.purpose[zebraCopyLocale(locale)]}</p></div>
        <div className={styles.links}>{entry.links.map(link => <a key={link.url} href={link.url} target={link.url.startsWith("mailto:") ? undefined : "_blank"} rel="noopener noreferrer">{link.label[zebraCopyLocale(locale)]}<Icon name="external" size={12} /></a>)}</div>
        <details className={styles.sources}><summary>{text.sources}</summary><p>{entry.limitation[zebraCopyLocale(locale)]}</p><ul>{entry.sources.map(source => <li key={source.url}><a href={source.url} target="_blank" rel="noopener noreferrer">{source.url}</a><span>{text.checked}: <time dateTime={source.checkedAt}>{source.checkedAt}</time></span></li>)}</ul></details>
      </li>)}</ul>
    </section>)}<p className={styles.privacy}>{text.privacy}</p></div>
  </section>;
}
