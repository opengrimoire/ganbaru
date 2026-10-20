import { describe, expect, it } from "vitest";
import { soundChoiceFields, soundChoiceFromAssignment, soundChoicesEqual, type MusicSoundChoice } from "./sound-choice";

describe("soundscape choices", () => {
  it("round trips every choice through assignment fields", () => {
    const choices: MusicSoundChoice[] = [
      { kind: "keep" },
      { kind: "silence" },
      { kind: "sound", soundscapeId: "rain" },
      { kind: "preset", presetId: "evening" },
    ];
    for (const choice of choices) {
      expect(soundChoicesEqual(soundChoiceFromAssignment(soundChoiceFields(choice)), choice)).toBe(true);
    }
  });

  it("names exactly one reference only when a sound plays", () => {
    expect(soundChoiceFields({ kind: "preset", presetId: "evening" })).toEqual({ soundscapeBehavior: "play-selected", soundscapeId: null, soundscapePresetId: "evening" });
    expect(soundChoiceFields({ kind: "sound", soundscapeId: "rain" })).toEqual({ soundscapeBehavior: "play-selected", soundscapeId: "rain", soundscapePresetId: null });
    expect(soundChoiceFields({ kind: "silence" })).toEqual({ soundscapeBehavior: "pause-soundscape", soundscapeId: null, soundscapePresetId: null });
  });

  it("uses the requested keep behavior so an override can mask a lower layer", () => {
    expect(soundChoiceFields({ kind: "keep" }).soundscapeBehavior).toBe("inherit");
    expect(soundChoiceFields({ kind: "keep" }, "keep-current-soundscape").soundscapeBehavior).toBe("keep-current-soundscape");
  });

  it("reads keep current and incomplete play rows as no change", () => {
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "keep-current-soundscape", soundscapeId: null, soundscapePresetId: null })).toEqual({ kind: "keep" });
    expect(soundChoiceFromAssignment({ soundscapeBehavior: "play-selected", soundscapeId: null, soundscapePresetId: null })).toEqual({ kind: "keep" });
  });

  it("distinguishes sounds and presets that share an identifier", () => {
    expect(soundChoicesEqual({ kind: "sound", soundscapeId: "same" }, { kind: "preset", presetId: "same" })).toBe(false);
    expect(soundChoicesEqual({ kind: "sound", soundscapeId: "rain" }, { kind: "sound", soundscapeId: "wind" })).toBe(false);
  });
});
