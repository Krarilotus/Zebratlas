import { Fragment } from "react";
import { NATIVE_MARKS, markPath } from "@/lib/zebra/brand-geometry";
import styles from "./Brand.module.css";

const NAME = "Zebratlas";
// One rectangle constructor defines all optical sizes and the favicon family.
export const ZEBRA_GRIDS = NATIVE_MARKS.map(grid => ({ ...grid, compact: markPath(grid), regular: markPath(grid) }));
export const ZEBRA_MARK_PATH = markPath(NATIVE_MARKS.find(grid => grid.name === "header")!);
export const ZEBRA_COMPACT_PATH = markPath(NATIVE_MARKS.find(grid => grid.name === "caption")!);

export function ZebraMark({ className, variant = "ink", optical = "compact" }: { className?: string; variant?: "ink" | "white"; optical?: "compact" | "regular" }) {
  return <span aria-hidden="true" className={`${styles.mark}${variant === "white" ? ` ${styles.white}` : ""}${className ? ` ${className}` : ""}`}>
    {ZEBRA_GRIDS.map(grid => <svg key={grid.name} data-grid={grid.name} focusable="false" width={grid.width} height={grid.height} viewBox={`0 0 ${grid.width} ${grid.height}`} shapeRendering="crispEdges" fill="currentColor"><path d={grid[optical]} /></svg>)}
  </span>;
}
/** The symbol is the initial Z. Keep the complete name as a single accessible text node. */
export function Brand({ name = NAME, decorative = false, className, optical = "compact" }: { name?: string; decorative?: boolean; className?: string; optical?: "compact" | "regular" }) {
  return <span dir="ltr" className={`${styles.brand}${className ? ` ${className}` : ""}`} aria-hidden={decorative || undefined}>
    <ZebraMark optical={optical} /><span aria-hidden="true">{name.slice(1)}</span>{!decorative && <span className={styles.accessible}>{name}</span>}
  </span>;
}

/** Render branding consistently inside translated captions and prose, without changing their words. */
export function BrandText({ text }: { text: string }) {
  const parts = text.split(NAME);
  return <>{parts.map((part, index) => <Fragment key={index}>{index > 0 && <Brand />}{part}</Fragment>)}</>;
}
