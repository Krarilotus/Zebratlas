/** Counts refer to this bounded response and rendered scene, never the dataset. */
export function graphPreviewCaption(template: string, visible: number): string {
  return template.replaceAll("{count}", String(Math.max(0, Math.floor(visible))));
}

export function graphPreviewScope(template: string, visible: number, loaded: number): string {
  return template.replaceAll("{visible}", String(Math.max(0, Math.floor(visible))))
    .replaceAll("{loaded}", String(Math.max(0, Math.floor(loaded))));
}
