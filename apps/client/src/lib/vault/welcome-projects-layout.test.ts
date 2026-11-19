import { describe, expect, it } from "vitest";
import {
  projectsCellMenuPlace,
  projectsListScrollPx,
  projectsPalettePlace,
  projectsSettingsOffsets,
  type Box,
  type ListLayout,
  type SettingsLayout,
} from "./welcome-projects-layout";

function box(top: number, left: number, width: number, height: number): Box {
  return { top, left, width, height };
}

function listLayout(overrides: Partial<ListLayout> = {}): ListLayout {
  return {
    viewportWidth: 300,
    contentWidth: 600,
    status: box(0, 100, 50, 20),
    subtaskStatus: null,
    priority: box(0, 400, 80, 20),
    menuWidth: 120,
    statusMenuHeight: 80,
    priorityMenuHeight: 60,
    ...overrides,
  };
}

function settingsLayout(overrides: Partial<SettingsLayout> = {}): SettingsLayout {
  return {
    viewport: box(0, 0, 300, 200),
    contentHeight: 1000,
    defaultsTop: 40,
    controls: {
      name: box(50, 0, 200, 30),
      color: box(100, 0, 200, 30),
      duration: box(300, 0, 200, 30),
      pomodoro: box(340, 0, 200, 30),
      focus: box(600, 0, 200, 30),
      "short-break": box(640, 0, 200, 30),
      "long-break": box(900, 0, 200, 30),
    },
    paletteWidth: 200,
    paletteHeight: 100,
    durationMenuHeight: 100,
    pomodoroMenuHeight: 100,
    durationMenuWidth: 100,
    pomodoroMenuWidth: 100,
    playlistMenuHeight: 100,
    rem: 16,
    ...overrides,
  };
}

describe("projectsListScrollPx", () => {
  it("stays put for a status already in view and eases toward the priority", () => {
    const layout = listLayout();
    expect(projectsListScrollPx(layout, { status: 1, priority: 0 })).toBe(0);
    expect(projectsListScrollPx(layout, { status: 1, priority: 0.5 })).toBe(96);
    expect(projectsListScrollPx(layout, { status: 1, priority: 1 })).toBe(192);
  });

  it("scrolls to the status first, then only the rest of the way to the priority", () => {
    const layout = listLayout({ status: box(0, 300, 50, 20) });
    expect(projectsListScrollPx(layout, { status: 0.5, priority: 0 })).toBe(31);
    expect(projectsListScrollPx(layout, { status: 1, priority: 0 })).toBe(62);
    expect(projectsListScrollPx(layout, { status: 1, priority: 1 })).toBe(192);
  });

  it("never scrolls past the end of the list", () => {
    const layout = listLayout({ priority: box(0, 560, 80, 20) });
    expect(projectsListScrollPx(layout, { status: 1, priority: 1 })).toBe(300);
  });
});

describe("projectsCellMenuPlace", () => {
  const base = { menuWidth: 120, menuHeight: 80, scrollX: 0, scrollY: 0, canvasW: 400, canvasH: 400 };

  it("opens below the cell when the menu fits there", () => {
    expect(projectsCellMenuPlace({ ...base, cell: box(100, 50, 60, 20) })).toEqual({ left: 50, top: 126 });
  });

  it("follows the list's scroll", () => {
    expect(projectsCellMenuPlace({ ...base, cell: box(100, 50, 60, 20), scrollX: 30, scrollY: 10 }))
      .toEqual({ left: 20, top: 116 });
  });

  it("opens above a cell near the bottom of the canvas", () => {
    expect(projectsCellMenuPlace({ ...base, cell: box(350, 50, 60, 20) })).toEqual({ left: 50, top: 264 });
  });

  it("keeps the menu inside the canvas edges", () => {
    expect(projectsCellMenuPlace({ ...base, cell: box(100, 380, 60, 20) }).left).toBe(272);
    expect(projectsCellMenuPlace({ ...base, cell: box(100, -20, 60, 20) }).left).toBe(8);
  });
});

describe("projectsSettingsOffsets", () => {
  it("rests on the Defaults heading, then scrolls the least that brings each control into view", () => {
    expect(projectsSettingsOffsets(settingsLayout(), 0, 400)).toEqual({
      name: 36,
      color: 36,
      duration: 138,
      pomodoro: 178,
      focus: 438,
      "short-break": 478,
      "long-break": 738,
    });
  });

  it("scrolls further for the color on a short canvas so its palette fits below it", () => {
    const layout = settingsLayout({ paletteHeight: 150 });
    expect(projectsSettingsOffsets(layout, 0, 400).color).toBe(36);
    expect(projectsSettingsOffsets(layout, 0, 250).color).toBe(44);
  });

  it("never scrolls past the end of the settings", () => {
    const offsets = projectsSettingsOffsets(settingsLayout({ contentHeight: 500 }), 0, 400);
    expect(offsets["long-break"]).toBe(300);
  });
});

describe("projectsPalettePlace", () => {
  const layout = { paletteWidth: 200, paletteHeight: 100, rem: 16 };

  it("opens below the trigger when the palette fits there", () => {
    expect(projectsPalettePlace({ top: 100, left: 20, bottom: 130 }, layout, 400, 400)).toEqual({ left: 20, top: 135 });
  });

  it("opens above the trigger when it does not fit below", () => {
    expect(projectsPalettePlace({ top: 180, left: 20, bottom: 210 }, layout, 400, 300)).toEqual({ left: 20, top: 75 });
  });

  it("pins the palette to the canvas when it fits on neither side", () => {
    expect(projectsPalettePlace({ top: 100, left: 20, bottom: 130 }, layout, 400, 200).top).toBe(92);
  });

  it("keeps the palette inside the canvas sideways", () => {
    expect(projectsPalettePlace({ top: 100, left: 350, bottom: 130 }, layout, 400, 400).left).toBe(192);
    expect(projectsPalettePlace({ top: 100, left: 0, bottom: 130 }, layout, 400, 400).left).toBe(8);
  });
});
