export type HeaderPopover = "account" | "networks" | "language" | null;
/** A single owner prevents overlapping menus, including stale outside listeners. */
export function toggleHeaderPopover(current: HeaderPopover, requested: Exclude<HeaderPopover, null>): HeaderPopover {
  return current === requested ? null : requested;
}
export function dismissHeaderPopover(current: HeaderPopover, requested: Exclude<HeaderPopover, null>): HeaderPopover {
  return current === requested ? null : current;
}
