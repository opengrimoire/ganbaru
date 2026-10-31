import { describe, expect, it } from "vitest";
import type { NotesFolder, NotesPage } from "$lib/notes/types";
import {
  canDropNotesNavigationItem,
  parseNotesNavigationDragItem,
  resolveNotesNavigationDrop,
  serializeNotesNavigationDragItem,
} from "./drag";

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
    project_id: "project-1",
    properties: { title: id },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("Notes navigation drag", () => {
  it("round trips valid drag payloads and rejects malformed values", () => {
    expect(parseNotesNavigationDragItem(
      serializeNotesNavigationDragItem({ kind: "page", id: "page-1" }),
    )).toEqual({ kind: "page", id: "page-1" });
    expect(parseNotesNavigationDragItem('{"kind":"other","id":"page-1"}')).toBeNull();
    expect(parseNotesNavigationDragItem("not-json")).toBeNull();
  });

  it("allows notes to move into folders and notes but not themselves or descendants", () => {
    const folders = [folder("folder-1")];
    const pages = [
      page("source"),
      page("child", { type: "page_id", page_id: "source" }),
      page("target"),
    ];

    expect(canDropNotesNavigationItem(
      { kind: "page", id: "source" },
      { kind: "folder", id: "folder-1" },
      pages,
      folders,
    )).toBe(true);
    expect(canDropNotesNavigationItem(
      { kind: "page", id: "source" },
      { kind: "page", id: "target" },
      pages,
      folders,
    )).toBe(true);
    expect(canDropNotesNavigationItem(
      { kind: "page", id: "source" },
      { kind: "page", id: "child" },
      pages,
      folders,
    )).toBe(false);
    expect(canDropNotesNavigationItem(
      { kind: "page", id: "child" },
      { kind: "root" },
      pages,
      folders,
    )).toBe(true);
    expect(canDropNotesNavigationItem(
      { kind: "page", id: "source" },
      { kind: "root" },
      pages,
      folders,
    )).toBe(false);
  });

  it("allows folders to move to valid folders but not themselves or descendants", () => {
    const folders = [folder("source"), folder("child", "source"), folder("target")];

    expect(canDropNotesNavigationItem(
      { kind: "folder", id: "source" },
      { kind: "folder", id: "target" },
      [],
      folders,
    )).toBe(true);
    expect(canDropNotesNavigationItem(
      { kind: "folder", id: "source" },
      { kind: "folder", id: "child" },
      [],
      folders,
    )).toBe(false);
    expect(canDropNotesNavigationItem(
      { kind: "folder", id: "child" },
      { kind: "folder", id: "source" },
      [],
      folders,
    )).toBe(false);
    expect(canDropNotesNavigationItem(
      { kind: "folder", id: "child" },
      { kind: "root" },
      [],
      folders,
    )).toBe(true);
    expect(canDropNotesNavigationItem(
      { kind: "folder", id: "source" },
      { kind: "page", id: "page-1" },
      [page("page-1")],
      folders,
    )).toBe(false);
  });

  it("leaves a note's current location unmarked while still forbidding other projects", () => {
    const folders = [folder("folder-1"), folder("folder-2"), { ...folder("other"), project_id: "project-2" }];
    const pages = [page("loose"), page("filed", { type: "workspace", workspace: true }, "folder-1")];
    const loose = { kind: "page", id: "loose" } as const;
    const filed = { kind: "page", id: "filed" } as const;
    expect(resolveNotesNavigationDrop(loose, { kind: "root" }, pages, folders)).toBe("current");
    expect(resolveNotesNavigationDrop(loose, { kind: "folder", id: "folder-1" }, pages, folders)).toBe("move");
    expect(resolveNotesNavigationDrop(filed, { kind: "folder", id: "folder-1" }, pages, folders)).toBe("current");
    expect(resolveNotesNavigationDrop(filed, { kind: "root" }, pages, folders)).toBe("move");
    expect(resolveNotesNavigationDrop(filed, { kind: "folder", id: "other" }, pages, folders)).toBe("forbidden");
  });

  it("treats a folder's own row and parent as current but its descendants as forbidden", () => {
    const folders = [folder("parent"), folder("child", "parent"), folder("grandchild", "child"), folder("sibling")];
    const child = { kind: "folder", id: "child" } as const;
    expect(resolveNotesNavigationDrop(child, { kind: "folder", id: "child" }, [], folders)).toBe("current");
    expect(resolveNotesNavigationDrop(child, { kind: "folder", id: "parent" }, [], folders)).toBe("current");
    expect(resolveNotesNavigationDrop(child, { kind: "folder", id: "grandchild" }, [], folders)).toBe("forbidden");
    expect(resolveNotesNavigationDrop(child, { kind: "folder", id: "sibling" }, [], folders)).toBe("move");
    expect(resolveNotesNavigationDrop(child, { kind: "root" }, [], folders)).toBe("move");
    expect(resolveNotesNavigationDrop({ kind: "folder", id: "parent" }, { kind: "root" }, [], folders)).toBe("current");
  });
});
