import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesDatabaseView, NotesFolder, NotesNavigationDatabase, NotesPage } from "$lib/notes/types";
import {
  notesHierarchyArchivedChildren,
  notesHierarchyChildren,
  notesHierarchyNodeParent,
  notesHierarchyPath,
  notesHierarchyPickerChildren,
  notesPageContainingFolderId,
} from "./hierarchy-menu";

function folder(id: string, parentFolderId: string | null = null): NotesFolder {
  return {
    object: "folder",
    id,
    project_id: "project-1",
    parent_folder_id: parentFolderId,
    name: id,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
  };
}

function page(
  id: string,
  parent: NotesPage["parent"] = { type: "workspace", workspace: true },
  folderId: string | null = null,
): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    parent,
    folder_id: folderId,
    in_trash: false,
    icon: null,
    cover: null,
    properties: { title: { id: "title", type: "title", title: [createRichText(id)] } },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("notes hierarchy navigation", () => {
  it("branches notes containing databases while keeping databases out of folder and project roots", () => {
    const notes = [page("parent"), page("child", { type: "page_id", page_id: "parent" })];
    const databases: NotesNavigationDatabase[] = [
      { id: "database", page_id: "parent", title: "Planning", data_source_id: "source" },
      { id: "other", page_id: "unrelated", title: "Other", data_source_id: "source" },
    ];
    const root = notesHierarchyPickerChildren(notes, [], databases, { kind: "root" }, "Untitled");
    expect(root).toMatchObject([{ key: "page:parent", hasChildren: true }]);
    const children = notesHierarchyPickerChildren(notes, [], databases, { kind: "page", id: "parent" }, "Untitled");
    expect(children.map((item) => item.key)).toEqual(["page:child", "database:database"]);
    expect(notesHierarchyPickerChildren([page("parent")], [], databases, { kind: "root" }, "Untitled"))
      .toMatchObject([{ key: "page:parent", hasChildren: true }]);
  });

  it("keeps saved view order and limits preselection to supported views belonging to the chosen shell", () => {
    const database: NotesNavigationDatabase = { id: "database", page_id: "page", title: "", data_source_id: "source" };
    const view = (id: string, shell = database.id, type: NotesDatabaseView["type"] = "table"): NotesDatabaseView => ({
      object: "view", id, parent: { type: "database_id", database_id: shell }, data_source_id: "source",
      name: id, type, filter: {}, sorts: [], url: null, configuration: null,
      created_time: "2026-09-30T00:00:00Z", last_edited_time: "2026-09-30T00:00:00Z",
      source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
    });
    const children = notesHierarchyPickerChildren([], [], [database], { kind: "database", id: database.id }, "Untitled", [], [
      view("Second"), view("Unrelated", "other"), view("Unavailable", database.id, "form"), view("First"),
    ]);
    expect(children.map((item) => item.key)).toEqual(["view:Second", "view:First"]);
    expect(children.every((item) => !item.hasChildren)).toBe(true);
  });
  it("returns only direct children at each folder and page level", () => {
    const folders = [folder("root-folder"), folder("child-folder", "root-folder")];
    const pages = [
      page("root-note"),
      page("folder-note", { type: "workspace", workspace: true }, "root-folder"),
      page("child-note", { type: "page_id", page_id: "folder-note" }),
    ];

    expect(notesHierarchyChildren(pages, folders, { kind: "root" }).map((item) => item.key))
      .toEqual(["folder:root-folder", "page:root-note"]);
    expect(notesHierarchyChildren(pages, folders, { kind: "folder", id: "root-folder" }).map((item) => item.key))
      .toEqual(["folder:child-folder", "page:folder-note"]);
    expect(notesHierarchyChildren(pages, folders, { kind: "page", id: "folder-note" }).map((item) => item.key))
      .toEqual(["page:child-note"]);
  });

  it("orders each level like the sidebar: folders first, favorites first, then the chosen order", () => {
    const folders = [folder("Alpha"), folder("Beta")];
    const older = { ...page("Older"), created_time: "2026-01-01T00:00:00Z" };
    const newer = { ...page("Newer"), created_time: "2026-03-01T00:00:00Z" };
    const starred = { ...page("Starred"), created_time: "2025-01-01T00:00:00Z" };
    const order = { sortOrder: "created-desc" as const, favoriteFolderIds: ["Beta"], favoritePageIds: ["Starred"] };

    expect(notesHierarchyChildren([older, newer, starred], folders, { kind: "root" }, "Untitled", [], order)
      .map((item) => item.key))
      .toEqual(["folder:Beta", "folder:Alpha", "page:Starred", "page:Newer", "page:Older"]);
    expect(notesHierarchyChildren([older, newer, starred], folders, { kind: "root" }).map((item) => item.key))
      .toEqual(["folder:Alpha", "folder:Beta", "page:Newer", "page:Older", "page:Starred"]);
  });

  it("ranks each level by the custom order, with unranked items after by name", () => {
    const folders = [folder("Alpha"), folder("Beta")];
    const pages = [page("One"), page("Two"), page("Three"), page("Starred")];
    const order = {
      sortOrder: "custom" as const,
      favoriteFolderIds: [],
      favoritePageIds: ["Starred"],
      customOrderKeys: ["page:Three", "folder:Beta", "page:Starred", "page:One"],
    };

    expect(notesHierarchyChildren(pages, folders, { kind: "root" }, "Untitled", [], order).map((item) => item.key))
      .toEqual(["folder:Beta", "folder:Alpha", "page:Starred", "page:Three", "page:One", "page:Two"]);
  });

  it("lists a note's databases after its child notes", () => {
    const notes = [page("parent"), page("Zed", { type: "page_id", page_id: "parent" })];
    const databases: NotesNavigationDatabase[] = [
      { id: "tasks", page_id: "parent", title: "Tasks", data_source_id: "source" },
      { id: "archive", page_id: "parent", title: "Archive", data_source_id: "source" },
    ];
    expect(notesHierarchyPickerChildren(notes, [], databases, { kind: "page", id: "parent" }, "Untitled")
      .map((item) => item.key))
      .toEqual(["page:Zed", "database:archive", "database:tasks"]);
  });

  it("keeps unloaded note branches available from shell metadata", () => {
    const rootPage = page("root-note");

    expect(notesHierarchyChildren(
      [rootPage],
      [],
      { kind: "root" },
      "Untitled",
      [rootPage.id],
    )).toMatchObject([{ key: "page:root-note", hasChildren: true }]);
  });

  it("keeps empty folders openable for their creation panel", () => {
    expect(notesHierarchyChildren([], [folder("empty")], { kind: "root" }))
      .toMatchObject([{ key: "folder:empty", hasChildren: true }]);
  });

  it("builds the full folder and note path without following cycles", () => {
    const folders = [folder("root-folder"), folder("child-folder", "root-folder")];
    const pages = [
      page("parent-note", { type: "workspace", workspace: true }, "child-folder"),
      page("selected-note", { type: "page_id", page_id: "parent-note" }),
    ];

    expect(notesHierarchyPath("selected-note", pages, folders).map((item) => item.key)).toEqual([
      "folder:root-folder",
      "folder:child-folder",
      "page:parent-note",
      "page:selected-note",
    ]);
  });

  it("resolves the sibling level containing a folder or note", () => {
    const parentFolder = folder("parent-folder");
    const childFolder = folder("child-folder", parentFolder.id);
    const parentPage = page(
      "parent-note",
      { type: "workspace", workspace: true },
      parentFolder.id,
    );
    const childPage = page("child-note", { type: "page_id", page_id: parentPage.id });
    const nodes = notesHierarchyChildren(
      [parentPage, childPage],
      [parentFolder, childFolder],
      { kind: "folder", id: parentFolder.id },
    );

    expect(notesHierarchyNodeParent(nodes[0]!)).toEqual({
      kind: "folder",
      id: parentFolder.id,
    });
    expect(notesHierarchyNodeParent(notesHierarchyPath(
      childPage.id,
      [parentPage, childPage],
      [parentFolder],
    ).at(-1)!)).toEqual({ kind: "page", id: parentPage.id });
  });

  it("finds the folder containing a nested note", () => {
    const parentPage = page(
      "parent-note",
      { type: "workspace", workspace: true },
      "folder-1",
    );
    const childPage = page("child-note", { type: "page_id", page_id: parentPage.id });

    expect(notesPageContainingFolderId(childPage.id, [parentPage, childPage]))
      .toBe("folder-1");
    expect(notesPageContainingFolderId("missing", [parentPage, childPage])).toBeNull();
  });
});

