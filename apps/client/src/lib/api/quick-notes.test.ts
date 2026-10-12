import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  createQuickNoteTag,
  getQuickNoteConflict,
  mapQuickNote,
  mapQuickNoteConflict,
  mapQuickNotesTrashPurge,
  mapQuickNoteTag,
  purgeExpiredQuickNotesTrash,
  QuickNoteConflictError,
  QuickNoteTagError,
  QuickNoteWriteError,
  renameQuickNoteTag,
  resolveQuickNoteConflict,
  updateQuickNote,
} from "./quick-notes";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:test" }));

function validQuickNote(): Record<string, unknown> {
  return {
    id: "note-1",
    title: "Plan",
    bodyPlainText: "Start here",
    runs: [
      {
        content: "Start here",
        bold: true,
        italic: false,
        underline: false,
      },
    ],
    previewTruncated: false,
    color: 4,
    tagId: "tag-1",
    pinned: true,
    archived: false,
    trashedAt: null,
    revision: 2,
    createdAt: "2026-07-13T12:00:00.000Z",
    updatedAt: "2026-07-13T12:01:00.000Z",
    hasConflict: false,
  };
}

function conflictVersion(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    version: "v1",
    device: { deviceId: "device-a", deviceLabel: "Laptop", ownDevice: true },
    editedAtMs: 1_000,
    displayed: true,
    title: "Plan",
    runs: null,
    ...overrides,
  };
}

