/** Native navigation links remain tabbable. Restore focus only after keyboard dismissal. */
export function installMenuDismissal(root: HTMLElement, trigger: HTMLButtonElement, close: () => void, document: Document) {
  const outside = (event: Event) => { if (!root.contains(event.target as Node | null)) close(); };
  const escape = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    const restore = root.contains(document.activeElement);
    if (restore) { event.preventDefault(); event.stopPropagation(); }
    close(); if (restore) trigger.focus({ preventScroll: true });
  };
  document.addEventListener("pointerdown", outside);
  document.addEventListener("focusin", outside);
  document.addEventListener("keydown", escape);
  return () => {
    document.removeEventListener("pointerdown", outside);
    document.removeEventListener("focusin", outside);
    document.removeEventListener("keydown", escape);
  };
}
