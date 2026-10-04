import type { BriefTaskId } from "./briefTasks";
import { compactResultLabel } from "./result-label";

export type EmailProvider = "mailto" | "gmail" | "outlook";
export const briefEmailWords = {
  en: { open: "Open email", provider: "Email app", recipient: "To", self: "To myself", default: "Default email app", gmail: "Gmail", outlook: "Outlook", tooLong: "The draft is too long for a compose link. Copy or download it instead.", invalid: "Enter a valid recipient email, or leave it empty.", saveTooLarge: "The brief exceeds the account save limit. Download it or shorten the draft.", facts: "Recorded context", dear: "Dear", unnamed: "[Name]", regards: "Best regards", sender: "[Your name]", proposeQuestion: "Could you help frame a testable research question and identify the first evidence to check?", researchStep: "Could you suggest the most useful next research step for this topic?" },
  de: { open: "E-Mail öffnen", provider: "E-Mail-App", recipient: "An", self: "An mich selbst", default: "Standard-E-Mail-App", gmail: "Gmail", outlook: "Outlook", tooLong: "Der Entwurf ist zu lang für einen Erstellungslink. Kopieren oder laden Sie ihn herunter.", invalid: "Geben Sie eine gültige E-Mail-Adresse ein oder lassen Sie das Feld leer.", saveTooLarge: "Der Brief überschreitet die Kontospeichergrenze. Laden Sie ihn herunter oder kürzen Sie den Entwurf.", facts: "Erfasster Kontext", dear: "Guten Tag", unnamed: "[Name]", regards: "Mit freundlichen Grüßen", sender: "[Ihr Name]", proposeQuestion: "Könnten Sie eine überprüfbare Forschungsfrage und die zuerst zu prüfenden Belege vorschlagen?", researchStep: "Könnten Sie den sinnvollsten nächsten Forschungsschritt zu diesem Thema vorschlagen?" },
};
const goals = {
  en: { F01: "access to a research resource", F02: "a programme feasibility review", F03: "support", F04: "study or registry information", F05: "a protocol reuse review", F06: "a mechanism or assay review", F07: "research input", F08: "a current funding route", F09: "lawful full-text access", F10: "a comparison of related conditions", F11: "input on a shared research question", F12: "help resolving a recorded evidence gap" },
  de: { F01: "Zugang zu einer Forschungsressource", F02: "eine Prüfung der Programmmachbarkeit", F03: "Unterstützung", F04: "Studien- oder Registerinformationen", F05: "eine Prüfung der Protokollwiederverwendung", F06: "eine Mechanismus- oder Assayprüfung", F07: "fachlichen Forschungsinput", F08: "einen aktuellen Förderweg", F09: "rechtmäßigen Volltextzugang", F10: "einen Vergleich verwandter Erkrankungen", F11: "Input zu einer gemeinsamen Forschungsfrage", F12: "Hilfe bei einer erfassten Beleglücke" },
};
const singleLine = (value: string) => value.replace(/\s+/g, " ").trim();
type LetterNames = { researcher?: string; sender?: string | null; locale: "en" | "de" };
/** Names and letter framing are local, even when the user explicitly requests an AI body. */
export function wrapInquiry(body: string, { researcher, sender, locale }: LetterNames): string {
  const words = briefEmailWords[locale];
  const greeting = /^(?:Dear|Hello|Hi|Guten Tag|Sehr geehrte(?:r)?|Bonjour|Cher|Chère|Hola|Estimad[oa]|Olá|Prezad[oa]|Caro|Cara|Gentile|亲爱|拝啓|こんにちは|प्रिय|عزيزي|عزيزتي|Уважаем\S*|Merhaba|Sayın)\b[^\n]{0,180}[,:]\s*\n+/iu;
  const signoff = /\n\s*(?:Best regards|Kind regards|Regards|Sincerely|Thank you|Thanks|Mit freundlichen Grüßen|Viele Grüße|Freundliche Grüße|Cordialement|Bien cordialement|Saludos|Atentamente|Atenciosamente|Cordiali saluti|Distinti saluti|С уважением)\s*,?\s*(?:\n[^\n]*){0,3}$/iu;
  const content = body.replace(/\r\n?/g, "\n").trim().replace(greeting, "").replace(signoff, "").trim();
  return `${words.dear} ${researcher ? singleLine(researcher) : words.unnamed},\n\n${content}\n\n${words.regards}\n${sender?.trim() ? singleLine(sender) : words.sender}`;
}
/** A short inquiry about the real question; scientific findings remain in the evidence appendix. */
export function scriptedInquiry({ query, topic, record, task, ask, researcher, sender, locale }: { query: string; topic?: string; record?: string; task: BriefTaskId; ask: string; researcher?: string; sender?: string | null; locale: "en" | "de" }): string {
  const question = singleLine(query);
  const context = compactResultLabel(singleLine(topic || record || question), 100);
  const opening = locale === "en" ? `I am looking for ${goals.en[task]} related to ${context}.` : `Ich suche ${goals.de[task]} im Zusammenhang mit ${context}.`;
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
