import {
  behaviorUsesPlaylist,
  completeMusicAssignmentDrafts,
  emptyMusicAssignmentDraft,
  MUSIC_ACTIVITY_PHASES,
} from "./assignment-draft";
import type { MusicActivityPhase, MusicContextAssignmentDraft } from "./context-assignment";

/**
 * Lists the phases an event can choose playlists for. Without Pomodoro the whole event runs as one
 * Focus phase, so only that phase is offered.
 *
 * @param pomodoroEnabled - Whether the event runs Pomodoro phases.
 * @returns The phases shown as playlist rows.
 */
export function eventMusicPhases(pomodoroEnabled: boolean): readonly MusicActivityPhase[] {
  return pomodoroEnabled ? MUSIC_ACTIVITY_PHASES : ["focus"];
}

/** Whether a row takes part in resolution, matching the native session, which skips fully inherited rows. */
function assignmentApplies(assignment: MusicContextAssignmentDraft | undefined): assignment is MusicContextAssignmentDraft {
  return assignment !== undefined
    && (assignment.behavior !== "inherit" || assignment.soundscapeBehavior !== "inherit");
}

/** Reads the playlist a single assignment row plays, or null when it plays none. */
function assignmentPlaylistId(assignment: MusicContextAssignmentDraft | undefined): string | null {
  return assignmentApplies(assignment) && behaviorUsesPlaylist(assignment.behavior) ? assignment.playlistId : null;
}

/**
 * Resolves the playlist a phase plays for the event: the override when it applies, otherwise the
 * project snapshot.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phase - Phase to resolve.
 * @returns The effective playlist ID, or null for no playlist.
 */
export function effectiveEventPlaylistId(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phase: MusicActivityPhase,
): string | null {
  const override = overrides.find((assignment) => assignment.phase === phase);
  if (assignmentApplies(override)) return assignmentPlaylistId(override);
  return assignmentPlaylistId(snapshots.find((assignment) => assignment.phase === phase));
}

/**
 * Applies a playlist choice for one phase. A choice equal to the project snapshot clears the
 * override so the event keeps following its project; any other choice plays that playlist
 * automatically, or silence for null, without ambient sound.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phase - Phase being changed.
 * @param playlistId - Chosen playlist, or null for none.
 * @returns The complete next override drafts.
 */
export function chooseEventPlaylist(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phase: MusicActivityPhase,
  playlistId: string | null,
): MusicContextAssignmentDraft[] {
  const inherited = assignmentPlaylistId(snapshots.find((assignment) => assignment.phase === phase));
  return completeMusicAssignmentDrafts(overrides).map((assignment): MusicContextAssignmentDraft => {
    if (assignment.phase !== phase) return assignment;
    if (playlistId === inherited) return emptyMusicAssignmentDraft(phase);
    return {
      ...emptyMusicAssignmentDraft(phase),
      behavior: playlistId ? "play-automatically" : "pause-music",
      playlistId,
    };
  });
}

/**
 * Silences every listed phase that currently plays a playlist.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phases - Phases to silence.
 * @returns The complete next override drafts.
 */
export function silenceEventPlaylists(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): MusicContextAssignmentDraft[] {
  return phases.reduce<MusicContextAssignmentDraft[]>(
    (next, phase) => effectiveEventPlaylistId(next, snapshots, phase) === null
      ? next
      : chooseEventPlaylist(next, snapshots, phase, null),
    completeMusicAssignmentDrafts(overrides),
  );
}

/**
 * Clears the overrides of the listed phases so they follow the project snapshot again.
 *
 * @param overrides - Event override drafts.
 * @param phases - Phases to restore.
 * @returns The complete next override drafts.
 */
export function restoreEventPlaylists(
  overrides: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): MusicContextAssignmentDraft[] {
  return completeMusicAssignmentDrafts(overrides).map((assignment) => phases.includes(assignment.phase)
    ? emptyMusicAssignmentDraft(assignment.phase)
    : assignment);
}

/**
 * Whether the project snapshot plays a playlist in any listed phase.
 *
 * @param snapshots - Project intent copied onto the event.
 * @param phases - Phases to inspect.
 * @returns True when restoring the project choice would play music.
 */
export function snapshotHasEventPlaylist(
  snapshots: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): boolean {
  return phases.some((phase) => assignmentPlaylistId(snapshots.find((assignment) => assignment.phase === phase)) !== null);
}
