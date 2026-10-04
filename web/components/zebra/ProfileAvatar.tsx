"use client";

import { useSyncExternalStore } from "react";
import { avatarSvg } from "@/lib/zebra/profile-avatar";
import { readAvatarVariation, subscribeAvatar } from "@/lib/zebra/avatar-preference";
import styles from "./ProfileAvatar.module.css";

export function useProfileAvatarVariation(userId?: string) {
  return useSyncExternalStore((changed) => subscribeAvatar(userId, changed), () => readAvatarVariation(userId), () => 0);
}
export function ProfileAvatar({ name, userId, variation, size = 32, decorative = false, label, className = "" }: {
  name: string; userId?: string; variation?: number; size?: number; decorative?: boolean; label?: string; className?: string;
}) {
  const saved = useProfileAvatarVariation(userId);
  const diameter = Math.min(96, Math.max(16, size));
  return <span className={`${styles.avatar} ${className}`} style={{ width: diameter, height: diameter }} role={decorative ? undefined : "img"} aria-label={decorative ? undefined : label ?? name} aria-hidden={decorative || undefined} data-profile-avatar dangerouslySetInnerHTML={{ __html: avatarSvg(name, variation ?? saved) }} />;
}
