import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import {
  orderedNotesPagesById,
  parseStoredNotesPageIdList,
  recordRecentNotesPageId,
  setNotesPageFavoriteId,
} from "./navigation";
import type { NotesPage, NotesParent } from "$lib/notes/types";

const now = "2026-06-30T12:00:00.000Z";

function page(id: string, title: string, parent: NotesParent): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent,
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      title: {
        id: "title",
        type: "title",
        title: [createRichText(title)],
      },
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("notes page navigation metadata", () => {
  it("parses stored page id lists defensively", () => {
    expect(parseStoredNotesPageIdList(["a", "b", "a", 1, null])).toEqual(["a", "b"]);
    expect(parseStoredNotesPageIdList("a")).toEqual([]);
  });

  it("adds and removes favorite ids while keeping favorites unique", () => {
    expect(setNotesPageFavoriteId(["a", "b"], "c", true)).toEqual(["c", "a", "b"]);
    expect(setNotesPageFavoriteId(["a", "b"], "b", true)).toEqual(["b", "a"]);
    expect(setNotesPageFavoriteId(["a", "b"], "a", false)).toEqual(["b"]);
  });

  it("records recent ids at the front with a bounded unique list", () => {
    expect(recordRecentNotesPageId(["a", "b", "c"], "b", 3)).toEqual(["b", "a", "c"]);
    expect(recordRecentNotesPageId(["a", "b", "c"], "d", 3)).toEqual(["d", "a", "b"]);
    expect(recordRecentNotesPageId(["a"], "b", 0)).toEqual([]);
  });

  it("returns pages in stored id order and drops stale ids", () => {
    const first = page("first", "First", { type: "workspace", workspace: true });
    const second = page("second", "Second", { type: "workspace", workspace: true });

    expect(orderedNotesPagesById([first, second], ["missing", "second", "first"])).toEqual([
      second,
      first,
    ]);
  });
});
