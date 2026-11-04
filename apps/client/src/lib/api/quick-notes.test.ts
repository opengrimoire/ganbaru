import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  createQuickNoteTag,
  downloadQuickNoteImage,
  getQuickNoteConflict,
  importQuickNoteImage,
  mapQuickNote,
  mapQuickNoteConflict,
  mapQuickNoteImage,
  mapQuickNoteImagePick,
  mapQuickNotesTrashPurge,
  mapQuickNoteTag,
  purgeExpiredQuickNotesTrash,
  QuickNoteConflictError,
  QuickNoteImageError,
  readQuickNoteImage,
  QuickNoteTagError,
  QuickNoteWriteError,
  resolveQuickNoteConflict,
  updateQuickNote,
  updateQuickNoteTag,
} from "./quick-notes";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:test" }));

const DIGEST = "ab".repeat(32);

function validImage(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return { sha256: DIGEST, contentType: "image/png", byteSize: 2_048, width: 640, height: 480, ...overrides };
}

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
    images: [validImage()],
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
    images: null,
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
      color: 4,
      createdAt: "2026-07-13T12:00:00.000Z",
      updatedAt: "2026-07-13T12:00:00.000Z",
    };
    expect(mapQuickNoteTag(validTag)).toMatchObject({ name: "Work", orderKey: "a0V", color: 4 });
    expect(() => mapQuickNoteTag({ ...validTag, color: 32 })).toThrow("color is invalid");
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
      const result = updateQuickNoteTag("tag-1", "Work", 4);
      await expect(result).rejects.toBeInstanceOf(QuickNoteTagError);
      await expect(result).rejects.toMatchObject({ code, message: "native wording" });
    },
  );

  it("passes unknown tag failures through unchanged", async () => {
    const failure = { code: "revision_conflict", message: "not a tag code" };
    vi.mocked(invoke).mockRejectedValueOnce(failure);
    await expect(createQuickNoteTag("tag-1", "Work", 4)).rejects.toBe(failure);
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

  it("validates image references on canonical rows", () => {
    expect(mapQuickNote(validQuickNote()).images).toEqual([validImage()]);
    expect(() => mapQuickNote({ ...validQuickNote(), images: null })).toThrow("images must be an array");
    expect(() => mapQuickNote({ ...validQuickNote(), images: [validImage({ sha256: DIGEST.toUpperCase() })] }))
      .toThrow("images[0].sha256 is invalid");
    expect(() => mapQuickNote({ ...validQuickNote(), images: [validImage({ contentType: "image/svg+xml" })] }))
      .toThrow("images[0].contentType is invalid");
    expect(() => mapQuickNote({ ...validQuickNote(), images: [validImage({ width: 0 })] }))
      .toThrow("images[0].width must be positive");
  });

  it("maps image conflict versions and requires the image list for that field", () => {
    const conflict = mapQuickNoteConflict({
      id: "note-1",
      groups: [{ field: "images", versions: [conflictVersion({ title: null, images: [validImage()] })] }],
    });
    expect(conflict.groups[0]?.versions[0]?.images).toEqual([validImage()]);
    expect(() => mapQuickNoteConflict({ id: "note-1", groups: [{ field: "images", versions: [conflictVersion({})] }] }))
      .toThrow("does not match its field");
    expect(() => mapQuickNoteConflict({
      id: "note-1",
      groups: [{ field: "title", versions: [conflictVersion({ images: [validImage()] })] }],
    })).toThrow("does not match its field");
  });

  it("imports raw bytes on desktop and base64 text on Android", async () => {
    const bytes = new Uint8Array([137, 80, 78, 71]);
    vi.mocked(invoke).mockResolvedValueOnce(validImage());
    await expect(importQuickNoteImage(bytes)).resolves.toEqual(validImage());
    expect(invoke).toHaveBeenLastCalledWith("quick_notes_import_image", bytes);

    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "android");
    try {
      vi.mocked(invoke).mockResolvedValueOnce(validImage());
      await importQuickNoteImage(bytes);
      expect(invoke).toHaveBeenLastCalledWith("quick_notes_import_image", { base64: "iVBORw==" });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("encodes large Android imports without exceeding argument limits", async () => {
    const bytes = new Uint8Array(200_000).map((_, index) => index % 251);
    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "android");
    try {
      vi.mocked(invoke).mockResolvedValueOnce(validImage());
      await importQuickNoteImage(bytes);
      const body = vi.mocked(invoke).mock.lastCall?.[1];
      if (typeof body !== "object" || body === null || !("base64" in body) || typeof body.base64 !== "string") {
        throw new Error("expected a base64 body");
      }
      expect(Uint8Array.from(atob(body.base64), (char) => char.charCodeAt(0))).toEqual(bytes);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it.each(["unsupported", "too_large", "unavailable"] as const)("preserves the native %s image code", async (code) => {
    vi.mocked(invoke).mockRejectedValueOnce({ code });
    const result = readQuickNoteImage(mapQuickNoteImage(validImage()));
    await expect(result).rejects.toBeInstanceOf(QuickNoteImageError);
    await expect(result).rejects.toMatchObject({ code });
  });

  it("reads image bytes as an ArrayBuffer by file reference only", async () => {
    const buffer = new ArrayBuffer(4);
    vi.mocked(invoke).mockResolvedValueOnce(buffer);
    await expect(readQuickNoteImage(mapQuickNoteImage(validImage()))).resolves.toBe(buffer);
    expect(invoke).toHaveBeenLastCalledWith("quick_notes_read_image", {
      file: { sha256: DIGEST, contentType: "image/png" },
    });
    vi.mocked(invoke).mockResolvedValueOnce([1, 2, 3]);
    await expect(readQuickNoteImage(mapQuickNoteImage(validImage()))).rejects.toThrow("must be an ArrayBuffer");
  });

  it("validates picker results and keeps per-file rejection codes", () => {
    expect(mapQuickNoteImagePick({
      images: [validImage()],
      rejected: [{ fileName: "notes.png", error: { code: "unsupported" } }],
      skipped: 2,
    })).toEqual({ images: [validImage()], rejected: [{ fileName: "notes.png", code: "unsupported" }], skipped: 2 });
    expect(() => mapQuickNoteImagePick({ images: [], rejected: [{ fileName: "a", error: "boom" }], skipped: 0 }))
      .toThrow("error is invalid");
    expect(() => mapQuickNoteImagePick({ images: [], rejected: [], skipped: -1 })).toThrow("skipped is invalid");
  });

  it("reports a canceled download as not saved", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(false);
    await expect(downloadQuickNoteImage(mapQuickNoteImage(validImage()))).resolves.toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("quick_notes_download_image", {
      file: { sha256: DIGEST, contentType: "image/png" },
    });
  });
});
