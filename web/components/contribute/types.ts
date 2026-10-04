// Shapes of the atlas-contrib API (crates/atlas-contrib/src/model.rs).
import type { MessageRef } from "@/lib/types";

export type ContributionKind = "new_link" | "correction" | "missing_evidence" | "outdated_contact";
export type SubjectKind = "patient_group" | "organisation" | "registry" | "study" | "person";
export type ContributionState = "submitted" | "auto_checked" | "accepted" | "rejected";
export type CheckStatus = "pass" | "warn" | "fail" | "skipped";

export const KINDS: ContributionKind[] = ["new_link", "correction", "missing_evidence", "outdated_contact"];
export const SUBJECT_KINDS: SubjectKind[] = ["patient_group", "organisation", "registry", "study", "person"];

export type NodeInput = { id?: string; label?: string };

export type Check = {
  name: string;
  status: CheckStatus;
  code: string;
  message: string;
  msg?: MessageRef;
  blocking?: boolean;
  detail?: unknown;
};

export type NodeHit = { id: string; kind: string; label: string; exact?: boolean };

export type CheckReport = {
  checked_at: string;
  agent: string;
  checks: Check[];
  subject?: NodeHit;
  target?: NodeHit;
  relation?: string;
};

export type Contribution = {
  id: string;
  kind: ContributionKind;
  state: ContributionState;
  version: number;
  submission: {
    kind: ContributionKind;
    subject_kind?: SubjectKind;
    subject: NodeInput;
    target?: NodeInput;
    edge?: string;
    statement: string;
    evidence_url?: string;
    quote?: string;
    contact_url?: string;
    found_via?: { page?: string; assistant?: string };
    lang?: string;
  };
  contributor: { name?: string; organisation?: string; contact?: string; signed_in?: boolean; user_id?: string };
  created_at: string;
  updated_at: string;
  checks?: CheckReport;
  review?: { decision: "accept" | "reject"; reason: string; at: string };
};

export type ReviewQueue = {
  items: Contribution[];
  counts: Record<ContributionState, number>;
};

/** A connection the person can say something about (from a card on the condition page). */
export type AboutOption = { value: string; label: string; edge?: string; subject?: string };
