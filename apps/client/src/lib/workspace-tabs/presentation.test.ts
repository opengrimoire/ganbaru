import { describe, expect, it } from "vitest";
import { translate } from "$lib/i18n/translator.svelte";
import { kindLocation } from "./model";
import {
  shortWorkspaceTabProject,
  workspaceTabFullLabel,
  workspaceTabLabel,
  workspaceTabPositionShortcut,
  workspaceTabProject,
  workspaceTabProjectHint,
} from "./presentation";

describe("workspace tab position shortcuts", () => {
  it("numbers the first eight tabs", () => {
    expect(workspaceTabPositionShortcut(0, 3)).toBe("Alt+1");
    expect(workspaceTabPositionShortcut(7, 12)).toBe("Alt+8");
  });

  it("gives the last tab Alt+9 whatever its position", () => {
    expect(workspaceTabPositionShortcut(8, 9)).toBe("Alt+9");
    expect(workspaceTabPositionShortcut(11, 12)).toBe("Alt+9");
  });

  it("leaves tabs between the eighth and the last without a shortcut", () => {
    expect(workspaceTabPositionShortcut(8, 12)).toBeNull();
    expect(workspaceTabPositionShortcut(10, 12)).toBeNull();
  });

  it("keeps a short strip's last tab on its own number", () => {
    expect(workspaceTabPositionShortcut(2, 3)).toBe("Alt+3");
  });
});

describe("workspace tab project names", () => {
  const notes = { kind: "notes" as const, projectId: "p1", item: null };

  it("shows the project for every kind except Calendar", () => {
    expect(workspaceTabProject({ location: notes, projectTitle: "Alpha" })).toBe("Alpha");
    expect(workspaceTabProject({ location: { kind: "calendar", projectId: "p1", item: null }, projectTitle: "Alpha" }))
      .toBeNull();
  });

  it("shows no project for a tab outside one, even with a stale cached name", () => {
    expect(workspaceTabProject({ location: { ...notes, projectId: null }, projectTitle: "Alpha" })).toBeNull();
    expect(workspaceTabProject({ location: notes, projectTitle: null })).toBeNull();
  });

  it("puts the project in parentheses after the label in the full name", () => {
    expect(workspaceTabFullLabel({ location: notes, title: "Roadmap", projectTitle: "Alpha" }, translate))
      .toBe("Roadmap (Alpha)");
    expect(workspaceTabFullLabel({ location: kindLocation("calendar"), title: null, projectTitle: null }, translate))
      .toBe("Calendar");
    expect(workspaceTabFullLabel({ location: notes, title: null, projectTitle: "Alpha" }, translate))
      .toBe("Notes (Alpha)");
  });

  it("labels a project tab with its project alone, leaving the kind to its icon", () => {
    expect(workspaceTabLabel({ location: notes, title: null, projectTitle: "Alpha" }, translate)).toBe("Alpha");
    expect(workspaceTabProjectHint({ location: notes, title: null, projectTitle: "Alpha" })).toBeNull();
  });

  it("labels an item tab with its item and keeps the project as a hint", () => {
    const tab = { location: notes, title: "Roadmap", projectTitle: "Alpha" };
    expect(workspaceTabLabel(tab, translate)).toBe("Roadmap");
    expect(workspaceTabProjectHint(tab)).toBe("Alpha");
  });

  it("falls back to the kind name for Calendar and tabs outside a project", () => {
    const calendar = { location: { kind: "calendar" as const, projectId: "p1", item: null }, title: null, projectTitle: "Alpha" };
    expect(workspaceTabLabel(calendar, translate)).toBe("Calendar");
    expect(workspaceTabLabel({ location: kindLocation("notes"), title: null, projectTitle: null }, translate)).toBe("Notes");
    expect(workspaceTabLabel({ location: notes, title: null, projectTitle: null }, translate)).toBe("Notes");
  });

  it("keeps short project names whole", () => {
    expect(shortWorkspaceTabProject("Alpha", 5)).toBe("Alpha");
    expect(shortWorkspaceTabProject("  Alpha  ", 5)).toBe("Alpha");
  });

  it("cuts long project names at the limit with an ellipsis and no trailing space", () => {
    expect(shortWorkspaceTabProject("Alpha Beta Gamma", 7)).toBe("Alpha…");
    expect(shortWorkspaceTabProject("Abcdefgh", 4)).toBe("Abc…");
  });

  it("counts characters rather than code units", () => {
    expect(shortWorkspaceTabProject("😀😀😀😀", 3)).toBe("😀😀…");
  });
});
