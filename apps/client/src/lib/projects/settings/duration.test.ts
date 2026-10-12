import { describe, expect, it } from "vitest";
import {
  isProjectCustomDurationInputShape,
  projectEffectiveDurationMinutes,
  projectCustomDurationDraftFromMinutes,
  projectDurationMinutesFromCustomInput,
  projectDurationPresetFromMinutes,
} from "./duration";

describe("project settings duration helpers", () => {
  it("maps a null stored duration to the default preset", () => {
    expect(projectDurationPresetFromMinutes(null)).toBe("default");
  });

  it("maps all-day time mode to the all-day preset", () => {
    expect(projectDurationPresetFromMinutes(null, "all_day")).toBe("all_day");
    expect(projectDurationPresetFromMinutes(90, "all_day")).toBe("all_day");
  });

  it("keeps an explicit hour separate from the default preset", () => {
    expect(projectDurationPresetFromMinutes(60)).toBe("60");
  });

  it("maps listed durations to their matching presets", () => {
    expect(projectDurationPresetFromMinutes(10)).toBe("10");
    expect(projectDurationPresetFromMinutes(120)).toBe("120");
    expect(projectDurationPresetFromMinutes(240)).toBe("240");
  });

  it("uses a custom preset for unlisted stored durations", () => {
    expect(projectDurationPresetFromMinutes(90)).toBe("custom");
    expect(projectDurationPresetFromMinutes(300)).toBe("custom");
  });

  it("prefers an hour draft when it can preserve up to two decimal places", () => {
    expect(projectCustomDurationDraftFromMinutes(90)).toEqual({
      value: "1.5",
      unit: "hours",
    });
    expect(projectCustomDurationDraftFromMinutes(75)).toEqual({
      value: "1.25",
      unit: "hours",
    });
  });

  it("uses a minute draft when an hour draft would lose precision", () => {
    expect(projectCustomDurationDraftFromMinutes(61)).toEqual({
      value: "61",
      unit: "minutes",
    });
  });

  it("falls back to the app default duration when no project duration is stored", () => {
    expect(projectEffectiveDurationMinutes(null)).toBe(60);
    expect(projectEffectiveDurationMinutes(undefined)).toBe(60);
    expect(projectEffectiveDurationMinutes(90)).toBe(90);
  });

  it("accepts empty or two-decimal custom input shapes", () => {
    expect(isProjectCustomDurationInputShape("")).toBe(true);
    expect(isProjectCustomDurationInputShape("1")).toBe(true);
    expect(isProjectCustomDurationInputShape("1.5")).toBe(true);
    expect(isProjectCustomDurationInputShape("1.25")).toBe(true);
  });

  it("rejects custom input shapes with more than two decimal places", () => {
    expect(isProjectCustomDurationInputShape("1.255")).toBe(false);
  });

  it("converts custom hours to stored minutes", () => {
    expect(projectDurationMinutesFromCustomInput("1.5", "hours")).toBe(90);
  });

  it("rounds custom minute decimals to stored whole minutes", () => {
    expect(projectDurationMinutesFromCustomInput("10.5", "minutes")).toBe(11);
  });

  it("rejects durations above 24 hours", () => {
    expect(projectDurationMinutesFromCustomInput("24.1", "hours")).toBeNull();
    expect(projectDurationMinutesFromCustomInput("1440.1", "minutes")).toBeNull();
  });
});
