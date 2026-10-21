import {
  behaviorUsesPlaylist,
  completeMusicAssignmentDrafts,
  emptyMusicAssignmentDraft,
  MUSIC_ACTIVITY_PHASES,
} from "./assignment-draft";
import type { MusicActivityPhase, MusicContextAssignmentDraft } from "./context-assignment";
import { soundChoiceFields, soundChoiceFromAssignment, soundChoicesEqual, type MusicSoundChoice } from "./sound-choice";

/**
 * Lists the phases an event can choose music for. Without Pomodoro the whole event runs as one
 * Focus phase, so only that phase is offered.
 *
 * @param pomodoroEnabled - Whether the event runs Pomodoro phases.
 * @returns The phases shown as music rows.
 */
export function eventMusicPhases(pomodoroEnabled: boolean): readonly MusicActivityPhase[] {
  return pomodoroEnabled ? MUSIC_ACTIVITY_PHASES : ["focus"];
}

function phaseRow(assignments: readonly MusicContextAssignmentDraft[], phase: MusicActivityPhase): MusicContextAssignmentDraft | undefined {
  return assignments.find((assignment) => assignment.phase === phase);
}

/** Reads the playlist a single row plays, or null when it plays none. */
function assignmentPlaylistId(assignment: MusicContextAssignmentDraft | undefined): string | null {
  return assignment && behaviorUsesPlaylist(assignment.behavior) ? assignment.playlistId : null;
}

/** Reads the soundscape a single row chooses, where a missing row changes nothing. */
function assignmentSound(assignment: MusicContextAssignmentDraft | undefined): MusicSoundChoice {
  return assignment ? soundChoiceFromAssignment(assignment) : { kind: "keep" };
}

/**
 * Resolves the playlist a phase plays for the event. Music and soundscape resolve
 * independently like the native session: the override decides music only when its music
 * behavior is set, otherwise the project snapshot does.
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
  const override = phaseRow(overrides, phase);
  if (override && override.behavior !== "inherit") return assignmentPlaylistId(override);
  return assignmentPlaylistId(phaseRow(snapshots, phase));
}

/**
 * Resolves the soundscape a phase uses for the event: the override when its sound behavior
 * is set, otherwise the project snapshot.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phase - Phase to resolve.
 * @returns The effective soundscape choice.
 */
export function effectiveEventSound(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phase: MusicActivityPhase,
): MusicSoundChoice {
  const override = phaseRow(overrides, phase);
  if (override && override.soundscapeBehavior !== "inherit") return assignmentSound(override);
  return assignmentSound(phaseRow(snapshots, phase));
}

/**
 * Applies a playlist choice for one phase without touching its soundscape. A choice equal
 * to the project snapshot clears the music override so the event keeps following its project; any
 * other choice plays that playlist automatically, or silence for null.
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
  const inherited = assignmentPlaylistId(phaseRow(snapshots, phase));
  return completeMusicAssignmentDrafts(overrides).map((assignment): MusicContextAssignmentDraft => {
    if (assignment.phase !== phase) return assignment;
    if (playlistId === inherited) return { ...assignment, behavior: "inherit", playlistId: null };
    return { ...assignment, behavior: playlistId ? "play-automatically" : "pause-music", playlistId };
  });
}

/**
 * Applies a soundscape choice for one phase without touching its music. A choice equal to
 * the project snapshot clears the soundscape override; Silence over a project preset is stored
 * as keeping the current soundscape so the project preset does not apply.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phase - Phase being changed.
 * @param choice - Chosen soundscape.
 * @returns The complete next override drafts.
 */
export function chooseEventSound(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phase: MusicActivityPhase,
  choice: MusicSoundChoice,
): MusicContextAssignmentDraft[] {
  const inherited = assignmentSound(phaseRow(snapshots, phase));
  const fields = soundChoicesEqual(choice, inherited)
    ? soundChoiceFields({ kind: "keep" })
    : soundChoiceFields(choice, "keep-current-soundscape");
  return completeMusicAssignmentDrafts(overrides).map((assignment) => assignment.phase === phase
    ? { ...assignment, ...fields }
    : assignment);
}

/** Whether a soundscape choice starts sound when its phase begins. */
export function eventSoundPlays(choice: MusicSoundChoice): boolean {
  return choice.kind === "preset";
}

/**
 * Silences music and soundscape in every listed phase that currently plays them.
 *
 * @param overrides - Event override drafts.
 * @param snapshots - Project intent copied onto the event.
 * @param phases - Phases to silence.
 * @returns The complete next override drafts.
 */
export function silenceEventMusic(
  overrides: readonly MusicContextAssignmentDraft[],
  snapshots: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): MusicContextAssignmentDraft[] {
  return phases.reduce<MusicContextAssignmentDraft[]>((next, phase) => {
    const quiet = effectiveEventPlaylistId(next, snapshots, phase) === null
      ? next
      : chooseEventPlaylist(next, snapshots, phase, null);
    return eventSoundPlays(effectiveEventSound(quiet, snapshots, phase))
      ? chooseEventSound(quiet, snapshots, phase, { kind: "silence" })
      : quiet;
  }, completeMusicAssignmentDrafts(overrides));
}

/**
 * Clears the overrides of the listed phases so their music and soundscape follow the
 * project snapshot again.
 *
 * @param overrides - Event override drafts.
 * @param phases - Phases to restore.
 * @returns The complete next override drafts.
 */
export function restoreEventMusic(
  overrides: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): MusicContextAssignmentDraft[] {
  return completeMusicAssignmentDrafts(overrides).map((assignment) => phases.includes(assignment.phase)
    ? emptyMusicAssignmentDraft(assignment.phase)
    : assignment);
}

/**
 * Whether the project snapshot plays a playlist or soundscape in any listed phase.
 *
 * @param snapshots - Project intent copied onto the event.
 * @param phases - Phases to inspect.
 * @returns True when restoring the project choice would play something.
 */
export function snapshotHasEventMusic(
  snapshots: readonly MusicContextAssignmentDraft[],
  phases: readonly MusicActivityPhase[],
): boolean {
  return phases.some((phase) => {
    const snapshot = phaseRow(snapshots, phase);
    return assignmentPlaylistId(snapshot) !== null || eventSoundPlays(assignmentSound(snapshot));
  });
}
