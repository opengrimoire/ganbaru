import { describe, expect, it } from "vitest";
import {
  NOTES_PAGE_PROJECT_ID_PROPERTY,
  mergeNotesNavigationPages,
  notesPageMatchesProject,
  notesPageProjectId,
  notesPageProjectProperties,
  notesPagesForProject,
} from "./project-membership";
import type { NotesPage } from "./types";

function page(id: string, projectId?: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-03T00:00:00.000Z",
    last_edited_time: "2026-07-03T00:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: projectId
      ? { [NOTES_PAGE_PROJECT_ID_PROPERTY]: projectId }
      : {},
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("notes project membership", () => {
  it("reads trimmed project ids from page properties", () => {
    expect(notesPageProjectId(page("page-a", " project-a "))).toBe("project-a");
    expect(notesPageProjectId(page("page-b"))).toBeNull();
  });

  it("filters pages by the selected project", () => {
    const projectPages = notesPagesForProject([
      page("page-a", "project-a"),
      page("page-b", "project-b"),
      page("page-c"),
    ], "project-a");

    expect(projectPages.map((item) => item.id)).toEqual(["page-a"]);
  });

  it("leaves page lists unfiltered without a project", () => {
    const pages = [page("page-a", "project-a"), page("page-b")];

    expect(notesPagesForProject(pages, null).map((item) => item.id)).toEqual(["page-a", "page-b"]);
    expect(notesPageMatchesProject(pages[1], null)).toBe(true);
  });

  it("keeps current workspace records ahead of cached destination records", () => {
    const current = {
      ...page("page-a", "project-a"),
      folder_id: "new-folder",
      last_edited_time: "2026-07-04T00:00:00.000Z",
    };
    const stale = { ...current, folder_id: "old-folder", last_edited_time: "2026-07-03T00:00:00.000Z" };
    const destinationOnly = page("page-b", "project-a");

    expect(mergeNotesNavigationPages([current], [stale, destinationOnly])).toEqual([
      current,
      destinationOnly,
    ]);
  });

  it("builds create properties only when a project is selected", () => {
    expect(notesPageProjectProperties(" project-a ")).toEqual({
      [NOTES_PAGE_PROJECT_ID_PROPERTY]: "project-a",
    });
    expect(notesPageProjectProperties(null)).toBeUndefined();
  });
});
