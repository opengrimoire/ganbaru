import { describe, expect, it } from "vitest";
import {
  blockWithText,
  createButtonPayload,
  createCalloutPayload,
  createChildDatabasePayload,
  createTabPayload,
  createTemplatePayload,
  createTextPayload,
  createTodoPayload,
  createTogglePayload,
} from "./factory";
import {
  blockColor,
  blockWithColor,
  canBlockHaveColor,
  notesBlockColorStyle,
} from "./color";
import type { NotesBlock, NotesColor } from "$lib/notes/types";

const now = "2026-06-30T09:00:00.000Z";

function paragraphBlock(color: NotesColor = "default"): NotesBlock {
  return {
    object: "block",
    id: "block-a",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "paragraph",
    paragraph: createTextPayload("Hello", color),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function todoBlock(color: NotesColor = "default"): NotesBlock {
  return {
    object: "block",
    id: "block-b",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "to_do",
    to_do: createTodoPayload("Check", true, color),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function toggleBlock(color: NotesColor = "default"): NotesBlock {
  return {
    object: "block",
    id: "block-c",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "toggle",
    toggle: createTogglePayload("Details", false, color),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function calloutBlock(color: NotesColor = "default"): NotesBlock {
  return {
    object: "block",
    id: "block-d",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "callout",
    callout: createCalloutPayload("Remember", color),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function toggleHeadingBlock(color: NotesColor = "default"): NotesBlock {
  return {
    object: "block",
    id: "block-e",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: true,
    in_trash: false,
    archived: false,
    type: "heading_2",
    heading_2: createTextPayload("Details", {
      color,
      isToggleable: true,
      open: false,
    }),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function childDatabaseBlock(): NotesBlock {
  return {
    object: "block",
    id: "block-f",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    type: "child_database",
    child_database: createChildDatabasePayload("Tasks"),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function templateBlock(): NotesBlock {
  return {
    object: "block",
    id: "block-g",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: true,
    in_trash: false,
    archived: false,
    type: "template",
    template: createTemplatePayload("Plan day"),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function buttonBlock(): NotesBlock {
  return {
    object: "block",
    id: "block-h",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: true,
    in_trash: false,
    archived: false,
    type: "button",
    button: createButtonPayload("Add agenda"),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

function tabBlock(): NotesBlock {
  return {
    object: "block",
    id: "block-i",
    parent: { type: "page_id", page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: true,
    in_trash: false,
    archived: false,
    type: "tab",
    tab: createTabPayload(),
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
}

describe("notes block colors", () => {
  it("detects color-capable block types", () => {
    expect(canBlockHaveColor("paragraph")).toBe(true);
    expect(canBlockHaveColor("heading_1")).toBe(true);
    expect(canBlockHaveColor("heading_4")).toBe(true);
    expect(canBlockHaveColor("to_do")).toBe(true);
    expect(canBlockHaveColor("toggle")).toBe(true);
    expect(canBlockHaveColor("callout")).toBe(true);
    expect(canBlockHaveColor("table_of_contents")).toBe(true);
    expect(canBlockHaveColor("column_list")).toBe(false);
    expect(canBlockHaveColor("column")).toBe(false);
    expect(canBlockHaveColor("table")).toBe(false);
    expect(canBlockHaveColor("table_row")).toBe(false);
    expect(canBlockHaveColor("tab")).toBe(false);
    expect(canBlockHaveColor("image")).toBe(false);
    expect(canBlockHaveColor("video")).toBe(false);
    expect(canBlockHaveColor("audio")).toBe(false);
    expect(canBlockHaveColor("file")).toBe(false);
    expect(canBlockHaveColor("pdf")).toBe(false);
    expect(canBlockHaveColor("child_page")).toBe(false);
    expect(canBlockHaveColor("child_database")).toBe(false);
    expect(canBlockHaveColor("breadcrumb")).toBe(false);
    expect(canBlockHaveColor("bookmark")).toBe(false);
    expect(canBlockHaveColor("link_preview")).toBe(false);
    expect(canBlockHaveColor("template")).toBe(false);
    expect(canBlockHaveColor("button")).toBe(false);
    expect(canBlockHaveColor("embed")).toBe(false);
    expect(canBlockHaveColor("equation")).toBe(false);
    expect(canBlockHaveColor("code")).toBe(false);
    expect(canBlockHaveColor("divider")).toBe(false);
    expect(canBlockHaveColor("unsupported")).toBe(false);
  });

  it("updates text-bearing block colors without changing rich text", () => {
    const update = blockWithColor(paragraphBlock(), "red_background");

    expect(update.type).toBe("paragraph");
    if (update.type === "paragraph") {
      expect(update.paragraph.color).toBe("red_background");
      expect(update.paragraph.rich_text[0]?.plain_text).toBe("Hello");
    }
  });

  it("preserves block color when text is rebuilt", () => {
    const update = blockWithText(toggleBlock("green_background"), "Done");

    expect(update.type).toBe("toggle");
    if (update.type === "toggle") {
      expect(update.toggle.color).toBe("green_background");
      expect(update.toggle.ganbaru_open).toBe(false);
      expect(update.toggle.rich_text[0]?.plain_text).toBe("Done");
    }
  });

  it("preserves callout icon and color when text is rebuilt", () => {
    const update = blockWithText(calloutBlock("yellow_background"), "Updated");

    expect(update.type).toBe("callout");
    if (update.type === "callout") {
      expect(update.callout.color).toBe("yellow_background");
      expect(update.callout.icon).toEqual({ type: "emoji", emoji: "💡" });
      expect(update.callout.rich_text[0]?.plain_text).toBe("Updated");
    }
  });

  it("preserves toggle heading state when text is rebuilt", () => {
    const update = blockWithText(toggleHeadingBlock("blue_background"), "Updated details");

    expect(update.type).toBe("heading_2");
    if (update.type === "heading_2") {
      expect(update.heading_2.color).toBe("blue_background");
      expect(update.heading_2.is_toggleable).toBe(true);
      expect(update.heading_2.ganbaru_open).toBe(false);
      expect(update.heading_2.rich_text[0]?.plain_text).toBe("Updated details");
    }
  });

  it("preserves to-do checked state when text is rebuilt", () => {
    const update = blockWithText(todoBlock("green_background"), "Done");

    expect(update.type).toBe("to_do");
    if (update.type === "to_do") expect(update.to_do.checked).toBe(true);
  });

  it("preserves child database payloads when color updates are requested", () => {
    const update = blockWithColor(childDatabaseBlock(), "red");

    expect(update.type).toBe("child_database");
    if (update.type === "child_database") {
      expect(update.child_database.title).toBe("Tasks");
    }
  });

  it("preserves template titles when text is rebuilt", () => {
    const update = blockWithText(templateBlock(), "Plan week");

    expect(update.type).toBe("template");
    if (update.type === "template") {
      expect(update.template.rich_text[0]?.plain_text).toBe("Plan week");
    }
  });

  it("preserves button actions when text is rebuilt", () => {
    const update = blockWithText(buttonBlock(), "Add checklist");

    expect(update.type).toBe("button");
    if (update.type === "button") {
      expect(update.button.rich_text[0]?.plain_text).toBe("Add checklist");
      expect(update.button.actions).toEqual([
        { type: "insert_blocks", source: "children", position: "below_button" },
      ]);
    }
  });

  it("preserves tab payloads when color updates are requested", () => {
    const update = blockWithColor(tabBlock(), "red");

    expect(update.type).toBe("tab");
    if (update.type === "tab") {
      expect(update.tab).toEqual({});
    }
  });

  it("returns CSS variables for rendered color state", () => {
    expect(blockColor(paragraphBlock("blue"))).toBe("blue");
    expect(notesBlockColorStyle("blue_background")).toContain("--notes-block-bg:");
  });
});
