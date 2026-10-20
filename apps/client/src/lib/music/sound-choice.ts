import type { MusicContextAssignmentDraft, MusicSoundscapeBehavior } from "./context-assignment";

/** Soundscape choice for one Focus phase, as shown in the phase pickers. */
export type MusicSoundChoice =
  | { kind: "keep" }
  | { kind: "silence" }
  | { kind: "sound"; soundscapeId: string }
  | { kind: "preset"; presetId: string };

/** Assignment fields that carry the soundscape part of a phase. */
export type MusicSoundChoiceFields = Pick<MusicContextAssignmentDraft, "soundscapeBehavior" | "soundscapeId" | "soundscapePresetId">;

/** Reads the soundscape choice stored on an assignment, treating inherit and keep alike. */
export function soundChoiceFromAssignment(assignment: MusicSoundChoiceFields): MusicSoundChoice {
  if (assignment.soundscapeBehavior === "pause-soundscape") return { kind: "silence" };
  if (assignment.soundscapeBehavior !== "play-selected") return { kind: "keep" };
  if (assignment.soundscapePresetId !== null) return { kind: "preset", presetId: assignment.soundscapePresetId };
  if (assignment.soundscapeId !== null) return { kind: "sound", soundscapeId: assignment.soundscapeId };
  return { kind: "keep" };
}

/**
 * Converts a choice into assignment fields that satisfy the native rule: play-selected names exactly one sound or preset, and other behaviors name neither.
 *
 * @param keepBehavior - Behavior used for "Don't change". Layers above a stored sound need `keep-current-soundscape` so the lower layer does not apply.
 */
export function soundChoiceFields(
  choice: MusicSoundChoice,
  keepBehavior: Extract<MusicSoundscapeBehavior, "inherit" | "keep-current-soundscape"> = "inherit",
): MusicSoundChoiceFields {
  switch (choice.kind) {
    case "keep": return { soundscapeBehavior: keepBehavior, soundscapeId: null, soundscapePresetId: null };
    case "silence": return { soundscapeBehavior: "pause-soundscape", soundscapeId: null, soundscapePresetId: null };
    case "sound": return { soundscapeBehavior: "play-selected", soundscapeId: choice.soundscapeId, soundscapePresetId: null };
    case "preset": return { soundscapeBehavior: "play-selected", soundscapeId: null, soundscapePresetId: choice.presetId };
  }
}

/** Compares two choices by meaning. */
export function soundChoicesEqual(left: MusicSoundChoice, right: MusicSoundChoice): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "sound" && right.kind === "sound") return left.soundscapeId === right.soundscapeId;
  if (left.kind === "preset" && right.kind === "preset") return left.presetId === right.presetId;
  return true;
}

/** Reports whether a choice changes the soundscape when its phase starts. */
export function soundChoiceIsActive(choice: MusicSoundChoice): boolean {
  return choice.kind !== "keep";
}
