import type {
  MusicActivityPhase,
  MusicAssignmentBehavior,
  MusicAssignmentProvenanceKind,
  MusicContextAssignment,
  MusicContextAssignmentDraft,
} from "./context-assignment";

export const MUSIC_ACTIVITY_PHASES: readonly MusicActivityPhase[] = [
  "focus",
  "short-break",
  "long-break",
];

export type MusicPhaseNavigationKey = "ArrowLeft" | "ArrowRight" | "Home" | "End";

export function nextMusicAssignmentPhase(
  phase: MusicActivityPhase,
  key: MusicPhaseNavigationKey,
): MusicActivityPhase {
  const current = MUSIC_ACTIVITY_PHASES.indexOf(phase);
  if (key === "Home") return MUSIC_ACTIVITY_PHASES[0];
  if (key === "End") return MUSIC_ACTIVITY_PHASES[MUSIC_ACTIVITY_PHASES.length - 1];
  const offset = key === "ArrowRight" ? 1 : -1;
  return MUSIC_ACTIVITY_PHASES[(current + offset + MUSIC_ACTIVITY_PHASES.length) % MUSIC_ACTIVITY_PHASES.length];
}

export function emptyMusicAssignmentDraft(
  phase: MusicActivityPhase,
  provenanceKind: MusicAssignmentProvenanceKind = "explicit",
  provenanceId: string | null = null,
): MusicContextAssignmentDraft {
  return {
    phase,
    behavior: "inherit",
    playlistId: null,
    soundscapeId: null,
    soundscapeBehavior: "inherit",
    provenanceKind,
    provenanceId,
  };
}

export function completeMusicAssignmentDrafts(
  assignments: readonly (MusicContextAssignment | MusicContextAssignmentDraft)[],
  provenanceKind: MusicAssignmentProvenanceKind = "explicit",
  provenanceId: string | null = null,
): MusicContextAssignmentDraft[] {
  return MUSIC_ACTIVITY_PHASES.map((phase) => {
    const assignment = assignments.find((entry) => entry.phase === phase);
    return assignment ? toMusicAssignmentDraft(assignment) : emptyMusicAssignmentDraft(phase, provenanceKind, provenanceId);
  });
}

export function toMusicAssignmentDraft(
  assignment: MusicContextAssignment | MusicContextAssignmentDraft,
): MusicContextAssignmentDraft {
  return {
    phase: assignment.phase,
    behavior: assignment.behavior,
    playlistId: assignment.playlistId,
    soundscapeId: assignment.soundscapeId,
    soundscapeBehavior: assignment.soundscapeBehavior,
    provenanceKind: assignment.provenanceKind,
    provenanceId: assignment.provenanceId,
  };
}

export function updateMusicAssignmentDraft(
  assignments: readonly MusicContextAssignmentDraft[],
  phase: MusicActivityPhase,
  update: Partial<Pick<MusicContextAssignmentDraft, "behavior" | "playlistId" | "soundscapeId" | "soundscapeBehavior">>,
): MusicContextAssignmentDraft[] {
  return completeMusicAssignmentDrafts(assignments).map((assignment) => {
    if (assignment.phase !== phase) return assignment;
    const next = { ...assignment, ...update };
    if (!behaviorUsesPlaylist(next.behavior)) next.playlistId = null;
    return next;
  });
}

export function persistedMusicAssignmentDrafts(
  assignments: readonly MusicContextAssignmentDraft[],
): MusicContextAssignmentDraft[] {
  return completeMusicAssignmentDrafts(assignments)
    .filter((assignment) => assignment.behavior !== "inherit"
      || assignment.playlistId !== null
      || assignment.soundscapeId !== null
      || assignment.soundscapeBehavior !== "inherit");
}

export function musicAssignmentDraftsEqual(
  left: readonly MusicContextAssignmentDraft[],
  right: readonly MusicContextAssignmentDraft[],
): boolean {
  const a = completeMusicAssignmentDrafts(left);
  const b = completeMusicAssignmentDrafts(right);
  return a.every((assignment, index) => {
    const other = b[index];
    return assignment.phase === other.phase
      && assignment.behavior === other.behavior
      && assignment.playlistId === other.playlistId
      && assignment.soundscapeId === other.soundscapeId
      && assignment.soundscapeBehavior === other.soundscapeBehavior
      && assignment.provenanceKind === other.provenanceKind
      && assignment.provenanceId === other.provenanceId;
  });
}

export function behaviorUsesPlaylist(behavior: MusicAssignmentBehavior): boolean {
  return behavior === "play-automatically" || behavior === "prepare-silently";
}
