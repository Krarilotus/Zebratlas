import { getZebraCatalog, ZEBRA_LOCALES, type ZebraLocale } from "@/lib/zebra/locale";
import type { BriefTaskId } from "./briefTasks";
import { compactResultLabel } from "./result-label";

export type EmailProvider = "mailto" | "gmail" | "outlook";
export const briefEmailWords = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).emailWords])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["emailWords"]>;
const goals = Object.fromEntries(ZEBRA_LOCALES.map(locale => [locale, getZebraCatalog(locale).emailOpenings])) as Record<ZebraLocale, ReturnType<typeof getZebraCatalog>["emailOpenings"]>;
const singleLine = (value: string) => value.replace(/\s+/g, " ").trim();
type LetterNames = { researcher?: string; sender?: string | null; locale: ZebraLocale };
/** Names and letter framing are local, even when the user explicitly requests an AI body. */
export function wrapInquiry(body: string, { researcher, sender, locale }: LetterNames): string {
  const words = briefEmailWords[locale];
  const greeting = /^(?:Dear|Hello|Hi|Guten Tag|Sehr geehrte(?:r)?|Bonjour|Cher|Chère|Hola|Estimad[oa]|Olá|Prezad[oa]|Caro|Cara|Gentile|亲爱|拝啓|こんにちは|प्रिय|عزيزي|عزيزتي|Уважаем\S*|Merhaba|Sayın)\b[^\n]{0,180}[,:]\s*\n+/iu;
  const signoff = /\n\s*(?:Best regards|Kind regards|Regards|Sincerely|Thank you|Thanks|Mit freundlichen Grüßen|Viele Grüße|Freundliche Grüße|Cordialement|Bien cordialement|Saludos|Atentamente|Atenciosamente|Cordiali saluti|Distinti saluti|С уважением)\s*,?\s*(?:\n[^\n]*){0,3}$/iu;
  const content = body.replace(/\r\n?/g, "\n").trim().replace(greeting, "").replace(signoff, "").trim();
  const greetingLine = words.greeting.replace("{recipient}", () => researcher ? singleLine(researcher) : words.unnamed);
  return `${greetingLine}\n\n${content}\n\n${words.regards}\n${sender?.trim() ? singleLine(sender) : words.sender}`;
}
/** A short inquiry about the real question; scientific findings remain in the evidence appendix. */
export function scriptedInquiry({ query, topic, record, task, ask, researcher, sender, locale }: { query: string; topic?: string; record?: string; task: BriefTaskId; ask: string; researcher?: string; sender?: string | null; locale: ZebraLocale }): string {
  const question = singleLine(query);
  const context = compactResultLabel(singleLine(topic || record || question), 100);
  const opening = goals[locale][task].replace("{context}", () => context);
  return wrapInquiry(`${opening} ${singleLine(ask)}`, { researcher, sender, locale });
}
export function validEmailRecipient(value: string): boolean { return !value || /^[^\s<>,;:?&]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$/.test(value); }
/** Only returns a compose URL. No send endpoint, remote request or side effect. */
export function emailComposeHref(provider: EmailProvider, to: string, subject: string, body: string): { href?: string; error?: "recipient" | "length" } {
  const address = to.trim();
  if (!validEmailRecipient(address)) return { error: "recipient" };
  if (body.length > 1600) return { error: "length" };
  const title = singleLine(subject);
  let href: string;
  if (provider === "mailto") href = `mailto:${encodeURIComponent(address).replace(/%40/gi, "@")}?${new URLSearchParams({ subject: title, body }).toString().replace(/\+/g, "%20")}`;
  else {
    const url = new URL(provider === "gmail" ? "https://mail.google.com/mail/" : "https://outlook.office.com/mail/deeplink/compose");
    if (provider === "gmail") { url.searchParams.set("view", "cm"); url.searchParams.set("fs", "1"); url.searchParams.set("su", title); }
    else url.searchParams.set("subject", title);
    if (address) url.searchParams.set("to", address);
    url.searchParams.set("body", body); href = url.href;
  }
  return href.length <= 2000 ? { href } : { error: "length" };
}
