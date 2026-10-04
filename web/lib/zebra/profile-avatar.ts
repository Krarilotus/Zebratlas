// Carried over from wt-ux/web/lib/avatar.ts: three sine-wave stripes in a circle, derived only from a
// hash of the username, so the same name always draws the same picture. Pure and dependency-free:
// the server renders the string; scripts/check-zebra-profile.mjs checks determinism and size.

/** Restrained colour pairs for the current airy interface. */
const PAIRS: [string, string][] = [
  ["var(--z-teal, #17675f)", "#e6f0ed"],
  ["#326984", "#e8f0f5"],
  ["#807052", "#f4efe4"],
  ["#a36558", "#f5eae5"],
  ["#626b83", "#ebedf4"],
  ["#4a7270", "#e7eeee"],
];

/** FNV-1a, 32 bit. */
export function hash(text: string): number {
  let h = 0x811c9dc5;
  for (const ch of text.normalize("NFC")) {
    h ^= ch.codePointAt(0)!;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/** One visible character, including combining marks and joined emoji. */
export function profileInitial(name: string, locale = "en"): string {
  const clean = name.trim().normalize("NFC");
  const first = new Intl.Segmenter(locale, { granularity: "grapheme" }).segment(clean)[Symbol.iterator]().next().value;
  return first?.segment ?? "";
}

/** xorshift32 seeded with the hash: a stream of numbers in [0, 1). */
function stream(seed: number): () => number {
  let x = seed || 0x9e3779b9;
  return () => {
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    return x / 0x100000000;
  };
}

const r1 = (n: number) => Math.round(n * 10) / 10;

/** One stripe: a sine wave across the 32 × 32 box, drawn as a thick line. */
function wave(next: () => number, base: number): string {
  const cycles = 0.5 + next() * 1.5;
  const amp = 1 + next() * 2.2;
  const phase = next() * Math.PI * 2;
  const pts: string[] = [];
  for (let x = -6; x <= 38; x += 2.75) pts.push(`${r1(x)} ${r1(base + amp * Math.sin((x / 32) * cycles * Math.PI * 2 + phase))}`);
  return `M${pts.join("L")}`;
}

/** The avatar as an SVG string (viewBox 32 × 32, scales cleanly from 24 to 96 px). */
export function avatarSvg(name: string, variation = 0): string {
  const suffix = Number.isSafeInteger(variation) && variation > 0 && variation <= 1000000 ? `\u0000${variation}` : "";
  const h = hash(name.trim().toLowerCase() + suffix);
  const next = stream(h);
  const [strong, soft] = PAIRS[h % PAIRS.length];
  const id = `av${h.toString(36)}`;
  const angle = Math.round(-60 + next() * 120);
  const width = r1(3 + next() * 2);
  const d = [5 + next() * 3, 14 + next() * 4, 24 + next() * 3].map((y) => wave(next, y)).join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" aria-hidden="true"><clipPath id="${id}"><circle cx="16" cy="16" r="16"/></clipPath><g clip-path="url(#${id})"><rect width="32" height="32" fill="${soft}"/><path transform="rotate(${angle} 16 16)" fill="none" stroke="${strong}" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round" d="${d}"/></g></svg>`;
}

export function profileDisplayName(user: { display_name?: string | null; email: string }): string {
  return user.display_name?.trim() || user.email.split("@")[0];
}
