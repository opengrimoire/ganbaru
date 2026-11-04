import { describe, expect, it } from "vitest";
import {
  filterNotesPagesByTitle,
  nextSelectedNotesPageId,
  nextSelectedNotesPageIdInProject,
  notesPageSubtreeIds,
  parseStoredNotesPageId,
  restoredNotesPageSelection,
} from "./selection";

describe("notes page selection", () => {
  it("parses stored page ids defensively", () => {
    expect(parseStoredNotesPageId(" page-a ")).toBe("page-a");
    expect(parseStoredNotesPageId(" ")).toBe(null);
    expect(parseStoredNotesPageId(1)).toBe(null);
  });

  it("selects the next nearby page after a page is removed", () => {
    const pages = [{ id: "a" }, { id: "b" }, { id: "c" }];
    expect(nextSelectedNotesPageId(pages, "b")).toBe("c");
    expect(nextSelectedNotesPageId(pages, "c")).toBe("b");
    expect(nextSelectedNotesPageId([{ id: "a" }], "a")).toBe(null);
  });

  it("replaces a removed page only with a page from the same project", () => {
    const inProject = (id: string, projectId: string | null) => ({
      id,
      project_id: projectId,
    });
    const pages = [
      inProject("social-only", "social"),
      inProject("eating-a", "eating"),
      inProject("loose", null),
      inProject("eating-b", "eating"),
    ];
    expect(nextSelectedNotesPageIdInProject(pages, "social-only", "social")).toBe(null);
    expect(nextSelectedNotesPageIdInProject(pages, "eating-a", "eating")).toBe("eating-b");
    expect(nextSelectedNotesPageIdInProject(pages, "eating-b", "eating")).toBe("eating-a");
    expect(nextSelectedNotesPageIdInProject(pages, "missing", null)).toBe("loose");
  });

  it("includes nested visible pages when a parent goes to Trash", () => {
    const pages = [
      { id: "grandchild", parent: { type: "page_id" as const, page_id: "child" } },
      { id: "other", parent: { type: "workspace" as const, workspace: true as const } },
      { id: "child", parent: { type: "page_id" as const, page_id: "root" } },
    ];
    expect([...notesPageSubtreeIds(pages, "root")]).toEqual(["root", "child", "grandchild"]);
  });

  it("restores only an explicit existing page selection", () => {
    const pages = [{ id: "a" }, { id: "b" }];
    expect(restoredNotesPageSelection(" b ", pages)).toBe("b");
    expect(restoredNotesPageSelection("missing", pages)).toBe(null);
    expect(restoredNotesPageSelection(null, pages)).toBe(null);
  });

  it("filters pages by normalized title", () => {
    const pages = [
      { id: "a", title: "Daily notes" },
      { id: "b", title: "Project index" },
    ];
    expect(filterNotesPagesByTitle(pages, " DAILY ", (page) => page.title)).toEqual([pages[0]]);
    expect(filterNotesPagesByTitle(pages, "", (page) => page.title)).toEqual(pages);
  });
});
