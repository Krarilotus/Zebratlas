/** Private text travels in bounded POST bodies, never in request URLs. */
export function lookupRequest(term: string, signal?: AbortSignal): [string, RequestInit] {
  return ["lookup", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ q: term }), signal }];
}

/** The original filename stays in browser memory; extraction needs only the bytes. */
export function documentRequest(file: File, signal?: AbortSignal): [string, RequestInit] {
  return ["document", { method: "POST", headers: { "content-type": file.type || "application/octet-stream" }, body: file, signal }];
}

const PUBLIC_ID = /^(?:(?:MONDO|HP):\d{7}|HGNC:\d+|(?:ORPHA|Orphanet|NCBIGene|OMIM):\d+|G2P:G2P\d{5}|NCT\d{8})$/;

/** A deep link is public only after the server returned that exact canonical identifier. */
export function publicSearchUrl(current: string, query: string, returnedIds: readonly string[], focusId?: string): string {
  const url = new URL(current, "https://zebratlas.invalid");
  url.searchParams.delete("q");
  url.searchParams.delete("node");
  const verified = (id: string) => PUBLIC_ID.test(id) && returnedIds.includes(id);
  if (verified(query)) url.searchParams.set("q", query);
  if (focusId && verified(focusId)) url.searchParams.set("node", focusId);
  return `${url.pathname}${url.search}${url.hash}`;
}
