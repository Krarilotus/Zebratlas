export const AVATAR_CHANGED = "zebra-avatar-changed";
const key = (userId: string) => `zebra.avatar.${encodeURIComponent(userId)}`;
export function avatarVariation(raw: string | null): number {
  const number = Number(raw);
  return Number.isSafeInteger(number) && number >= 0 && number <= 1000000 ? number : 0;
}
export function readAvatarVariation(userId?: string): number {
  if (!userId || typeof window === "undefined") return 0;
  try { return avatarVariation(window.localStorage.getItem(key(userId))); }
  catch { return 0; }
}
export function saveAvatarVariation(userId: string, variation: number): boolean {
  if (!userId || typeof window === "undefined" || avatarVariation(String(variation)) !== variation) return false;
  try {
    window.localStorage.setItem(key(userId), String(variation));
    window.dispatchEvent(new Event(AVATAR_CHANGED));
    return true;
  } catch { return false; }
}
export function subscribeAvatar(userId: string | undefined, changed: () => void) {
  if (!userId || typeof window === "undefined") return () => {};
  const storage = (event: StorageEvent) => { if (event.key === key(userId) || event.key === null) changed(); };
  window.addEventListener("storage", storage);
  window.addEventListener(AVATAR_CHANGED, changed);
  return () => { window.removeEventListener("storage", storage); window.removeEventListener(AVATAR_CHANGED, changed); };
}
