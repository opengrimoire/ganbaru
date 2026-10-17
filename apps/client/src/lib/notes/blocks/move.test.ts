import { describe, expect, it } from "vitest";
import { notesMoveToPageTargets } from "./move";
import { createBlockWrite } from "./factory";
import type {
  NotesBlock,
  NotesBlockWrite,
  NotesPage,
  NotesParent,
  NotesRichText,
} from "$lib/notes/types";

const now = "2026-06-30T09:00:00.000Z";

function richText(text: string): NotesRichText {
  return {
    type: "text",
    text: { content: text, link: null },
    annotations: {
      bold: false,
      italic: false,
      strikethrough: false,
      underline: false,
      code: false,
      color: "default",
    },
    plain_text: text,
    href: null,
  };
}

function page(
  id: string,
  title: string,
  inTrash = false,
  parent: NotesParent = { type: "workspace", workspace: true },
): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent,
    folder_id: null,
    in_trash: inTrash,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      title: {
        id: "title",
        type: "title",
        title: title ? [richText(title)] : [],
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

function blockFromWrite(write: NotesBlockWrite, parent: NotesParent): NotesBlock {
  const base = {
    object: "block" as const,
    id: write.id,
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
  if (write.type === "child_page") return { ...base, type: write.type, child_page: write.child_page };
  if (write.type === "paragraph") return { ...base, type: write.type, paragraph: write.paragraph };
  throw new Error("fixture supports paragraph and child_page blocks only");
}

describe("notes block move targets", () => {
  it("excludes the current page and trashed pages", () => {
    const block = blockFromWrite(
      createBlockWrite("block", "paragraph", "Move me"),
      { type: "page_id", page_id: "current" },
    );

    expect(
      notesMoveToPageTargets(
        [
          page("current", "Current"),
          page("target", "Target"),
          page("trashed", "Trashed", true),
        ],
        block,
        "current",
        "Untitled",
      ),
    ).toMatchObject([{ id: "target", title: "Target", path: [], depth: 0 }]);
  });

  it("excludes a child page block's own page", () => {
    const block = blockFromWrite(
      createBlockWrite("child-page", "child_page", "Nested"),
      { type: "page_id", page_id: "current" },
    );

    expect(
      notesMoveToPageTargets(
        [
          page("child-page", "Nested"),
          page("target", "Target"),
        ],
        block,
        "current",
        "Untitled",
      ),
    ).toMatchObject([{ id: "target", title: "Target" }]);
  });

  it("excludes pages from the moved loaded subtree", () => {
    const block = blockFromWrite(
      createBlockWrite("block", "paragraph", "Move me"),
      { type: "page_id", page_id: "current" },
    );

    expect(
      notesMoveToPageTargets(
        [
          page("descendant-page", "Descendant"),
          page("target", "Target"),
        ],
        block,
        "current",
        "Untitled",
        { excludedPageIds: ["descendant-page"] },
      ).map((target) => target.id),
    ).toEqual(["target"]);
  });

  it("keeps nested page context and recent destination state", () => {
    const block = blockFromWrite(
      createBlockWrite("block", "paragraph", "Move me"),
      { type: "page_id", page_id: "current" },
    );

    const targets = notesMoveToPageTargets(
      [
        page("root", "Root"),
        page("child", "Child", false, { type: "page_id", page_id: "root" }),
      ],
      block,
      "current",
      "Untitled",
      { recentPageIds: ["child"] },
    );

    expect(targets).toMatchObject([
      { id: "root", title: "Root", path: [], depth: 0, recent: false },
      { id: "child", title: "Child", path: ["Root"], depth: 1, recent: true },
    ]);
  });

  it("uses the untitled label for blank page titles", () => {
    const block = blockFromWrite(
      createBlockWrite("block", "paragraph", "Move me"),
      { type: "page_id", page_id: "current" },
    );

    expect(
      notesMoveToPageTargets([page("target", "")], block, "current", "Untitled"),
    ).toMatchObject([{ id: "target", title: "Untitled" }]);
  });
});
