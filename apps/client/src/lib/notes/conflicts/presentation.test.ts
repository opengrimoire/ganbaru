import { describe, expect, it } from "vitest";
import { NOTES_CONFLICT_TARGETS, type NotesConflictTarget, type NotesFieldConflict } from "$lib/api/notes/conflicts";
import type { NotesBlock, NotesBlockUpdate } from "$lib/notes/types";
import {
  notesBlockEditConflictView,
  notesBlockUpdateText,
  notesFieldConflictTitleKey,
  notesPropertyNameById,
  notesThreadStatusIsResolved,
  orderNotesFieldConflicts,
  previewNotesConflictText,
} from "./presentation";

function text(content: string) {
  return { type: "text", plain_text: content, text: { content, link: null }, annotations: {}, href: null };
}

function paragraph(content: string): NotesBlockUpdate {
  return { type: "paragraph", paragraph: { rich_text: [text(content)], color: "default" } } as unknown as NotesBlockUpdate;
}

function canonical(content: string): NotesBlock {
  return { id: "block", ...paragraph(content) } as unknown as NotesBlock;
}

describe("notes block conflict text", () => {
  it("joins rich text runs into the block text", () => {
    const update = {
      type: "paragraph",
      paragraph: { rich_text: [text("Hello "), text("world")], color: "default" },
    } as unknown as NotesBlockUpdate;
    expect(notesBlockUpdateText(update)).toBe("Hello world");
  });

  it("separates table cells with tabs and ignores attributes", () => {
    const row = {
      type: "table_row",
      table_row: { cells: [[text("a")], [text("b"), text("c")]], color: "red" },
    } as unknown as NotesBlockUpdate;
    expect(notesBlockUpdateText(row)).toBe("a\tbc");
  });

  it("puts a caption on its own line after the main text", () => {
    const code = {
      type: "code",
      code: { rich_text: [text("let x = 1;")], caption: [text("Example")], language: "rust" },
    } as unknown as NotesBlockUpdate;
    expect(notesBlockUpdateText(code)).toBe("let x = 1;\nExample");
  });

  it("returns empty text for a block without a text group", () => {
    const divider = { type: "divider", divider: {} } as unknown as NotesBlockUpdate;
    expect(notesBlockUpdateText(divider)).toBe("");
  });

  it("cuts long text at the preview limit by characters, not code units", () => {
    expect(previewNotesConflictText("short", 10)).toBe("short");
    expect(previewNotesConflictText("😀😀😀😀", 2)).toBe("😀😀…");
  });
});

describe("notes block edit conflict view", () => {
  it("offers mine, theirs, and keep both for a block edited on both sides", () => {
    const view = notesBlockEditConflictView({
      blockId: "block",
      pageId: "page",
      kind: "text",
      mine: paragraph("Mine"),
      theirs: canonical("Theirs"),
    });
    expect(view).toEqual({
      blockId: "block",
      removed: false,
      mine: "Mine",
      theirs: "Theirs",
      choices: ["mine", "theirs", "both"],
    });
  });

  it("offers restoring or discarding for a block removed elsewhere, without keep both", () => {
    const view = notesBlockEditConflictView({ blockId: "block", pageId: "page", kind: "removed", mine: paragraph("Mine"), theirs: null });
    expect(view.removed).toBe(true);
    expect(view.theirs).toBeNull();
    expect(view.choices).toEqual(["mine", "theirs"]);
  });

  it("treats a text conflict whose canonical block is gone as removed", () => {
    const view = notesBlockEditConflictView({ blockId: "block", pageId: "page", kind: "text", mine: paragraph("Mine"), theirs: null });
    expect(view.removed).toBe(true);
    expect(view.choices).not.toContain("both");
  });
});

const device = { deviceId: "device-a", deviceLabel: "Phone", ownDevice: false };

function fieldConflict(target: NotesConflictTarget, id: string, displayedFirst = true): NotesFieldConflict {
  const shown = { version: `${id}-shown`, device, editedAtMs: 1, displayed: true, text: "Shown" };
  const other = { version: `${id}-other`, device, editedAtMs: 2, displayed: false, text: "Other" };
  return { target, id, versions: displayedFirst ? [shown, other] : [other, shown] };
}

describe("surfaced Notes conflicts", () => {
  it("names every target except a property value, whose card names the property", () => {
    for (const target of NOTES_CONFLICT_TARGETS) {
      const key = notesFieldConflictTitleKey(target);
      if (target === "property_value") expect(key).toBeNull();
      else expect(key).toMatch(/^notes\.conflict\./);
    }
  });

  it("orders cards as the page shows its fields and blocks in page order", () => {
    const ordered = orderNotesFieldConflicts([
      fieldConflict("comment_body", "comment"),
      fieldConflict("block_text", "block-b"),
      fieldConflict("thread_status", "thread"),
      fieldConflict("block_text", "unknown-block"),
      fieldConflict("block_text", "block-a"),
      fieldConflict("property_value", "score"),
      fieldConflict("page_title", "page"),
    ], ["block-a", "block-b"]);
    expect(ordered.map((conflict) => conflict.id)).toEqual([
      "page",
      "score",
      "block-a",
      "block-b",
      "unknown-block",
      "thread",
      "comment",
    ]);
  });

  it("leads each card with the displayed version without changing the input", () => {
    const input = [fieldConflict("page_title", "page", false)];
    const [ordered] = orderNotesFieldConflicts(input);
    expect(ordered?.versions.map((version) => version.displayed)).toEqual([true, false]);
    expect(input[0]?.versions[0]?.displayed).toBe(false);
  });

  it("finds a property name by its id", () => {
    const properties = { Score: { id: "score", type: "number" }, Done: { id: "done" }, Broken: "text" };
    expect(notesPropertyNameById(properties, "done")).toBe("Done");
    expect(notesPropertyNameById(properties, "missing")).toBeNull();
  });

  it("reads only the resolved status as resolved", () => {
    expect(notesThreadStatusIsResolved("resolved")).toBe(true);
    expect(notesThreadStatusIsResolved("open")).toBe(false);
    expect(notesThreadStatusIsResolved("")).toBe(false);
  });
});
