import { describe, expect, it } from "vitest";
import { translate } from "$lib/i18n/translator.svelte";
import { kindLocation } from "./model";
import {
  shortWorkspaceTabProject,
  workspaceTabFullLabel,
  workspaceTabPositionShortcut,
  workspaceTabProject,
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
