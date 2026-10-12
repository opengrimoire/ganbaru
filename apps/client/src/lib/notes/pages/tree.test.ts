import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import {
  buildNotesPageTree,
} from "./tree";
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

describe("notes page tree", () => {
  it("builds a visible nested sidebar tree", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: root.id });
    const leaf = page("leaf", "Leaf", { type: "page_id", page_id: child.id });

    expect(buildNotesPageTree([root, child, leaf]).map((item) => [item.page.id, item.depth])).toEqual([
      ["root", 0],
      ["child", 1],
      ["leaf", 2],
    ]);
  });

  it("hides descendants of collapsed pages", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: root.id });

    expect(
      buildNotesPageTree([root, child], { collapsedPageIds: [root.id] }).map((item) => [
        item.page.id,
        item.collapsed,
      ]),
    ).toEqual([["root", true]]);
  });

  it("uses expanded ids and child markers for lazy sidebar trees", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: root.id });

    expect(
      buildNotesPageTree([root], {
        expandedPageIds: [],
        pageIdsWithChildren: [root.id],
      }).map((item) => [item.page.id, item.hasChildren, item.collapsed]),
    ).toEqual([["root", true, true]]);

    expect(
      buildNotesPageTree([root, child], {
        expandedPageIds: [root.id],
        pageIdsWithChildren: [root.id],
      }).map((item) => [item.page.id, item.depth, item.collapsed]),
    ).toEqual([
      ["root", 0, false],
      ["child", 1, false],
    ]);
  });

  it("reveals the selected page through collapsed ancestors", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: root.id });

    expect(
      buildNotesPageTree([root, child], {
        activePageId: child.id,
        collapsedPageIds: [root.id],
      }).map((item) => item.page.id),
    ).toEqual(["root", "child"]);
  });

  it("shows matching pages with ancestors while searching", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Project notes", { type: "page_id", page_id: root.id });
    const sibling = page("sibling", "Daily", { type: "workspace", workspace: true });

    const items = buildNotesPageTree([root, child, sibling], {
      collapsedPageIds: [root.id],
      query: "project",
    });

    expect(items.map((item) => [item.page.id, item.matchesQuery, item.descendantMatchesQuery]))
      .toEqual([
        ["root", false, true],
        ["child", true, false],
      ]);
  });

  it("promotes missing parents and parent cycles to roots", () => {
    const orphan = page("orphan", "Orphan", { type: "page_id", page_id: "missing" });
    const first = page("first", "First", { type: "page_id", page_id: "second" });
    const second = page("second", "Second", { type: "page_id", page_id: "first" });

    expect(buildNotesPageTree([orphan, first, second]).map((item) => item.page.id)).toEqual([
      "orphan",
      "first",
      "second",
    ]);
  });

  it("marks missing and trashed parent states", () => {
    const missingChild = page("missing-child", "Missing child", {
      type: "page_id",
      page_id: "missing",
    });
    const trashedChild = page("trashed-child", "Trashed child", {
      type: "page_id",
      page_id: "trashed",
    });

    expect(
      buildNotesPageTree([missingChild, trashedChild], {
        missingParentPageIds: ["missing"],
        trashedParentPageIds: ["trashed"],
      }).map((item) => [item.page.id, item.parentStatus]),
    ).toEqual([
      ["missing-child", "missing"],
      ["trashed-child", "trashed"],
    ]);
  });

});
