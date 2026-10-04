import { getZebraCatalog, resolveZebraLocale } from "./locale";
import type { MessageRequest, MessageResponse, VerifyResult, IntegrityResponse } from "@/lib/types";
import type { Contribution } from "@/components/contribute/types";
import type { Conversation, SavedKind } from "@/components/account/types";
import type { AuthResult, EmailActionResult } from "./types";
import { lookupRequest, documentRequest } from "./search-privacy";
import type { SearchLookup } from "./search-suggestions";
import type { SparqlRunSettings } from "./types";
import { accountChanged, getAccountRevision, isAccountIdentity, rememberAccountIdentity } from "./account-events";
import type { AccountState, ConditionDetail, ConditionSection, ContributionInput, ConnectorDevice, ConversationSummary, ExplorePlan, ExploreResponse, ExploreSchema, ExtractedDocument, InitiativesResponse, JobsResponse, ModelChoice, ModelsResponse, ProvChain, SavedItem, SavedPayload } from "./types";

export class ZebraApiError extends Error {
  constructor(message: string, public readonly status: number, public readonly code?: string) { super(message); this.name = "ZebraApiError"; }
}
async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const locale = resolveZebraLocale(typeof document === "undefined" ? "en" : document.documentElement.lang);
  const words = getZebraCatalog(locale).apiErrors;
  const url = new URL(`/zebra/api/${path}`, "https://fixture.invalid");
  url.searchParams.set("lang", locale);
  const accountRevision = getAccountRevision();
  const response = await fetch(`${url.pathname}${url.search}`, { ...init, credentials: "same-origin", cache: "no-store" }).catch((error: unknown) => { if (init.signal?.aborted || (error instanceof Error && error.name === "AbortError")) throw error; throw new ZebraApiError(words.unavailable, 0); });
  if (!response.ok) {
    if (response.status === 401 && !init.signal?.aborted && accountRevision === getAccountRevision() && ["account", "models", "connectors", "saved", "conversations", "password", "logout", "logout-all"].includes(path.split("?")[0])) accountChanged({ state: "signed_out" });
    const error = await response.json().catch(() => null) as { detail?: string; code?: string } | null;
    throw new ZebraApiError(error?.code === "forbidden" ? words.forbidden : error?.detail || words.retry, response.status, error?.code);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}
