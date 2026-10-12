import { describe, expect, it } from "vitest";
import {
  createNotesLinkedDatabaseViewRequest,
  notesChildDatabaseViewScope,
} from "./linked";
import type { NotesChildDatabaseBlock } from "$lib/notes/types";

function childDatabaseBlock(
  childDatabase: NotesChildDatabaseBlock["child_database"],
): NotesChildDatabaseBlock {
  return {
    object: "block",
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    parent: {
      type: "page_id",
      page_id: "11111111-1111-4111-8111-111111111111",
    },
    created_time: "2026-07-01T12:00:00.000Z",
    last_edited_time: "2026-07-01T12:00:00.000Z",
    has_children: false,
    in_trash: false,
    type: "child_database",
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    child_database: childDatabase,
  };
}

describe("linked database views", () => {
  it("builds a request with a new linked database shell and view id", () => {
    const source = childDatabaseBlock({
      title: "Tasks",
      database_id: "80808080-8080-4080-8080-808080808080",
      data_source_id: "81818181-8181-4181-8181-818181818181",
      view_id: "82828282-8282-4282-8282-828282828282",
    });

    expect(notesChildDatabaseViewScope(source)).toEqual({
      databaseId: "80808080-8080-4080-8080-808080808080",
      viewId: "82828282-8282-4282-8282-828282828282",
    });
    expect(createNotesLinkedDatabaseViewRequest(
      source,
      {
        databaseId: "83838383-8383-4383-8383-838383838383",
        viewId: "84848484-8484-4484-8484-848484848484",
      },
      "  Task mirror  ",
    )).toEqual({
      id: "83838383-8383-4383-8383-838383838383",
      view_id: "84848484-8484-4484-8484-848484848484",
      source_block_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      title: "Task mirror",
    });
  });

  it("rejects preserved child database blocks without local view identity", () => {
    const preserved = childDatabaseBlock({ title: "Imported database" });

    expect(notesChildDatabaseViewScope(preserved)).toBeNull();
    expect(() =>
      createNotesLinkedDatabaseViewRequest(
        preserved,
        {
          databaseId: "83838383-8383-4383-8383-838383838383",
          viewId: "84848484-8484-4484-8484-848484848484",
        },
        "Imported mirror",
      ),
    ).toThrow("source block must be a local database");
  });
});
