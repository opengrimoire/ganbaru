import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import {
  buildNotesNavigationTree,
  notesFolderMoveTargets,
  notesFoldersForProject,
  notesPageFolderMoveTargets,
  parseStoredNotesSidebarCollapsedFolderIds,
} from "./tree";
import { NOTES_PAGE_PROJECT_ID_PROPERTY } from "$lib/notes/project-membership";
import type { NotesFolder, NotesPage, NotesParent } from "$lib/notes/types";

const now = "2026-07-10T12:00:00.000Z";

function folder(
  id: string,
  name: string,
  parentFolderId: string | null = null,
  projectId = "project-a",
): NotesFolder {
  return {
    object: "folder",
    id,
    project_id: projectId,
    parent_folder_id: parentFolderId,
    name,
    created_time: now,
    last_edited_time: now,
  };
}

function page(
  id: string,
  title: string,
  parent: NotesParent = { type: "workspace", workspace: true },
  folderId: string | null = null,
  projectId = "project-a",
): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent,
    folder_id: folderId,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      [NOTES_PAGE_PROJECT_ID_PROPERTY]: projectId,
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

describe("notes navigation tree", () => {
  it("keeps child notes out of sidebar folders even after loading or selecting them", () => {
    const pages = [page("root", "Root", undefined, "folder"),
      page("child", "Child", { type: "page_id", page_id: "root" }),
      page("nested", "Nested", { type: "block_id", block_id: "block" })];
    const folders = [folder("folder", "Folder")];
    const sidebar = buildNotesNavigationTree(pages, folders, {
      rootPagesOnly: true, activePageId: "child", expandedPageIds: ["root"], pageIdsWithChildren: ["root"],
    });
    expect(sidebar.map((item) => [item.key, item.depth, item.hasChildren])).toEqual([
      ["folder:folder", 0, true], ["page:root", 1, false],
    ]);
    expect(buildNotesNavigationTree(pages, folders).some((item) => item.key === "page:child")).toBe(true);
  });

  it("combines nested folders with folder-owned and nested pages", () => {
    const pages = [
      page("root-note", "Root note", undefined, "research"),
      page("child-note", "Child note", { type: "page_id", page_id: "root-note" }),
      page("loose-note", "Loose note"),
    ];
    const folders = [
      folder("research", "Research"),
      folder("sources", "Sources", "research"),
    ];

    expect(
      buildNotesNavigationTree(pages, folders).map((item) => [item.kind, item.key, item.depth]),
    ).toEqual([
      ["folder", "folder:research", 0],
      ["folder", "folder:sources", 1],
      ["page", "page:root-note", 1],
      ["page", "page:child-note", 2],
      ["page", "page:loose-note", 0],
    ]);
  });

  it("sorts folders and notes within each hierarchy level", () => {
    const early = "2026-07-08T12:00:00.000Z";
    const middle = "2026-07-09T12:00:00.000Z";
    const late = "2026-07-10T12:00:00.000Z";
    const pages = [
      { ...page("page-z", "Zulu"), created_time: early, last_edited_time: late },
      { ...page("page-a", "Alpha"), created_time: late, last_edited_time: middle },
    ];
    const folders = [
      { ...folder("folder-z", "Zoo"), created_time: middle, last_edited_time: early },
      { ...folder("folder-a", "Archive"), created_time: late, last_edited_time: late },
    ];
    const keysFor = (sortOrder: NonNullable<Parameters<typeof buildNotesNavigationTree>[2]>["sortOrder"]): string[] =>
      buildNotesNavigationTree(pages, folders, { sortOrder }).map((item) => item.key);

    expect(keysFor("name-asc")).toEqual([
      "folder:folder-a", "folder:folder-z", "page:page-a", "page:page-z",
    ]);
    expect(keysFor("name-desc")).toEqual([
      "folder:folder-z", "folder:folder-a", "page:page-z", "page:page-a",
    ]);
    expect(keysFor("modified-desc")).toEqual([
      "folder:folder-a", "folder:folder-z", "page:page-z", "page:page-a",
    ]);
    expect(keysFor("modified-asc")).toEqual([
      "folder:folder-z", "folder:folder-a", "page:page-a", "page:page-z",
    ]);
    expect(keysFor("created-desc")).toEqual([
      "folder:folder-a", "folder:folder-z", "page:page-a", "page:page-z",
    ]);
    expect(keysFor("created-asc")).toEqual([
      "folder:folder-z", "folder:folder-a", "page:page-z", "page:page-a",
    ]);
  });

  it("keeps selected notes open while allowing their folder to close", () => {
    const pages = [
      page("root-note", "Root note", undefined, "research"),
      page("child-note", "Child note", { type: "page_id", page_id: "root-note" }),
    ];
    const folders = [folder("research", "Research")];

    expect(
      buildNotesNavigationTree(pages, folders, {
        collapsedFolderIds: ["research"],
        expandedPageIds: [],
      }).map((item) => item.key),
    ).toEqual(["folder:research"]);
    expect(
      buildNotesNavigationTree(pages, folders, {
        collapsedFolderIds: ["research"],
        expandedPageIds: [],
        activePageId: "child-note",
      }).map((item) => item.key),
    ).toEqual(["folder:research"]);
    expect(
      buildNotesNavigationTree(pages, folders, {
        expandedPageIds: [],
        activePageId: "child-note",
      }).map((item) => item.key),
    ).toEqual(["folder:research", "page:root-note", "page:child-note"]);
  });

  it("opens empty folders by default and preserves their closed state", () => {
    const folders = [folder("empty", "Empty")];

    expect(buildNotesNavigationTree([], folders)[0]).toMatchObject({
      kind: "folder",
      collapsed: false,
      hasChildren: false,
    });
    expect(buildNotesNavigationTree([], folders, {
      collapsedFolderIds: ["empty"],
    })[0]).toMatchObject({
      kind: "folder",
      collapsed: true,
      hasChildren: false,
    });
  });

  it("shows matching pages with their folder and page ancestors", () => {
    const pages = [
      page("root-note", "Root note", undefined, "research"),
      page("child-note", "Launch checklist", { type: "page_id", page_id: "root-note" }),
      page("other", "Daily"),
    ];

    const result = buildNotesNavigationTree(pages, [folder("research", "Research")], {
      query: "launch",
    });

    expect(result.map((item) => [item.key, item.matchesQuery, item.descendantMatchesQuery]))
      .toEqual([
        ["folder:research", false, true],
        ["page:root-note", false, true],
        ["page:child-note", true, false],
      ]);
  });

  it("promotes missing parents and cycles to safe roots", () => {
    const pages = [
      page("missing-folder", "Missing folder", undefined, "absent"),
      page("missing-page", "Missing page", { type: "page_id", page_id: "absent-page" }),
      page("page-a", "Page A", { type: "page_id", page_id: "page-b" }),
      page("page-b", "Page B", { type: "page_id", page_id: "page-a" }),
    ];
    const folders = [
      folder("folder-a", "Folder A", "folder-b"),
      folder("folder-b", "Folder B", "folder-a"),
      folder("orphan", "Orphan", "absent-folder"),
    ];

    const result = buildNotesNavigationTree(pages, folders);

    expect(result.map((item) => [item.key, item.depth])).toEqual([
      ["folder:folder-a", 0],
      ["folder:folder-b", 0],
      ["folder:orphan", 0],
      ["page:missing-folder", 0],
      ["page:missing-page", 0],
      ["page:page-a", 0],
      ["page:page-b", 0],
    ]);
    expect(result.find((item) => item.key === "page:missing-page"))
      .toMatchObject({ kind: "page", parentStatus: "missing" });
  });

  it("filters folders by normalized project id", () => {
    const folders = [
      folder("a", "A", null, "project-a"),
      folder("b", "B", null, "project-b"),
    ];

    expect(notesFoldersForProject(folders, " project-a ").map((item) => item.id)).toEqual(["a"]);
    expect(notesFoldersForProject(folders, null).map((item) => item.id)).toEqual(["a", "b"]);
  });

  it("builds folder and page destinations without page descendants", () => {
    const pages = [
      page("source", "Source", undefined, "current"),
      page("descendant", "Descendant", { type: "page_id", page_id: "source" }),
      page("target", "Target"),
      page("other-project", "Other project", undefined, null, "project-b"),
    ];
    const folders = [
      folder("current", "Current"),
      folder("destination", "Destination"),
      folder("other-project-folder", "Other project", null, "project-b"),
    ];

    const targets = notesPageFolderMoveTargets(
      pages,
      folders,
      "source",
      "Project notes",
      undefined,
      ["target"],
    );

    expect(targets.map((target) => [target.key, target.kind, target.recent])).toEqual([
      ["workspace", "workspace", false],
      ["folder:destination", "folder", false],
      ["page:target", "page", true],
    ]);
  });

  it("builds folder move destinations without self, descendants, or current parent", () => {
    const folders = [
      folder("root", "Root"),
      folder("source", "Source", "root"),
      folder("descendant", "Descendant", "source"),
      folder("target", "Target"),
      folder("other-project", "Other project", null, "project-b"),
    ];

    expect(notesFolderMoveTargets(folders, "source", "Project notes").map((target) => target.key))
      .toEqual(["workspace", "folder:target"]);
  });

  it("parses stored collapsed folder ids defensively", () => {
    expect(parseStoredNotesSidebarCollapsedFolderIds(["a", "b", "a", 1])).toEqual(["a", "b"]);
    expect(parseStoredNotesSidebarCollapsedFolderIds("a")).toEqual([]);
  });
});