const json = (body: unknown, signal?: AbortSignal): RequestInit => ({ method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal });
const enc = encodeURIComponent;
async function accountMutation<T>(path: string, init: RequestInit): Promise<T> {
  const revision = getAccountRevision();
  const value = await request<T>(path, init);
  const state = value && typeof value === "object" && "state" in value ? value.state : undefined;
  if (state === "signed_in" || state === "signed_out") {
    if (path === "auth" || revision === getAccountRevision()) accountChanged(value as AccountState);
  } else if (revision === getAccountRevision()) {
    if (["logout", "logout-all", "reset-password"].includes(path)) accountChanged({ state: "signed_out" });
    else accountChanged();
  }
  return value;
}
export const lookupEntities = (term: string, signal: AbortSignal) => request<SearchLookup>(...lookupRequest(term, signal));
export const runSparqlQuery = async (sparql: string, query: string, signal: AbortSignal | undefined, settings: SparqlRunSettings) => {
  if (!settings || typeof settings.reasoning !== "boolean" || !Number.isInteger(settings.limit) || settings.limit < 1 || settings.limit > 160) throw new ZebraApiError("The execution settings are unavailable. Select a completed query.", 400, "invalid_rerun_settings");
  const result = await request<ExploreResponse>("sparql", json({ sparql, query, focus: settings.focus, semantic_focus: settings.semantic_focus, limit: settings.limit, reasoning: settings.reasoning }, signal));
  return { ...result, execution: { ...result.execution, rerun_context: settings } };
};
export const explore = (query: string, options: { caption?: string; mode?: "knowledge" | "community"; limit?: number; signal?: AbortSignal; plan?: ExplorePlan } = {}) => request<ExploreResponse>("search", json({ query, caption: options.caption, mode: options.mode, limit: options.limit, plan: options.plan }, options.signal));
export const exploreSchema = (signal?: AbortSignal) => request<ExploreSchema>("schema", { signal });
export const community = (options: { limit?: number; signal?: AbortSignal } = {}) => request<ExploreResponse>(`community?limit=${options.limit ?? 60}`, { signal: options.signal });
export const condition = (id: string, lang = "en", signal?: AbortSignal, sections: ConditionSection[] = ["summary", "connections", "gaps"]) => request<ConditionDetail>(`condition?id=${enc(id)}&lang=${enc(lang)}&sections=${sections.join(",")}`, { signal });
export const provenance = (id: string, signal?: AbortSignal) => request<ProvChain>(`provenance?id=${enc(id)}`, { signal });
export const verify = (id: string, signal?: AbortSignal) => request<VerifyResult>(`verify?id=${enc(id)}`, { signal });
export const integrity = (signal?: AbortSignal) => request<IntegrityResponse>("integrity", { signal });
export const draftMessage = (input: MessageRequest, signal?: AbortSignal) => request<MessageResponse>("message", json(input, signal));
export const submitContribution = async (input: ContributionInput, signal?: AbortSignal) => {
  const value = await request<Contribution>("contribute", json(input, signal));
  if (!value || typeof value.id !== "string" || !value.id.trim() || !["submitted", "auto_checked"].includes(value.state)) throw new Error("invalid_contribution_response");
  return value;
};
export const contribution = (id: string, signal?: AbortSignal) => request<Contribution>(`contribute?id=${enc(id)}`, { signal });
export const extractDocument = async (file: File, signal?: AbortSignal) => ({ ...await request<ExtractedDocument>(...documentRequest(file, signal)), name: file.name });
export const account = async (signal?: AbortSignal) => {
  const revision = getAccountRevision();
  const value = await request<AccountState>("account", { signal });
  if (!isAccountIdentity(value)) throw new Error("invalid_account_state_response");
  if (!signal?.aborted) rememberAccountIdentity(value, revision);
  return value;
};
export const auth = (mode: "login" | "signup", input: { email: string; password: string; display_name?: string; locale?: string }) => accountMutation<AuthResult>("auth", json({ mode, ...input }));
export const verifyEmail = (token: string, locale?: string) => request<EmailActionResult>("verify-email", json({ token, locale }));
export const resendVerification = (email: string, locale: string) => request<EmailActionResult>("resend-verification", json({ email, locale }));
export const forgotPassword = (email: string, locale: string) => request<EmailActionResult>("forgot-password", json({ email, locale }));
export const resetPassword = (token: string, new_password: string, locale?: string) => accountMutation<EmailActionResult>("reset-password", json({ token, new_password, locale }));
export const logout = () => accountMutation<void>("logout", json({}));
export const updateProfile = (input: { display_name?: string | null; locale?: string | null }, signal?: AbortSignal) => accountMutation<AccountState>("account", { ...json(input, signal), method: "PATCH" });
export const changePassword = (input: { current_password: string; new_password: string }, signal?: AbortSignal) => request<{ ended_other_sessions: number }>("password", json(input, signal));
export const logoutAll = () => accountMutation<{ ended_sessions: number }>("logout-all", json({}));
export const savedItems = (signal?: AbortSignal) => request<SavedItem[]>("saved", { signal });
export const saveItem = (input: { kind: SavedKind; title: string; refs: string[]; payload?: SavedPayload; note?: string }) => request<SavedItem>("saved", json(input));
export const updateSaved = (id: string, input: { title?: string; payload?: SavedPayload; note?: string | null }, signal?: AbortSignal) => request<SavedItem>(`saved?id=${enc(id)}`, { ...json(input, signal), method: "PATCH" });
export const removeSaved = (id: string) => request<void>(`saved?id=${enc(id)}`, { method: "DELETE" });
export const conversations = (signal?: AbortSignal) => request<ConversationSummary[]>("conversations", { signal });
export const conversation = (id: string, signal?: AbortSignal) => request<Conversation>(`conversations?id=${enc(id)}`, { signal });
export const rdfExportHref = (id: string) => `/zebra/api/export?id=${enc(id)}`;
export const accountExportHref = "/zebra/api/export?format=account";
export const jobs = (id: string, kind: "condition" | "gene" = "condition", signal?: AbortSignal) => request<JobsResponse>(`jobs?id=${enc(id)}&kind=${kind}`, { signal });
export const initiatives = (signal?: AbortSignal) => request<InitiativesResponse>("initiatives", { signal });
export const models = (signal?: AbortSignal) => request<ModelsResponse>("models", { signal });
export const checkModelConnection = async (connection: string, signal?: AbortSignal) => {
  const checked = await request<import("./types").ModelConnectionCheck>(`models?connection=${enc(connection)}`, { signal });
  if (!checked || checked.connection !== connection || typeof checked.connected !== "boolean" || !Array.isArray(checked.models) || checked.models.some((model) => typeof model !== "string")) throw new Error("invalid_model_connection_response");
  return checked;
};
export const chooseModel = (choice: ModelChoice) => request<{ selected: ModelChoice }>("models", { ...json(choice), method: "PUT" });
export const resetModel = () => request<{ selected: null }>("models", { method: "DELETE" });
export const connectors = (signal?: AbortSignal) => request<ConnectorDevice[]>("connectors", { signal });
export const approveConnector = (userCode: string) => request<{ approved: boolean }>("connectors", json({ user_code: userCode }));
export const revokeConnector = (id: string) => request<{ revoked: boolean }>(`connectors?id=${enc(id)}`, { method: "DELETE" });

export const submitPrivacyRequest = (input: { email: string; concerns: string; type: "remove" | "correct" | "object" }) => request<{ reference: string; received_at: string; respond_by: string }>("privacy", json({ email: input.email, concerns: input.concerns, type: input.type }));
