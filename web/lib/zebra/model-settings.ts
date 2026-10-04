import type { ModelConnection, ModelConnectionCheck, ModelsResponse } from "./types";

export function connectionLabel(connection: ModelConnection): string {
  return connection.default_model ? `${connection.label} · ${connection.default_model}` : connection.label;
}
/** Resolve the saved choice, rather than an unsaved selection in the form. */
export function currentModel(listing: ModelsResponse) {
  const name = listing.selected?.connection || listing.default;
  const connection = listing.connections.find((candidate) => candidate.name === name);
  return { connection: name, label: connection?.label || name, model: listing.selected?.model || connection?.default_model };
}
/** Configured defaults and key presence cannot unlock a model catalogue. */
export function connectedModels(connection: ModelConnection | undefined, check: ModelConnectionCheck | null): string[] {
  if (!connection || !check?.connected || check.connection !== connection.name) return [];
  const allowed = new Set([...connection.models, ...(connection.default_model ? [connection.default_model] : [])]);
  return [...new Set(check.models)].filter((model) => allowed.has(model));
}
export function canUseModel(connection: ModelConnection | undefined, check: ModelConnectionCheck | null, model: string): boolean {
  const resolved = model || connection?.default_model;
  return !!resolved && connectedModels(connection, check).includes(resolved);
}
