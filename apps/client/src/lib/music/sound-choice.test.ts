import { describe, expect, it } from "vitest";
import { soundChoiceFields, soundChoiceFromAssignment, soundChoicesEqual, type MusicSoundChoice } from "./sound-choice";

describe("soundscape choices", () => {
  it("round trips every choice through assignment fields", () => {
    const choices: MusicSoundChoice[] = [
      { kind: "keep" },
      { kind: "silence" },
      { kind: "preset", presetId: "evening" },
    ];
    for (const choice of choices) {
      expect(soundChoicesEqual(soundChoiceFromAssignment(soundChoiceFields(choice)), choice)).toBe(true);
    }
  });

  it("names a preset only when the soundscape plays", () => {
    expect(soundChoiceFields({ kind: "preset", presetId: "evening" })).toEqual({ soundscapeBehavior: "play-selected", soundscapePresetId: "evening" });
    expect(soundChoiceFields({ kind: "silence" })).toEqual({ soundscapeBehavior: "pause-soundscape", soundscapePresetId: null });
  });

  it("uses the requested keep behavior so an override can mask a lower layer", () => {
    expect(soundChoiceFields({ kind: "keep" }).soundscapeBehavior).toBe("inherit");
    expect(soundChoiceFields({ kind: "keep" }, "keep-current-soundscape").soundscapeBehavior).toBe("keep-current-soundscape");
  });

  it("reads keep current and incomplete play rows as no change", () => {
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "keep-current-soundscape", soundscapePresetId: null })).toEqual({ kind: "keep" });
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "play-selected", soundscapePresetId: null })).toEqual({ kind: "keep" });
  });

  it("ignores a stale preset on a row that does not play", () => {
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "pause-soundscape", soundscapePresetId: "evening" })).toEqual({ kind: "silence" });
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "inherit", soundscapePresetId: "evening" })).toEqual({ kind: "keep" });
  });

  it("tells presets apart by identifier", () => {
    expect(soundChoicesEqual({ kind: "preset", presetId: "rain" }, { kind: "preset", presetId: "wind" })).toBe(false);
    expect(soundChoicesEqual({ kind: "preset", presetId: "rain" }, { kind: "silence" })).toBe(false);
  });
});
