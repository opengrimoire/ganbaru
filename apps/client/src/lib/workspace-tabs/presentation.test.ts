import MessagesSquare from "@lucide/svelte/icons/messages-square";
import { describe, expect, it } from "vitest";
import { translate } from "$lib/i18n/translator.svelte";
import { kindLocation } from "./model";
import {
  workspaceTabFullLabel,
  workspaceTabIcon,
  workspaceTabKindIcon,
  workspaceTabLabel,
  workspaceTabPositionShortcut,
  workspaceTabProject,
} from "./presentation";

describe("workspace tab icons", () => {
  it("marks reply thread tabs with the thread icon and other tabs with their kind", () => {
    const thread = { kind: "chat" as const, projectId: "p", item: { type: "chat-reply-thread" as const, id: "r", channelId: "c" } };
    expect(workspaceTabIcon({ location: thread })).toBe(MessagesSquare);
    expect(workspaceTabIcon({ location: { ...thread, item: { type: "chat-channel", id: "c" } } }))
      .toBe(workspaceTabKindIcon("chat"));
    expect(workspaceTabIcon({ location: kindLocation("notes") })).toBe(workspaceTabKindIcon("notes"));
  });
});

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
  });

  it("labels an item tab with its project alone, and with its item when the project is unknown", () => {
    expect(workspaceTabLabel({ location: notes, title: "Roadmap", projectTitle: "Alpha" }, translate)).toBe("Alpha");
    expect(workspaceTabLabel({ location: notes, title: "Roadmap", projectTitle: null }, translate)).toBe("Roadmap");
  });

  it("falls back to the kind name for Calendar and tabs outside a project", () => {
    const calendar = { location: { kind: "calendar" as const, projectId: "p1", item: null }, title: null, projectTitle: "Alpha" };
    expect(workspaceTabLabel(calendar, translate)).toBe("Calendar");
    expect(workspaceTabLabel({ location: kindLocation("notes"), title: null, projectTitle: null }, translate)).toBe("Notes");
    expect(workspaceTabLabel({ location: notes, title: null, projectTitle: null }, translate)).toBe("Notes");
  });
});
