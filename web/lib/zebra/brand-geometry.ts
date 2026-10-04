/** Pixel grids shared by the wordmark, native favicon assets and raster checks. */
export type MarkGrid = { name: string; width: number; height: number; stroke: number; middle: 1 | 2 | 3; middleWidth: number; stepY: number };
export type MarkRectangle = { x: number; y: number; width: number; height: number };
export const NATIVE_MARKS: readonly MarkGrid[] = [
  { name: "tiny", width: 8, height: 7, stroke: 1, middle: 1, middleWidth: 4, stepY: 0 },
  { name: "body", width: 10, height: 10, stroke: 1, middle: 2, middleWidth: 4, stepY: 3 },
  { name: "caption", width: 13, height: 12, stroke: 2, middle: 2, middleWidth: 6, stepY: 4 },
  { name: "header", width: 16, height: 14, stroke: 2, middle: 3, middleWidth: 6, stepY: 3 },
  { name: "display", width: 24, height: 23, stroke: 3, middle: 3, middleWidth: 8, stepY: 5 },
  { name: "displaySmall", width: 20, height: 20, stroke: 2, middle: 3, middleWidth: 8, stepY: 5 },
];
export const FAVICON_MARKS: readonly MarkGrid[] = [
  { name: "16", width: 16, height: 16, stroke: 2, middle: 3, middleWidth: 6, stepY: 3 },
  { name: "32", width: 32, height: 32, stroke: 4, middle: 3, middleWidth: 12, stepY: 7 },
  { name: "48", width: 48, height: 48, stroke: 6, middle: 3, middleWidth: 18, stepY: 11 },
];

export function markRectangles(grid: MarkGrid): MarkRectangle[] {
  const rows: MarkRectangle[] = [{ x: 0, y: 0, width: grid.width, height: grid.stroke }];
  const extent = grid.height - grid.stroke;
  const travel = grid.width - grid.middleWidth;
  for (let index = 0; index < grid.middle; index++) {
    const offset = index - (grid.middle - 1) / 2;
    const y = extent / 2 + offset * grid.stepY;
    // Snap the upper half once, then mirror it: independent rounding at a half-pixel
    // would break 180-degree symmetry. The central bar stays exactly centred.
    const x = offset === 0 ? travel / 2 : offset < 0
      ? Math.round(travel * (extent - y) / extent)
      : travel - Math.round(travel * y / extent);
    rows.push({ x, y, width: grid.middleWidth, height: grid.stroke });
  }
  rows.push({ x: 0, y: grid.height - grid.stroke, width: grid.width, height: grid.stroke });
  for (const row of rows) {
    if (!Object.values(row).every(Number.isInteger) || row.x < 0 || row.y < 0 || row.width < 1 || row.height < 1 || row.x + row.width > grid.width || row.y + row.height > grid.height) throw new Error("Invalid native mark grid");
  }
  for (let index = 1; index < rows.length; index++) if (rows[index - 1].y + rows[index - 1].height >= rows[index].y) throw new Error("Pixel strokes must be disconnected");
  return rows;
}
export function markPath(grid: MarkGrid): string {
  return markRectangles(grid).map(({ x, y, width, height }) => `M${x} ${y}H${x + width}V${y + height}H${x}Z`).join("");
}