describe("archived notes in the hierarchy pickers", () => {
  function archived(id: string, parent?: NotesPage["parent"], folderId: string | null = null): NotesPage {
    return { ...page(id, parent, folderId), archived: true };
  }

  it("places archived notes under their own level in the level order, without further levels", () => {
    const pages = [
      archived("zeta"),
      archived("alpha"),
      archived("in-folder", undefined, "folder-a"),
      archived("child", { type: "page_id", page_id: "parent" }),
    ];

    expect(notesHierarchyArchivedChildren(pages, new Set(), { kind: "root" }, "Untitled").map((item) => item.key))
      .toEqual(["page:alpha", "page:zeta"]);
    expect(notesHierarchyArchivedChildren(pages, new Set(), { kind: "folder", id: "folder-a" }, "Untitled"))
      .toMatchObject([{ key: "page:in-folder", hasChildren: false }]);
    expect(notesHierarchyArchivedChildren(pages, new Set(), { kind: "page", id: "parent" }, "Untitled").map((item) => item.key))
      .toEqual(["page:child"]);
  });

  it("leaves out notes that are active again, trashed, or under a database level", () => {
    const pages = [archived("restored"), { ...archived("trashed"), in_trash: true }, archived("kept")];

    expect(notesHierarchyArchivedChildren(pages, new Set(["restored"]), { kind: "root" }, "Untitled").map((item) => item.key))
      .toEqual(["page:kept"]);
    expect(notesHierarchyArchivedChildren(pages, new Set(), { kind: "database", id: "database" }, "Untitled")).toEqual([]);
  });

  it("follows the custom order of the level", () => {
    const pages = [archived("first"), archived("second")];
    const order = { sortOrder: "custom" as const, favoriteFolderIds: [], favoritePageIds: [], customOrderKeys: ["page:second", "page:first"] };

    expect(notesHierarchyArchivedChildren(pages, new Set(), { kind: "root" }, "Untitled", order).map((item) => item.key))
      .toEqual(["page:second", "page:first"]);
  });
});
