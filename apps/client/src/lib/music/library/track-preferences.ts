import type { MusicMembershipMatrixEntry, MusicSnooze, MusicWeight } from "./contracts";

export const MUSIC_WEIGHT_ORDER: readonly MusicWeight[] = [
  "rarely",
  "less-often",
  "normal",
  "more-often",
  "much-more-often",
];

/** Returns the memberships affected by the shared scope selector. */
export function musicMembershipsForScope(
  memberships: readonly MusicMembershipMatrixEntry[],
  playlistId: string | null,
): MusicMembershipMatrixEntry[] {
  return playlistId === null ? [...memberships] : memberships.filter((entry) => entry.playlistId === playlistId);
}

/** Returns a shared weight, or null when the scope has no or mixed weights. */
export function musicWeightForScope(memberships: readonly MusicMembershipMatrixEntry[]): MusicWeight | null {
  const first = memberships[0]?.weight;
  return first && memberships.every((entry) => entry.weight === first) ? first : null;
}

/** Returns active snoozes that belong to the selected scope. */
export function musicSnoozesForScope(
  snoozes: readonly MusicSnooze[],
  playlistId: string | null,
  now: number,
): MusicSnooze[] {
  return snoozes.filter((entry) => entry.startsAtMs <= now
    && (entry.endsAtMs === null || entry.endsAtMs > now)
    && (playlistId === null || (entry.scope === "playlist" && entry.playlistId === playlistId)));
}
