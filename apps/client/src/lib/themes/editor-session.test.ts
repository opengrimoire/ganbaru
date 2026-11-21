import { describe, expect, it } from "vitest";
import {
  openThemeEditorSession,
  themeEditorSessionHasChanges,
  transitionThemeEditorSession,
  type ThemeEditorSession,
  type ThemeEditorSessionEvent,
  type ThemeEditorSessionState,
} from "./editor-session";

function session(
  overrides: Partial<ThemeEditorSessionState> = {},
): ThemeEditorSessionState {
  return {
    editingId: "custom",
    snapshot: '{"id":"custom","value":"before"}',
    createdFresh: false,
    previousActiveId: "light",
    ...overrides,
  };
}

describe("themeEditorSessionHasChanges", () => {
  it("treats every freshly duplicated theme as unsaved", () => {
    expect(themeEditorSessionHasChanges(
      session({ createdFresh: true, snapshot: undefined }),
      '{"id":"custom"}',
    )).toBe(true);
  });

  it("compares existing user themes with their opening snapshot", () => {
    const existing = session();
    expect(themeEditorSessionHasChanges(existing, existing.snapshot)).toBe(false);
    expect(themeEditorSessionHasChanges(
      existing,
      '{"id":"custom","value":"after"}',
    )).toBe(true);
  });

  it("keeps built-in read-only previews and absent sessions clean", () => {
    expect(themeEditorSessionHasChanges(
      session({ editingId: "dark", snapshot: undefined }),
      '{"id":"dark"}',
    )).toBe(false);
    expect(themeEditorSessionHasChanges(undefined, undefined)).toBe(false);
  });
});

describe("transitionThemeEditorSession", () => {
  const editing = openThemeEditorSession(session());
  const committing: ThemeEditorSession = { ...editing, phase: "committing" };

  it("opens sessions in the editing phase", () => {
    expect(editing).toEqual({ ...session(), phase: "editing" });
  });

  it("starts one save at a time", () => {
    expect(transitionThemeEditorSession(editing, { type: "commitStarted" })).toEqual(committing);
    expect(transitionThemeEditorSession(committing, { type: "commitStarted" })).toBe(committing);
  });

  it("closes the session when the save lands", () => {
    expect(transitionThemeEditorSession(committing, { type: "committed" })).toBeUndefined();
  });

  it("reopens editing when the save stops at a conflict or fails", () => {
    expect(transitionThemeEditorSession(committing, { type: "commitStopped" })).toEqual(editing);
  });

  it("cancels only while editing", () => {
    expect(transitionThemeEditorSession(editing, { type: "cancelled" })).toBeUndefined();
    expect(transitionThemeEditorSession(committing, { type: "cancelled" })).toBe(committing);
  });

  it.each<ThemeEditorSessionEvent>([{ type: "committed" }, { type: "commitStopped" }])(
    "ignores $type without a save in flight",
    (event) => {
      expect(transitionThemeEditorSession(editing, event)).toBe(editing);
    },
  );

  it.each<ThemeEditorSessionEvent>([
    { type: "commitStarted" }, { type: "committed" }, { type: "commitStopped" }, { type: "cancelled" },
  ])("ignores $type without a session", (event) => {
    expect(transitionThemeEditorSession(undefined, event)).toBeUndefined();
  });
});
