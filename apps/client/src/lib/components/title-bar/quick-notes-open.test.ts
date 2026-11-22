import { describe, expect, it } from "vitest";
import { quickNotesOpenApplies, startQuickNotesOpen, type TitleBarSurfaces } from "./quick-notes-open";

const NONE_OPEN: TitleBarSurfaces = {
  music: false,
  pomodoro: false,
  performance: false,
  titleBarMenu: false,
  tabContextMenu: false,
  utilityOverflow: false,
  themeQuickSwitcher: false,
  settings: false,
};

describe("quick notes open", () => {
  it("opens the panel when nothing happened while the preload ran", () => {
    const request = startQuickNotesOpen(NONE_OPEN);
    expect(quickNotesOpenApplies(request, request, NONE_OPEN)).toBe(true);
  });

  it("stays closed when a second toggle cancelled the open", () => {
    const request = startQuickNotesOpen(NONE_OPEN);
    expect(quickNotesOpenApplies(null, request, NONE_OPEN)).toBe(false);
  });

  it("leaves the newer open to apply when a later toggle replaced it", () => {
    const first = startQuickNotesOpen(NONE_OPEN);
    const second = startQuickNotesOpen(NONE_OPEN);
    expect(quickNotesOpenApplies(second, first, NONE_OPEN)).toBe(false);
    expect(quickNotesOpenApplies(second, second, NONE_OPEN)).toBe(true);
  });

  it("stays closed when Music opened while the preload ran", () => {
    const request = startQuickNotesOpen(NONE_OPEN);
    expect(quickNotesOpenApplies(request, request, { ...NONE_OPEN, music: true })).toBe(false);
  });

  it("stays closed when Settings opened while the preload ran", () => {
    const request = startQuickNotesOpen(NONE_OPEN);
    expect(quickNotesOpenApplies(request, request, { ...NONE_OPEN, settings: true })).toBe(false);
  });

  it("opens over a surface that was already open when the toggle started", () => {
    const request = startQuickNotesOpen({ ...NONE_OPEN, performance: true });
    expect(quickNotesOpenApplies(request, request, { ...NONE_OPEN, performance: true })).toBe(true);
  });

  it("opens when a surface open at the start closed while the preload ran", () => {
    const request = startQuickNotesOpen({ ...NONE_OPEN, music: true });
    expect(quickNotesOpenApplies(request, request, NONE_OPEN)).toBe(true);
  });
});
