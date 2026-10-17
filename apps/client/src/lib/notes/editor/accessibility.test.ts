import { describe, expect, it } from "vitest";
import {
  notesMentionMenuDomId,
  notesMentionOptionDomId,
  notesRichTextEditorActiveDescendant,
  notesRichTextEditorControls,
  notesRichTextEditorDomId,
  notesRichTextEditorStatusDomId,
  notesSlashMenuItemDomId,
  notesSlashMenuDomId,
} from "./accessibility";

describe("notes rich editor accessibility helpers", () => {
  it("builds stable block-scoped ids for editor popups", () => {
    expect(notesRichTextEditorDomId("block-1")).toBe("notes-rich-editor-block-1");
    expect(notesRichTextEditorStatusDomId("block-1")).toBe(
      "notes-rich-editor-status-block-1",
    );
    expect(notesMentionMenuDomId("block-1")).toBe("notes-mention-menu-block-1");
    expect(notesMentionOptionDomId("block-1", 2)).toBe("notes-mention-option-block-1-2");
    expect(notesSlashMenuDomId("block-1")).toBe("notes-slash-menu-block-1");
    expect(notesSlashMenuItemDomId("block-1", 2)).toBe("notes-slash-menu-block-1-item-2");
  });

  it("points the editor at the open menu only", () => {
    expect(notesRichTextEditorControls("block-1", true, false)).toBe(
      "notes-mention-menu-block-1",
    );
    expect(notesRichTextEditorControls("block-1", false, true)).toBe(
      "notes-slash-menu-block-1",
    );
    expect(notesRichTextEditorControls("block-1", false, false)).toBeUndefined();
  });

  it("announces the active mention option only when it exists", () => {
    expect(notesRichTextEditorActiveDescendant("block-1", true, 1, 3)).toBe(
      "notes-mention-option-block-1-1",
    );
    expect(notesRichTextEditorActiveDescendant("block-1", true, 3, 3)).toBeUndefined();
    expect(notesRichTextEditorActiveDescendant("block-1", false, 1, 3)).toBeUndefined();
  });
});
