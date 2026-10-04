export type TBoxView = { x: number; y: number; scale: number };
export type TBoxBounds = { left: number; top: number; width: number; height: number };
export function fitTBoxBounds(bounds: TBoxBounds, viewport: { width: number; height: number }): TBoxView {
  const scale = Math.min(2, Math.max(.08, Math.min((viewport.width - 40) / Math.max(1, bounds.width), (viewport.height - 40) / Math.max(1, bounds.height))));
  return { scale, x: (viewport.width - bounds.width * scale) / 2 - bounds.left * scale, y: (viewport.height - bounds.height * scale) / 2 - bounds.top * scale };
}
export function zoomTBoxView(view: TBoxView, factor: number, anchor: { x: number; y: number }): TBoxView {
  const scale = Math.max(.08, Math.min(3, view.scale * factor)), ratio = scale / view.scale;
  return { scale, x: anchor.x - (anchor.x - view.x) * ratio, y: anchor.y - (anchor.y - view.y) * ratio };
}
