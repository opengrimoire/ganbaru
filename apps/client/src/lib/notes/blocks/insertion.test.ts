import { describe, expect, it } from "vitest";
import {
  createBlockWriteFromInsertCommand,
  isNotesInsertableBlockType,
  normalizeNotesBlockInsertCommand,
  notesBlockInsertCommandKey,
  notesBlockInsertCommands,
  notesBlockInsertMenuStyle,
  notesInsertableBlockTypes,
} from "./insertion";

describe("notes block insertion catalog", () => {
  it("includes user-created block types", () => {
    expect(notesInsertableBlockTypes()).toContain("paragraph");
    expect(notesInsertableBlockTypes()).toContain("heading_4");
    expect(notesInsertableBlockTypes()).toContain("to_do");
    expect(notesInsertableBlockTypes()).toContain("column_list");
    expect(notesInsertableBlockTypes()).toContain("tab");
    expect(notesInsertableBlockTypes()).toContain("child_database");
    expect(notesInsertableBlockTypes()).toContain("pdf");
    expect(notesInsertableBlockTypes()).toContain("link_preview");
    expect(notesInsertableBlockTypes()).toContain("template");
    expect(notesInsertableBlockTypes()).toContain("button");
  });

  it("excludes internal structural and preservation-only block types", () => {
    expect(notesInsertableBlockTypes()).not.toContain("column");
    expect(notesInsertableBlockTypes()).not.toContain("table_row");
    expect(notesInsertableBlockTypes()).not.toContain("synced_block");
    expect(notesInsertableBlockTypes()).not.toContain("unsupported");
  });

  it("validates insertable block types", () => {
    expect(isNotesInsertableBlockType("bookmark")).toBe(true);
    expect(isNotesInsertableBlockType("link_preview")).toBe(true);
    expect(isNotesInsertableBlockType("child_database")).toBe(true);
    expect(isNotesInsertableBlockType("synced_block")).toBe(false);
    expect(isNotesInsertableBlockType("table_row")).toBe(false);
    expect(isNotesInsertableBlockType("unknown")).toBe(false);
  });

  it("builds plus menu commands from user-created blocks and toggle headings", () => {
    const commands = notesBlockInsertCommands();
    expect(commands).toContainEqual({ kind: "block", blockType: "paragraph" });
    expect(commands).toContainEqual({ kind: "block", blockType: "table" });
    expect(commands).toContainEqual({ kind: "block", blockType: "child_database" });
    expect(commands).toContainEqual({ kind: "toggle_heading", headingType: "heading_2" });
    expect(commands).not.toContainEqual({ kind: "block", blockType: "table_row" });
    expect(commands).not.toContainEqual({ kind: "block", blockType: "column" });
    expect(notesBlockInsertCommandKey({ kind: "toggle_heading", headingType: "heading_2" })).toBe(
      "toggle-heading:heading_2",
    );
  });

  it("defaults omitted insertion requests to a paragraph", () => {
    expect(normalizeNotesBlockInsertCommand()).toEqual({
      kind: "block",
      blockType: "paragraph",
    });
  });

  it("creates toggle heading writes for plus insertion", () => {
    const write = createBlockWriteFromInsertCommand("block-1", {
      kind: "toggle_heading",
      headingType: "heading_3",
    });

    expect(write.type).toBe("heading_3");
    if (write.type !== "heading_3") throw new Error("Expected heading 3 write");
    expect(write.heading_3.is_toggleable).toBe(true);
    expect(write.heading_3.ganbaru_open).toBe(true);
  });

  it("clamps the plus menu inside narrow viewports", () => {
    const style = notesBlockInsertMenuStyle({
      triggerRect: {
        top: 20,
        right: 292,
        bottom: 40,
        left: 268,
      },
      viewportWidth: 280,
      viewportHeight: 180,
    });

    expect(style).toContain("left:16px");
    expect(style).toContain("width:256px");
    expect(style).toContain("max-height:128px");
  });

  it("places the plus menu above the trigger when there is more room above", () => {
    const style = notesBlockInsertMenuStyle({
      triggerRect: {
        top: 380,
        right: 40,
        bottom: 400,
        left: 16,
      },
      viewportWidth: 320,
      viewportHeight: 420,
    });

    expect(style).toContain("top:8px");
    expect(style).toContain("max-height:368px");
  });
});
