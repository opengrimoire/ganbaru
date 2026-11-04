import { describe, expect, it } from "vitest";
import {
  NotesConflictError,
  mapNotesMismatchedValuePreview,
  mapNotesPageConflicts,
  toNotesConflictError,
} from "./conflicts";

const device = { deviceId: "device-a", deviceLabel: "Phone", ownDevice: false };

describe("Notes conflict mapping", () => {
  it("maps the conflicts of a page", () => {
    const read = mapNotesPageConflicts({
      pageId: "page-1",
      conflicts: [{
        target: "block_text",
        id: "block-1",
        versions: [
          { version: "ab", device, editedAtMs: 5, displayed: true, text: "Mine" },
          { version: "cd", device, editedAtMs: 7, displayed: false, text: "Theirs" },
        ],
      }],
    });
    expect(read.pageId).toBe("page-1");
    expect(read.conflicts[0]?.target).toBe("block_text");
    expect(read.conflicts[0]?.versions.map((version) => version.text)).toEqual(["Mine", "Theirs"]);
  });

  it("rejects a target the native side does not define", () => {
    expect(() => mapNotesPageConflicts({
      pageId: "p",
      conflicts: [{ target: "page_icon", id: "p", versions: [] }],
    })).toThrow("notes conflicts.conflicts[0].target has an unsupported value");
  });

  it("rejects a malformed read", () => {
    expect(() => mapNotesPageConflicts({ pageId: "p", conflicts: {} })).toThrow();
    expect(() => mapNotesPageConflicts({
      pageId: "p",
      conflicts: [{ target: "page_title", id: "p", versions: [{ version: "a", device, editedAtMs: -1, displayed: true, text: "" }] }],
    })).toThrow();
    expect(() => mapNotesPageConflicts(null)).toThrow();
  });
});

describe("Notes conflict errors", () => {
  it("turns known native codes into typed errors", () => {
    for (const code of ["resolved", "unfit", "failed"] as const) {
      const error = toNotesConflictError({ code, message: "text" });
      expect(error).toBeInstanceOf(NotesConflictError);
      expect(error).toMatchObject({ code, message: "text" });
    }
  });

  it("passes other rejections through", () => {
    const unknownCode = { code: "would_cycle", message: "text" };
    expect(toNotesConflictError(unknownCode)).toBe(unknownCode);
    expect(toNotesConflictError({ code: "failed", message: 3 })).toEqual({ code: "failed", message: 3 });
    expect(toNotesConflictError("plain")).toBe("plain");
  });
});

describe("Notes mismatched value previews", () => {
  it("reads whether converting drops data and refuses malformed previews", () => {
    expect(mapNotesMismatchedValuePreview({ conversionLosesData: true })).toEqual({ conversionLosesData: true });
    expect(() => mapNotesMismatchedValuePreview({ conversionLosesData: "yes" })).toThrow();
    expect(() => mapNotesMismatchedValuePreview(null)).toThrow();
  });
});
