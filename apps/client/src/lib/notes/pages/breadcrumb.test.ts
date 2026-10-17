import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import { parseNotesPageBreadcrumbItem } from "$lib/notes/blocks/validation";
import { addNotesWorkspaceBreadcrumb, buildNotesPageBreadcrumb } from "./breadcrumb";
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

describe("notes page breadcrumb", () => {
  it("builds a workspace-prefixed page ancestor chain", () => {
    const root = page("root", "Root", { type: "workspace", workspace: true });
    const child = page("child", "Child", { type: "page_id", page_id: root.id });
    const leaf = page("leaf", "Leaf", { type: "page_id", page_id: child.id });

    expect(
      buildNotesPageBreadcrumb(leaf, [child, root], "Workspace", "Untitled").map((item) => [
        item.id,
        item.title,
        item.current,
        item.status,
      ]),
    ).toEqual([
      [null, "Workspace", false, "workspace"],
      ["root", "Root", false, "active"],
      ["child", "Child", false, "active"],
      ["leaf", "Leaf", true, "active"],
    ]);
  });

  it("renders the current page even when an ancestor is not loaded", () => {
    const leaf = page("leaf", "Leaf", { type: "page_id", page_id: "missing" });

    expect(
      buildNotesPageBreadcrumb(leaf, [], "Workspace", "Untitled").map((item) => item.title),
    ).toEqual(["Workspace", "Leaf"]);
  });

  it("stops at cycles instead of repeating pages", () => {
    const first = page("first", "First", { type: "page_id", page_id: "second" });
    const second = page("second", "Second", { type: "page_id", page_id: "first" });

    expect(
      buildNotesPageBreadcrumb(first, [second], "Workspace", "Untitled").map((item) => item.id),
    ).toEqual([null, "second", "first"]);
  });

  it("prepends the workspace to backend-resolved breadcrumb rows", () => {
    expect(
      addNotesWorkspaceBreadcrumb(
        [
          { id: "archived", title: "Old", current: false, status: "archived" },
          { id: "leaf", title: "Leaf", current: true, status: "active" },
        ],
        "Workspace",
        "Untitled",
        "Missing page",
      ),
    ).toEqual([
      { id: null, title: "Workspace", current: false, status: "workspace" },
      { id: "archived", title: "Old", current: false, status: "archived" },
      { id: "leaf", title: "Leaf", current: true, status: "active" },
    ]);
  });

  it("labels backend-resolved missing and untitled breadcrumb rows", () => {
    expect(
      addNotesWorkspaceBreadcrumb(
        [
          { id: "missing", title: "", current: false, status: "missing" },
          { id: "leaf", title: "", current: true, status: "active" },
        ],
        "Workspace",
        "Untitled",
        "Missing page",
      ).map((item) => item.title),
    ).toEqual(["Workspace", "Missing page", "Untitled"]);
  });

  it("parses breadcrumb DTOs from the Tauri boundary", () => {
    expect(
      parseNotesPageBreadcrumbItem({
        id: "root",
        title: "Root",
        current: false,
        status: "archived",
      }).status,
    ).toBe("archived");
  });

  it("rejects unsupported breadcrumb statuses", () => {
    expect(() =>
      parseNotesPageBreadcrumbItem({
        id: "root",
        title: "Root",
        current: false,
        status: "deleted",
      }),
    ).toThrow("page breadcrumb.status must be workspace, active, archived, trashed, or missing");
  });
});
