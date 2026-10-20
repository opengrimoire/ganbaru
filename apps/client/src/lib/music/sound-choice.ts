import type { MusicContextAssignmentDraft, MusicSoundscapeBehavior } from "./context-assignment";

/** Soundscape choice for one Focus phase, as shown in the phase pickers. A single sound plays through its one-sound preset. */
export type MusicSoundChoice =
  | { kind: "keep" }
  | { kind: "silence" }
  | { kind: "preset"; presetId: string };

/** Assignment fields that carry the soundscape part of a phase. */
export type MusicSoundChoiceFields = Pick<MusicContextAssignmentDraft, "soundscapeBehavior" | "soundscapePresetId">;

/** Reads the soundscape choice stored on an assignment, treating inherit and keep alike. */
export function soundChoiceFromAssignment(assignment: MusicSoundChoiceFields): MusicSoundChoice {
  if (assignment.soundscapeBehavior === "pause-soundscape") return { kind: "silence" };
  if (assignment.soundscapeBehavior !== "play-selected") return { kind: "keep" };
  if (assignment.soundscapePresetId !== null) return { kind: "preset", presetId: assignment.soundscapePresetId };
  return { kind: "keep" };
}

/**
 * Converts a choice into assignment fields that satisfy the native rule: play-selected names exactly one preset, and other behaviors name none.
 *
 * @param keepBehavior - Behavior used for "Don't change". Layers above a stored preset need `keep-current-soundscape` so the lower layer does not apply.
 */
export function soundChoiceFields(
  choice: MusicSoundChoice,
  keepBehavior: Extract<MusicSoundscapeBehavior, "inherit" | "keep-current-soundscape"> = "inherit",
): MusicSoundChoiceFields {
  switch (choice.kind) {
    case "keep": return { soundscapeBehavior: keepBehavior, soundscapePresetId: null };
    case "silence": return { soundscapeBehavior: "pause-soundscape", soundscapePresetId: null };
    case "preset": return { soundscapeBehavior: "play-selected", soundscapePresetId: choice.presetId };
  }
}

/** Compares two choices by meaning. */
export function soundChoicesEqual(left: MusicSoundChoice, right: MusicSoundChoice): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "preset" && right.kind === "preset") return left.presetId === right.presetId;
  return true;
}

/** Reports whether a choice changes the soundscape when its phase starts. */
export function soundChoiceIsActive(choice: MusicSoundChoice): boolean {
  return choice.kind !== "keep";
}
