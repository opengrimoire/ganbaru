import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { applyNotesProjectHistoryMutationDeadline } from "$lib/notes/history/project-history-scheduler";
import { invokeNotesMutation } from "./mutation";
import { publishNotesDatabaseChange } from "$lib/notes/database/window-sync";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/notes/database/window-sync", () => ({ publishNotesDatabaseChange: vi.fn() }));
vi.mock("$lib/notes/history/project-history-scheduler", () => ({
  applyNotesProjectHistoryMutationDeadline: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const applyDeadlineMock = vi.mocked(applyNotesProjectHistoryMutationDeadline);

describe("Notes mutation API boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns the value from the current mutation envelope", async () => {
    invokeMock.mockResolvedValue({
      value: { id: "page-1" },
      nextHistoryCheckpointAt: "2026-08-30T12:00:00.000Z",
    });

    await expect(invokeNotesMutation("notes_create_page", { dbUrl: "sqlite:test" }))
      .resolves.toEqual({ id: "page-1" });
    expect(applyDeadlineMock).toHaveBeenCalledWith("2026-08-30T12:00:00.000Z");
    expect(publishNotesDatabaseChange).toHaveBeenCalledOnce();
  });

  it("rejects predecessor raw mutation results", async () => {
    invokeMock.mockResolvedValue({ id: "page-1" });

    await expect(invokeNotesMutation("notes_create_page", { dbUrl: "sqlite:test" }))
      .rejects.toThrow("invalid Notes mutation envelope");
  });

  it("rejects malformed checkpoint deadlines", async () => {
    invokeMock.mockResolvedValue({ value: null, nextHistoryCheckpointAt: 42 });

    await expect(invokeNotesMutation("notes_trash_page", { dbUrl: "sqlite:test" }))
      .rejects.toThrow("invalid Notes history deadline");
  });

  it("does not invalidate snapshots when the native write fails", async () => {
    invokeMock.mockRejectedValue(new Error("Cannot save"));
    await expect(invokeNotesMutation("notes_create_page", {})).rejects.toThrow("Cannot save");
    expect(publishNotesDatabaseChange).not.toHaveBeenCalled();
  });
});
