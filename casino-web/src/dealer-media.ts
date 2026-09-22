export type DealerMood = "idle" | "deal" | "flip";
export type ClipFlags = Partial<Record<DealerMood, boolean>>;

export function selectDealerClip(requested: DealerMood, current: DealerMood | null, ready: ClipFlags, failed: ClipFlags): DealerMood | null {
  const usable = (clip: DealerMood) => ready[clip] && !failed[clip];
  if (usable(requested)) return requested;
  if (current && usable(current)) return current;
  return usable("idle") ? "idle" : null;
}
