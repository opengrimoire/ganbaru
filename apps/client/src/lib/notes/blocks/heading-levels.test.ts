// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  applyBlockUpdate, blockEditableRichText, blockPlainText, blockUpdateFromBlock,
  blockWithHeadingToggleOpen, blockWithHeadingToggleable, blockWithText, createBlockWrite,
  headingIsToggleable, headingToggleOpen, isTextEditableBlock,
} from "./factory";
import { blockColor, blockWithColor } from "./color";
import { parseNotesBlock } from "./validation";
import { baseBlock } from "./validation.fixtures";
import { blockTypeForTextShortcut } from "./shortcuts";
import { notesHeadingTextClass } from "./editor-ui";
import { notesBlockInsertCommands } from "./insertion";
import { notesSlashCommandItems, filterNotesSlashCommandItems } from "$lib/notes/editor/slash-commands";
import { buildNotesTableOfContents } from "$lib/notes/block-types/table-of-contents";
import { planNotesPlainTextPaste } from "$lib/notes/clipboard/blocks";
import { planNotesRichHtmlPaste } from "$lib/notes/rich-text/paste";
import { planNotesKeyboardAction } from "./keyboard";

const headings = ["heading_1", "heading_2", "heading_3", "heading_4", "heading_5", "heading_6"] as const;

describe("six heading levels", () => {
  it.each(headings)("preserves %s through editing, color, toggles, validation, and history writes", (type) => {
    let block = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, type, "Original") });
    expect(isTextEditableBlock(type)).toBe(true);
    block = applyBlockUpdate(block, blockWithHeadingToggleable(block, type, true));
    block = applyBlockUpdate(block, blockWithHeadingToggleOpen(block, false));
    block = applyBlockUpdate(block, blockWithColor(block, "blue"));
    block = applyBlockUpdate(block, blockWithText(block, "Edited"));
    expect(blockPlainText(block)).toBe("Edited");
    expect(blockColor(block)).toBe("blue");
    expect(headingIsToggleable(block)).toBe(true);
    expect(headingToggleOpen(block)).toBe(false);
    const restored = parseNotesBlock({ ...baseBlock, ...blockUpdateFromBlock(block) });
    expect(blockUpdateFromBlock(restored)).toEqual(blockUpdateFromBlock(block));
    expect(blockEditableRichText(restored)).toHaveLength(1);
  });

  it.each(headings)("exposes %s through shortcuts, menus, outline, and shared typography", (type) => {
    const level = headings.indexOf(type) + 1;
    const hashes = "#".repeat(level);
    expect(blockTypeForTextShortcut(hashes)).toBe(type);
    expect(notesBlockInsertCommands()).toContainEqual({ kind: "block", blockType: type });
    expect(notesBlockInsertCommands()).toContainEqual({ kind: "toggle_heading", headingType: type });
    const matches = filterNotesSlashCommandItems(notesSlashCommandItems({ canSetColor: false }), hashes);
    expect(matches).toContainEqual(expect.objectContaining({ command: { kind: "block", blockType: type } }));
    const block = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, type, "Title") });
    expect(buildNotesTableOfContents([{ block, depth: 0, parentId: "page", previousSiblingId: null, previousVisibleId: null }])).toEqual([{ blockId: block.id, title: "Title", level }]);
    expect(notesHeadingTextClass(type)).toBe(`notes-heading-text notes-heading-${level} font-semibold leading-[1.3]`);
    expect(planNotesKeyboardAction({
      key: "Backspace", text: "Title", selectionStart: 0, selectionEnd: 0, blockType: type,
      previousBlockType: "paragraph", isOnlyBlock: false,
      ctrlKey: false, metaKey: false, altKey: false, shiftKey: false,
    }).type).toBe("remove_block_format");
  });

  it.each(headings)("retains the %s level in Markdown and rich HTML paste", (type) => {
    const level = headings.indexOf(type) + 1;
    const currentBlock = parseNotesBlock({ ...baseBlock, ...createBlockWrite(baseBlock.id, "paragraph", "") });
    const plain = planNotesPlainTextPaste({
      currentBlockId: currentBlock.id, currentBlockType: "paragraph", currentText: "",
      selectionStart: 0, selectionEnd: 0, plainText: `${"#".repeat(level)} Heading\nBody`,
      createId: () => "next",
    });
    expect(plain?.currentUpdate.type).toBe(type);
    const rich = planNotesRichHtmlPaste({
      currentBlock, selectionStart: 0, selectionEnd: 0,
      html: `<h${level}><strong>Heading</strong></h${level}><p>Body</p>`, createId: () => "next",
    });
    expect(rich?.currentUpdate.type).toBe(type);
    if (!rich) throw new Error("Expected heading paste");
    expect(blockEditableRichText(applyBlockUpdate(currentBlock, rich.currentUpdate))[0].annotations.bold).toBe(true);
  });

  it("does not interpret a seventh heading level as a heading", () => {
    expect(blockTypeForTextShortcut("#######")).toBeNull();
    expect(notesHeadingTextClass("heading_7")).toBe("");
  });
});
