/** Convert a section's volume multiplier into an adjustment from the overall volume. */
export function soundscapeVolumeAdjustment(level: number | null): number {
  return Math.round(((level ?? 1) - 1) * 100);
}

/** Convert a percentage adjustment into a multiplier, leaving neutral unset. */
export function soundscapeLevelFromAdjustment(adjustment: number): number | null {
  return adjustment === 0 ? null : 1 + adjustment / 100;
}
