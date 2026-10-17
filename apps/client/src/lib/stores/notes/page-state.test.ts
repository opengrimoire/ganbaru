import { beforeEach, describe, expect, it, vi } from "vitest";

const configMocks = vi.hoisted(() => ({
  values: new Map<string, unknown>(),
  setConfigKey: vi.fn<(key: string, value: unknown) => void>(),
}));

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (key: string, fallback: unknown) =>
    configMocks.values.has(key) ? configMocks.values.get(key) : fallback,
  setConfigKey: configMocks.setConfigKey,
}));

import {
  initialNotesSidebarCollapsedFolderIds,
  notesSidebarCollapsedFolderIdsKey,
  saveNotesSidebarCollapsedFolderIds,
} from "./page-state";

describe("notes folder navigation state", () => {
  beforeEach(() => {
    configMocks.values.clear();
    configMocks.setConfigKey.mockClear();
  });

  it("loads collapsed folder ids defensively", () => {
    configMocks.values.set(notesSidebarCollapsedFolderIdsKey, ["a", "b", "a", 1]);

    expect(initialNotesSidebarCollapsedFolderIds()).toEqual(["a", "b"]);
  });

  it("persists collapsed folder ids and removes an empty value", () => {
    saveNotesSidebarCollapsedFolderIds(["a", "b"]);
    saveNotesSidebarCollapsedFolderIds([]);

    expect(configMocks.setConfigKey).toHaveBeenNthCalledWith(
      1,
      notesSidebarCollapsedFolderIdsKey,
      ["a", "b"],
    );
    expect(configMocks.setConfigKey).toHaveBeenNthCalledWith(
      2,
      notesSidebarCollapsedFolderIdsKey,
      undefined,
    );
  });
});
