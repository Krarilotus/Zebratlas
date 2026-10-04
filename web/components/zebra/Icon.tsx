import type { CSSProperties } from "react";

const paths = {
  search: "m21 21-4.7-4.7M19 10.5a8.5 8.5 0 1 1-17 0 8.5 8.5 0 0 1 17 0Z",
  arrow: "M5 12h14m-6-6 6 6-6 6", close: "m6 6 12 12M6 18 18 6",
  attach: "m8 12 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l9-9", menu: "M4 6h16M4 12h16M4 18h16",
  graph: "M7 6h10M6 8v8m2 2h8m2-2V8M8 8l8 8M8 16 16 8M7 5a2 2 0 1 1-4 0 2 2 0 0 1 4 0Zm14 0a2 2 0 1 1-4 0 2 2 0 0 1 4 0ZM7 19a2 2 0 1 1-4 0 2 2 0 0 1 4 0Zm14 0a2 2 0 1 1-4 0 2 2 0 0 1 4 0Z",
  chevron: "m9 5 7 7-7 7", bookmark: "M6 3h12v18l-6-4-6 4V3Z", external: "M14 3h7v7m0-7L10 14M10 3H3v18h18v-7",
  check: "m5 12 4 4L19 6", share: "M12 16V3m-5 5 5-5 5 5M5 13v8h14v-8", back: "M19 12H5m6-6-6 6 6 6",
} as const;
export function Icon({ name, size = 20, style }: { name: keyof typeof paths; size?: number; style?: CSSProperties }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}><path d={paths[name]} /></svg>;
}
