import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import { notesPageMoveTargets } from "./move";
import type { NotesPage, NotesParent } from "$lib/notes/types";

function page(id: string, title: string, parent: NotesParent): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-06-30T12:00:00.000Z",
    last_edited_time: "2026-06-30T12:00:00.000Z",
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

describe("notes page move targets", () => {
  it("excludes the source page and descendants", () => {
    const pages = [
      page("root-a", "Root A", { type: "workspace", workspace: true }),
      page("child-a", "Child A", { type: "page_id", page_id: "root-a" }),
      page("grandchild-a", "Grandchild A", { type: "page_id", page_id: "child-a" }),
      page("root-b", "Root B", { type: "workspace", workspace: true }),
    ];

    const targets = notesPageMoveTargets(pages, "root-a", "Workspace");

    expect(targets.map((target) => target.title)).toEqual(["Root B"]);
  });

  it("offers workspace when moving a nested page", () => {
    const pages = [
      page("root-a", "Root A", { type: "workspace", workspace: true }),
      page("child-a", "Child A", { type: "page_id", page_id: "root-a" }),
      page("root-b", "Root B", { type: "workspace", workspace: true }),
    ];

    const targets = notesPageMoveTargets(pages, "child-a", "Workspace");

    expect(targets.map((target) => target.parent)).toEqual([
      { type: "workspace", workspace: true },
      { type: "page_id", page_id: "root-b" },
    ]);
  });

  it("keeps nested page context and recent destination state", () => {
    const pages = [
      page("root-a", "Root A", { type: "workspace", workspace: true }),
      page("child-a", "Child A", { type: "page_id", page_id: "root-a" }),
      page("root-b", "Root B", { type: "workspace", workspace: true }),
    ];

    const targets = notesPageMoveTargets(
      pages,
      "root-b",
      "Workspace",
      undefined,
      ["child-a"],
    );

    expect(targets).toMatchObject([
      {
        pageId: "root-a",
        title: "Root A",
        path: [],
        depth: 0,
        recent: false,
      },
      {
        pageId: "child-a",
        title: "Child A",
        path: ["Root A"],
        depth: 1,
        recent: true,
      },
    ]);
  });
});
