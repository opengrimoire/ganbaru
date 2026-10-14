import { describe, expect, it } from "vitest";
import { NOTES_PAGE_PROJECT_ID_PROPERTY } from "$lib/notes/project-membership";
import type { NotesPage, NotesSearchResult, NotesSearchResultType } from "$lib/notes/types";
import {
  NOTES_SEARCH_QUERY_MAX_CHARS,
  notesPageSearchGroups,
  notesPageSearchQueryIsSearchable,
} from "./page-search";

function page(id: string, projectId?: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-10-13T00:00:00.000Z",
    last_edited_time: "2026-10-13T00:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: projectId ? { [NOTES_PAGE_PROJECT_ID_PROPERTY]: projectId } : {},
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function result(id: string, type: NotesSearchResultType, matchedPage: NotesPage): NotesSearchResult {
  return {
    object: "search_result",
    id,
    type,
    page: matchedPage,
    block_id: type === "block" ? `block-${id}` : null,
    block_type: null,
    comment_id: null,
    discussion_id: null,
    comment_status: null,
    comment_author: null,
    comment_anchor: null,
    snippet: "",
    last_edited_time: matchedPage.last_edited_time,
  };
}

const alpha = { id: "alpha" };
const beta = { id: "beta" };

describe("notesPageSearchGroups", () => {
  it("groups note matches by project in the given project order and keeps rank order inside a project", () => {
    const groups = notesPageSearchGroups([
      result("1", "page", page("alpha-roadmap", "alpha")),
      result("2", "page", page("beta-plan", "beta")),
      result("3", "page", page("alpha-notes", "alpha")),
    ], [beta, alpha]);

    expect(groups.map((group) => [group.project?.id, group.pages.map((entry) => entry.id)])).toEqual([
      ["beta", ["beta-plan"]],
      ["alpha", ["alpha-roadmap", "alpha-notes"]],
    ]);
  });

  it("keeps one row per note and ignores block and comment matches", () => {
    const roadmap = page("roadmap", "alpha");
    const groups = notesPageSearchGroups([
      result("1", "page", roadmap),
      result("2", "page", roadmap),
      result("3", "block", page("body-only", "alpha")),
      result("4", "comment", page("comment-only", "alpha")),
    ], [alpha]);

    expect(groups).toEqual([{ project: alpha, pages: [roadmap] }]);
  });

  it("lists notes outside any project last and leaves out notes of unlisted projects", () => {
    const loose = page("loose");
    const groups = notesPageSearchGroups([
      result("1", "page", loose),
      result("2", "page", page("hidden", "inactive")),
      result("3", "page", page("plan", "alpha")),
    ], [alpha]);

    expect(groups.map((group) => [group.project?.id ?? null, group.pages.map((entry) => entry.id)])).toEqual([
      ["alpha", ["plan"]],
      [null, ["loose"]],
    ]);
  });

  it("returns no groups without note matches", () => {
    expect(notesPageSearchGroups([], [alpha])).toEqual([]);
  });
});

describe("notesPageSearchQueryIsSearchable", () => {
  it("accepts queries with letters or digits in any script", () => {
    expect(notesPageSearchQueryIsSearchable("plan")).toBe(true);
    expect(notesPageSearchQueryIsSearchable(" 2026 ")).toBe(true);
    expect(notesPageSearchQueryIsSearchable("頑張る")).toBe(true);
  });

  it("rejects blank and punctuation-only queries", () => {
    expect(notesPageSearchQueryIsSearchable("   ")).toBe(false);
    expect(notesPageSearchQueryIsSearchable("--?")).toBe(false);
  });

  it("rejects queries longer than the index accepts", () => {
    expect(notesPageSearchQueryIsSearchable("a".repeat(NOTES_SEARCH_QUERY_MAX_CHARS))).toBe(true);
    expect(notesPageSearchQueryIsSearchable("a".repeat(NOTES_SEARCH_QUERY_MAX_CHARS + 1))).toBe(false);
  });
});