describe("Quick notes API boundary", () => {
  it.each(["revision_conflict", "failed"] as const)("preserves the native %s code independently of wording", async (code) => {
    const message = code === "revision_conflict" ? "The note changed elsewhere" : "Unable to inspect revision conflict state";
    vi.mocked(invoke).mockRejectedValueOnce({ code, message });
    const note = mapQuickNote(validQuickNote());
    const result = updateQuickNote({ ...note, expectedRevision: note.revision });
    await expect(result).rejects.toBeInstanceOf(QuickNoteWriteError);
    await expect(result).rejects.toMatchObject({ code, message });
  });

  it("maps a valid canonical row", () => {
    expect(mapQuickNote(validQuickNote())).toMatchObject({
      id: "note-1",
      color: 4,
      revision: 2,
      pinned: true,
    });
  });

  it("rejects malformed rows and nested runs", () => {
    expect(() => mapQuickNote({ ...validQuickNote(), color: null })).toThrow("color is invalid");
    expect(() => mapQuickNote({ ...validQuickNote(), color: 32 })).toThrow("color is invalid");
    expect(() =>
      mapQuickNote({
        ...validQuickNote(),
        runs: [{ content: "text", bold: "yes", italic: false, underline: false }],
      }),
    ).toThrow("runs[0].bold must be a boolean");
    expect(() => mapQuickNote({ ...validQuickNote(), revision: 1.5 })).toThrow(
      "revision must be an integer",
    );
  });

  it("validates canonical tag rows", () => {
    const validTag = {
      id: "tag-1",
      name: "Work",
      orderKey: "a0V",
      createdAt: "2026-07-13T12:00:00.000Z",
      updatedAt: "2026-07-13T12:00:00.000Z",
    };
    expect(mapQuickNoteTag(validTag)).toMatchObject({ name: "Work", orderKey: "a0V" });
    expect(() => mapQuickNoteTag({ ...validTag, name: " Work " })).toThrow("name is invalid");
    expect(() => mapQuickNoteTag({ ...validTag, orderKey: 8 })).toThrow("orderKey must be a string");
    expect(() => mapQuickNoteTag({ ...validTag, orderKey: "" })).toThrow("orderKey is invalid");
    expect(() => mapQuickNoteTag({ ...validTag, orderKey: "a0 " })).toThrow("orderKey is invalid");
    expect(() => mapQuickNoteTag({ ...validTag, orderKey: `a0${"1".repeat(127)}` })).toThrow(
      "orderKey is invalid",
    );
  });

  it.each(["duplicate_name", "limit_reached", "not_found", "failed"] as const)(
    "preserves the native %s tag code",
    async (code) => {
      vi.mocked(invoke).mockRejectedValueOnce({ code, message: "native wording" });
      const result = renameQuickNoteTag("tag-1", "Work");
      await expect(result).rejects.toBeInstanceOf(QuickNoteTagError);
      await expect(result).rejects.toMatchObject({ code, message: "native wording" });
    },
  );

  it("passes unknown tag failures through unchanged", async () => {
    const failure = { code: "revision_conflict", message: "not a tag code" };
    vi.mocked(invoke).mockRejectedValueOnce(failure);
    await expect(createQuickNoteTag("tag-1", "Work")).rejects.toBe(failure);
  });

  it("validates trash purge results", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ purged: 2, nextPurgeAt: "2026-10-15T00:00:00.000Z" });
    await expect(purgeExpiredQuickNotesTrash()).resolves.toEqual({
      purged: 2,
      nextPurgeAt: "2026-10-15T00:00:00.000Z",
    });
    expect(invoke).toHaveBeenLastCalledWith("quick_notes_purge_expired_trash", { dbUrl: "sqlite:test" });
    expect(mapQuickNotesTrashPurge({ purged: 0, nextPurgeAt: null })).toEqual({ purged: 0, nextPurgeAt: null });
    expect(() => mapQuickNotesTrashPurge({ purged: -1, nextPurgeAt: null })).toThrow("purged is invalid");
    expect(() => mapQuickNotesTrashPurge({ purged: 1 })).toThrow("nextPurgeAt must be a string");
  });
  it("requires the conflict flag on canonical rows", () => {
    expect(mapQuickNote({ ...validQuickNote(), hasConflict: true }).hasConflict).toBe(true);
    const { hasConflict: _omitted, ...withoutFlag } = validQuickNote();
    expect(() => mapQuickNote(withoutFlag)).toThrow("hasConflict must be a boolean");
  });

  it("maps title and body conflict versions", () => {
    const conflict = mapQuickNoteConflict({
      id: "note-1",
      groups: [
        { field: "title", versions: [conflictVersion({}), conflictVersion({ version: "v2", displayed: false, title: "Other" })] },
        {
          field: "body",
          versions: [conflictVersion({ title: null, runs: [{ content: "Body", bold: false, italic: true, underline: false }] })],
        },
      ],
    });
    expect(conflict.groups.map((group) => group.field)).toEqual(["title", "body"]);
    expect(conflict.groups[0]?.versions[1]).toMatchObject({ version: "v2", title: "Other", displayed: false });
    expect(conflict.groups[1]?.versions[0]?.runs).toEqual([{ content: "Body", bold: false, italic: true, underline: false }]);
    expect(conflict.groups[0]?.versions[0]?.device).toEqual({ deviceId: "device-a", deviceLabel: "Laptop", ownDevice: true });
  });

  it("rejects conflict versions whose value does not match the field", () => {
    expect(() => mapQuickNoteConflict({ id: "note-1", groups: [{ field: "body", versions: [conflictVersion({})] }] }))
      .toThrow("does not match its field");
    expect(() => mapQuickNoteConflict({ id: "note-1", groups: [{ field: "color", versions: [] }] }))
      .toThrow("field is invalid");
    expect(() => mapQuickNoteConflict({ id: "note-1", groups: [{ field: "title", versions: [conflictVersion({ editedAtMs: -1 })] }] }))
      .toThrow("editedAtMs is invalid");
  });

  it("sends resolutions and maps the kept copy", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ note: validQuickNote(), copy: { ...validQuickNote(), id: "note-2" } });
    const resolved = await resolveQuickNoteConflict({ id: "note-1", field: "title", choice: { kind: "keep_both", version: "v2" } });
    expect(resolved.copy?.id).toBe("note-2");
    expect(invoke).toHaveBeenLastCalledWith("quick_notes_resolve_conflict", {
      dbUrl: "sqlite:test",
      resolution: { id: "note-1", field: "title", choice: { kind: "keep_both", version: "v2" } },
    });
  });

  it("preserves the native resolved conflict code", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: "resolved", message: "gone" });
    const result = getQuickNoteConflict("note-1");
    await expect(result).rejects.toBeInstanceOf(QuickNoteConflictError);
    await expect(result).rejects.toMatchObject({ code: "resolved" });
  });
});
