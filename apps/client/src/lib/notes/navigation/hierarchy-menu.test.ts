import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesDatabaseView, NotesFolder, NotesNavigationDatabase, NotesPage } from "$lib/notes/types";
import {
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
