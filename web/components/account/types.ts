// Shapes of the accounts API (crates/atlas-accounts). Shared by server and client components.

export type Plan = "free" | "organisation" | "enterprise";
export type Role = "owner" | "admin" | "member";

export const SAVED_KINDS = ["condition", "connection_card", "connection_map", "message_draft", "node", "search"] as const;
export type SavedKind = (typeof SAVED_KINDS)[number];

export type User = {
  id: string;
  email: string;
  display_name: string | null;
  locale: string | null;
  created_at: string;
};

export type Membership = {
  id: string;
  name: string;
  plan: Plan;
  role: Role;
  created_at: string;
};

export type Account = {
  user: User;
  plan: Plan;
  organisations: Membership[];
  session?: { expires_at: string };
};

/** What a Save button stores. `href` is an in-app path without the locale, used to reopen the item. */
export type SavedPayload = {
  href?: string;
  subject?: string;
  body?: string;
  [key: string]: unknown;
};

export type SavedItem = {
  id: string;
  kind: SavedKind;
  title: string;
  note: string | null;
  payload: SavedPayload;
  refs: string[];
  atlas_snapshot: string | null;
  org_id: string | null;
  created_at: string;
  updated_at: string;
};

export type Citation = { id?: string; label?: string; url?: string; source?: string; [key: string]: unknown };

export type ConversationMessage = {
  id: string;
  seq: number;
  role: "user" | "assistant";
  content: string;
  citations: Citation[];
  meta: Record<string, unknown> | null;
  created_at: string;
};

export type ConversationSummary = {
  id: string;
  title: string;
  locale: string | null;
  org_id: string | null;
  message_count: number;
  created_at: string;
  updated_at: string;
};

export type Conversation = ConversationSummary & { messages: ConversationMessage[] };

/** A message to store; citations point at atlas ids and sources. */
export type NewMessage = { role: "user" | "assistant"; content: string; citations?: Citation[]; meta?: Record<string, unknown> };

/** Stable error codes from the API plus the web's own; each has a string under account.error.*. */
export type ErrorCode =
  | "bad_credentials"
  | "conflict"
  | "invalid"
  | "invalid_email"
  | "password_short"
  | "rate_limited"
  | "unavailable"
  | "unauthorized"
  | "not_found"
  | "forbidden"
  | "generic";

export type ActionResult<T = undefined> = { ok: true; value?: T } | { ok: false; error: ErrorCode };
