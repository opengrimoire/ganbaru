import { describe, expect, it } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import { notesPageTitle } from "./title";
import type { NotesPage } from "$lib/notes/types";

const now = "2026-07-03T12:00:00.000Z";

function pageWithTitle(title: string): NotesPage {
  return {
    object: "page",
    id: "page-id",
    created_time: now,
    last_edited_time: now,
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      title: {
        id: "title",
        type: "title",
        title: title ? [createRichText(title)] : [],
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

describe("notesPageTitle", () => {
  it("returns plain title text when present", () => {
    expect(notesPageTitle(pageWithTitle("Project plan"), "Untitled")).toBe("Project plan");
  });

  it("returns the fallback for display when the title is empty", () => {
    expect(notesPageTitle(pageWithTitle(""), "Untitled")).toBe("Untitled");
  });

  it("returns an empty raw title when no fallback is provided", () => {
    expect(notesPageTitle(pageWithTitle(""))).toBe("");
  });
});
