import { describe, expect, it } from "vitest";
import {
  mergeNotesHeldEditNotice,
  NotesHeldEditError,
  notesHeldEditNoticeFromFailure,
  notesRemovedHeldEditNotice,
} from "./held-edit-notice.svelte";

describe("notes held edit notice from a failed flush", () => {
  it("names the first page that holds an edit", () => {
    const error = new NotesHeldEditError(["block-a", "block-b"], ["page-a", "page-b"]);
    expect(notesHeldEditNoticeFromFailure(error)).toEqual({ kind: "held", pageId: "page-a" });
  });

  it("still refuses with no page when the held edit has none", () => {
    expect(notesHeldEditNoticeFromFailure(new NotesHeldEditError(["block"], []))).toEqual({ kind: "held", pageId: null });
  });

  it("ignores other failures so callers report them as before", () => {
    expect(notesHeldEditNoticeFromFailure(new Error("disk full"))).toBeNull();
    expect(notesHeldEditNoticeFromFailure({ name: "NotesHeldEditError" })).toBeNull();
    expect(notesHeldEditNoticeFromFailure(undefined)).toBeNull();
  });
});

describe("notes removed held edit notice", () => {
  it("keeps every non-blank held text, separated by blank lines", () => {
    expect(notesRemovedHeldEditNotice(["First", "  ", "Second"])).toEqual({ kind: "removed", text: "First\n\nSecond" });
  });

  it("shows nothing when no held text would be lost", () => {
    expect(notesRemovedHeldEditNotice([])).toBeNull();
    expect(notesRemovedHeldEditNotice(["", " \n"])).toBeNull();
  });
});

describe("merging held edit notices", () => {
  const held = { kind: "held", pageId: "page" } as const;

  it("shows the next notice when nothing or a refusal is shown", () => {
    expect(mergeNotesHeldEditNotice(null, held)).toEqual(held);
    expect(mergeNotesHeldEditNotice({ kind: "held", pageId: "other" }, held)).toEqual(held);
  });

  it("never lets a refusal replace removed text", () => {
    const removed = { kind: "removed", text: "Kept" } as const;
    expect(mergeNotesHeldEditNotice(removed, held)).toEqual(removed);
  });

  it("appends later removed text to the text shown now", () => {
    expect(mergeNotesHeldEditNotice({ kind: "removed", text: "First" }, { kind: "removed", text: "Second" }))
      .toEqual({ kind: "removed", text: "First\n\nSecond" });
  });
});
