import { describe, expect, it } from "vitest";
import {
  themeEditorSessionHasChanges,
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
