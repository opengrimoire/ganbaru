import { describe, expect, it } from "vitest";
import { EVENT_COLOR_OPTIONS } from "$lib/calendar/utils";
import {
  nextProjectSettingsPaletteColor,
  nextUnusedProjectSettingsColor,
  projectSettingsScrollTargetTop,
} from "./ui";

describe("nextProjectSettingsPaletteColor", () => {
  it("moves to the next palette color", () => {
    expect(nextProjectSettingsPaletteColor(8, 13)).toBe(9);
  });

  it("uses the fallback for an unknown color", () => {
    expect(nextProjectSettingsPaletteColor(100, 13)).toBe(13);
  });
});

describe("nextUnusedProjectSettingsColor", () => {
  it("returns the preferred color when it is unused", () => {
    expect(nextUnusedProjectSettingsColor({
      preferredColor: 8,
      usedColors: new Set([7, 9]),
      fallbackColor: 13,
    })).toBe(8);
  });

  it("walks forward to the next unused color", () => {
    expect(nextUnusedProjectSettingsColor({
      preferredColor: 8,
      usedColors: new Set([8, 9, 10]),
      fallbackColor: 13,
    })).toBe(11);
  });

  it("uses the random palette color when all colors are used", () => {
    expect(nextUnusedProjectSettingsColor({
      preferredColor: 8,
      usedColors: new Set(EVENT_COLOR_OPTIONS),
      fallbackColor: 13,
      random: () => 0.5,
    })).toBe(EVENT_COLOR_OPTIONS[Math.floor(EVENT_COLOR_OPTIONS.length * 0.5)]);
  });
});

describe("projectSettingsScrollTargetTop", () => {
  it("keeps the current scroll top when the row is fully visible", () => {
    expect(projectSettingsScrollTargetTop({
      scrollTop: 20,
      scrollHeight: 400,
      clientHeight: 120,
      scrollTopBoundary: 0,
      scrollBottomBoundary: 120,
      rowTop: 40,
      rowBottom: 90,
      fadeInset: 16,
    })).toBe(20);
  });

  it("scrolls up enough to reveal a clipped row", () => {
    expect(projectSettingsScrollTargetTop({
      scrollTop: 60,
      scrollHeight: 400,
      clientHeight: 120,
      scrollTopBoundary: 0,
      scrollBottomBoundary: 120,
      rowTop: 8,
      rowBottom: 48,
      fadeInset: 16,
    })).toBe(48);
  });

  it("scrolls down enough to reveal a clipped row", () => {
    expect(projectSettingsScrollTargetTop({
      scrollTop: 20,
      scrollHeight: 400,
      clientHeight: 120,
      scrollTopBoundary: 0,
      scrollBottomBoundary: 120,
      rowTop: 100,
      rowBottom: 150,
      fadeInset: 16,
    })).toBe(70);
  });
});
