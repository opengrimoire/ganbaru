import { describe, expect, it } from "vitest";
import {
  isNotesPageOpenMode,
  notesContextualPageOpenMode,
  notesPageOpenModeForSelection,
  notesDefaultOpenModeForProject,
  type NotesPageOpenMode,
} from "./open-mode";

describe("notes page open mode", () => {
  it("uses a project override before the global default", () => {
    expect(notesDefaultOpenModeForProject("center", "side")).toBe("side");
    expect(notesDefaultOpenModeForProject("full", null)).toBe("full");
    expect(notesDefaultOpenModeForProject("side", undefined)).toBe("side");
  });

  it("opens fresh page selections as a full page", () => {
    expect(
      notesPageOpenModeForSelection({
        currentOpenMode: "side",
        hasOpenPage: false,
      }),
    ).toBe("full");
  });

  it.each<NotesPageOpenMode>(["side", "center", "full"])(
    "keeps %s mode when another page is already open",
    (currentOpenMode) => {
      expect(
        notesPageOpenModeForSelection({
          currentOpenMode,
          hasOpenPage: true,
        }),
      ).toBe(currentOpenMode);
    },
  );

  it("lets explicit open modes override the current mode", () => {
    expect(
      notesPageOpenModeForSelection({
        requestedOpenMode: "center",
        currentOpenMode: "side",
        hasOpenPage: true,
      }),
    ).toBe("center");
  });

  it("uses the configured mode when opening from a primary note", () => {
    expect(notesContextualPageOpenMode("full", "center")).toBe("center");
    expect(notesContextualPageOpenMode("full", "side")).toBe("side");
  });

  it("keeps the current peek mode through contextual navigation", () => {
    expect(notesContextualPageOpenMode("center", "side")).toBe("center");
    expect(notesContextualPageOpenMode("side", "center")).toBe("side");
  });

  it("accepts only known open mode values", () => {
    expect(isNotesPageOpenMode("side")).toBe(true);
    expect(isNotesPageOpenMode("center")).toBe(true);
    expect(isNotesPageOpenMode("full")).toBe(true);
    expect(isNotesPageOpenMode("new-tab")).toBe(false);
    expect(isNotesPageOpenMode(undefined)).toBe(false);
  });
});
