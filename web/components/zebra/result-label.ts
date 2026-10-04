/** Compact source wording without generating a new title; keep the full title in accessible labels. */
export function compactResultLabel(label: string, limit = 84): string {
  const text = label.replace(/\s+/g, " ").trim();
  const characters = Array.from(text);
  if (characters.length <= limit) return text;
  const prefix = characters.slice(0, Math.max(1, limit - 1)).join("");
  const boundary = prefix.lastIndexOf(" ");
  return `${(boundary > prefix.length * .6 ? prefix.slice(0, boundary) : prefix).replace(/[.,;:]$/, "")}\u2026`;
}
