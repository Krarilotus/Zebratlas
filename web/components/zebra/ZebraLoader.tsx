"use client";

import { useEffect, useRef } from "react";
import { NATIVE_MARKS, markRectangles } from "@/lib/zebra/brand-geometry";
import styles from "./ZebraLoader.module.css";

const grid = NATIVE_MARKS.find(mark => mark.name === "display")!;
const native = markRectangles(grid);
const profile = [{ x: 8, width: 3 }, { x: 3, width: 12 }, { x: 2, width: 12 }, { x: 1, width: 12 }, { x: 0, width: 12 }];
// Keep the native five rows; three lengths grow from zero for the ear, eye and muzzle.
export const STRIPES = native.map((z, index) => ({ z, head: { ...z, ...profile[index] } })).concat([
  { z: { x: 16, y: 10, width: 0, height: 3 }, head: { x: 16, y: 10, width: 5, height: 3 } },
  { z: { x: 11, y: 15, width: 0, height: 3 }, head: { x: 16, y: 15, width: 8, height: 3 } },
  { z: { x: 0, y: 0, width: 0, height: 3 }, head: { x: 3, y: 0, width: 2, height: 3 } },
]);

/** One cosine controls both bar lengths and an eased turn around the vertical axis. */
export function zebraFrame(elapsed: number) {
  const phase = (elapsed % 6000) / 6000;
  const morph = (1 - Math.cos(phase * Math.PI * 2)) / 2;
  return { morph, turn: phase < .5 ? 180 * morph : 360 - 180 * morph };
}

/** Decorative; the caller owns the request state and localized announcement. */
export function ZebraLoader() {
  const ref = useRef<SVGSVGElement>(null);
  useEffect(() => {
    const svg = ref.current!;
    const bars = [...svg.querySelectorAll("rect")];
    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    let start = performance.now();
    function draw(morph: number, turn: number) {
      STRIPES.forEach(({ z, head }, index) => {
        bars[index].setAttribute("x", String(z.x + (head.x - z.x) * morph));
        bars[index].setAttribute("width", String(z.width + (head.width - z.width) * morph));
      });
      svg.style.transform = `perspective(72px) rotateY(${turn}deg)`;
    }
    function tick(now: number) {
      const { morph, turn } = zebraFrame(now - start);
      draw(morph, turn);
      frame = requestAnimationFrame(tick);
    }
    function update() {
      cancelAnimationFrame(frame);
      if (reduced.matches) { draw(1, 0); svg.style.transform = "none"; }
      else { start = performance.now(); tick(start); }
    }
    update();
    reduced.addEventListener("change", update);
    return () => { cancelAnimationFrame(frame); reduced.removeEventListener("change", update); };
  }, []);
  return <span className={styles.loader} data-zebra-loading aria-hidden="true">
    <svg ref={ref} className={styles.turn} width="24" height="24" viewBox="0 0 28 27" focusable="false" fill="currentColor">
      <g transform="translate(2 2)">{STRIPES.map(({ head }, index) => <rect key={index} {...head} />)}</g>
    </svg>
  </span>;
}
