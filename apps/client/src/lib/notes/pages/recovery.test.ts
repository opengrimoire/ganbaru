import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import { notesPageRestoresToWorkspace } from "./recovery";
import type { NotesPage, NotesParent } from "$lib/notes/types";

const now = "2026-07-01T12:00:00.000Z";

function page(id: string, title: string, parent: NotesParent): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent,
    folder_id: null,
    in_trash: true,
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

describe("notes page recovery helpers", () => {
  it("reports workspace restore when the parent is also in Trash", () => {
    const parent = page("parent", "Parent", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: "parent" });

    expect(notesPageRestoresToWorkspace(child, [parent, child])).toBe(true);
  });

  it("keeps original destination messaging when the parent is not in Trash", () => {
    const child = page("child", "Child", { type: "page_id", page_id: "parent" });
    const workspacePage = page("workspace", "Workspace", { type: "workspace", workspace: true });

    expect(notesPageRestoresToWorkspace(child, [child])).toBe(false);
    expect(notesPageRestoresToWorkspace(workspacePage, [workspacePage])).toBe(false);
  });
});
