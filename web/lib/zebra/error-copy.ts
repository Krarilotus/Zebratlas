import { formatMessage } from "@/lib/i18n/format";
import { getZebraCatalog, resolveZebraLocale } from "./locale";

/** Local error copy is translated; an upstream's original diagnostic stays available as data. */
export function localizedError(request: Request, message: string, code?: string) {
  const locale = resolveZebraLocale(request.headers.get("x-zebra-locale"));
  const catalog = getZebraCatalog(locale);
  const source = getZebraCatalog("en");
  const accountMessages = {
    invalid_token: "accountLinkInvalid", email_unverified: "verificationRequired",
    mail_unavailable: "accountMailUnavailable", rate_limited: "accountTryLater",
    bad_credentials: "signInFailed",
  } as const;
  if (code && Object.hasOwn(accountMessages, code)) {
    const key = accountMessages[code as keyof typeof accountMessages];
    return { detail: catalog.copy[key], detail_lang: locale, original_detail: message };
  }
  if (message.startsWith("validField:")) {
    const name = message.slice("validField:".length);
    const field = catalog.apiFields[name as keyof typeof catalog.apiFields];
    return {
      detail: field ? formatMessage(catalog.apiErrors.validField, { field }, locale) : catalog.apiErrors.invalidRequest,
      detail_lang: locale,
      original_detail: formatMessage(source.apiErrors.validField, { field: name }, "en"),
    };
  }
  const key = message as keyof typeof catalog.apiErrors;
  if (message === "signInFailed") {
    return { detail: catalog.copy.signInFailed, detail_lang: locale, original_detail: source.copy.signInFailed };
  }
  return {
    detail: Object.hasOwn(catalog.apiErrors, key) ? catalog.apiErrors[key] : catalog.apiErrors.retry,
    detail_lang: locale,
    original_detail: Object.hasOwn(source.apiErrors, key) ? source.apiErrors[key] : message,
  };
}
