import { describe, expect, it } from "vitest";
import {
  normalizeNotesClipboardPlainText,
  notesPasteAppendRequests,
  planNotesPlainTextPaste,
} from "./blocks";

let idCounter = 0;

function nextId(): string {
  idCounter += 1;
  return `00000000-0000-4000-8000-${idCounter.toString().padStart(12, "0")}`;
}

function plan(input: {
  currentText?: string;
  selectionStart?: number;
  selectionEnd?: number;
  plainText: string;
}) {
  idCounter = 0;
  return planNotesPlainTextPaste({
    currentBlockId: "00000000-0000-4000-8000-000000000999",
    currentBlockType: "paragraph",
    currentText: input.currentText ?? "",
    selectionStart: input.selectionStart ?? input.currentText?.length ?? 0,
    selectionEnd: input.selectionEnd ?? input.currentText?.length ?? 0,
    plainText: input.plainText,
    createId: nextId,
  });
}

describe("notes clipboard paste planning", () => {
  it("preserves child depth when the first pasted item joins existing text", () => {
    const result = plan({ currentText: "Before ", plainText: "- Parent\n  - Child" });
    expect(result?.blockDepths).toEqual([0, 1]);
    expect(result?.currentUpdate).toMatchObject({ paragraph: { rich_text: [expect.objectContaining({ plain_text: "Before - Parent" })] } });
  });

  it("retains list ancestry across blank lines and indented continuation text", () => {
    const result = plan({ plainText: "- Parent\n  continued\n\n  - Child\n\n- Sibling" });
    expect(result?.blockDepths).toEqual([0, 1, 0]);
    expect(result?.currentUpdate).toMatchObject({ bulleted_list_item: { rich_text: [expect.objectContaining({ plain_text: "Parent\ncontinued" })] } });
  });

  it("preserves mixed list nesting with spaces, tabs, dedentation, and task markers", () => {
    const result = plan({ plainText: "- Parent\n  1. Child\n\t- [x] Task\n  2. Child two\n- Sibling\nAfter" });
    if (!result) throw new Error("Expected plan");
    expect(result.blockDepths).toEqual([0, 1, 2, 1, 0, 0]);
    expect(result.appendedBlocks[1]).toMatchObject({ type: "to_do", to_do: { checked: true } });
    const requests = notesPasteAppendRequests("current", { type: "page_id", page_id: "page" }, result.appendedBlocks, result.blockDepths);
    expect(requests.map((request) => [request.parent, request.children.map((write) => write.id)])).toEqual([
      [{ type: "block_id", block_id: "current" }, [result.appendedBlocks[0].id, result.appendedBlocks[2].id]],
      [{ type: "block_id", block_id: result.appendedBlocks[0].id }, [result.appendedBlocks[1].id]],
      [{ type: "page_id", page_id: "page" }, [result.appendedBlocks[3].id, result.appendedBlocks[4].id]],
    ]);
  });

  it("leaves normal single-line paste to the editor input handler", () => {
    expect(plan({ currentText: "Hello ", plainText: "world" })).toBeNull();
  });

  it("normalizes line endings and rejects unsupported control characters", () => {
    expect(normalizeNotesClipboardPlainText("a\r\nb\rc")).toBe("a\nb\nc");
    expect(normalizeNotesClipboardPlainText("bad\u0008")).toBeNull();
  });

  it("splits multi-line paste into a current block update and sibling writes", () => {
    const pastePlan = plan({
      currentText: "Start ",
      plainText: "one\ntwo\nthree",
    });
    expect(pastePlan?.currentUpdate).toMatchObject({
      type: "paragraph",
      paragraph: {
        rich_text: [
          expect.objectContaining({
            plain_text: "Start one",
          }),
        ],
      },
    });
    expect(pastePlan?.appendedBlocks.map((block) => block.type)).toEqual([
      "paragraph",
      "paragraph",
    ]);
    expect(pastePlan?.appendedBlocks[1]).toMatchObject({
      paragraph: {
        rich_text: [
          expect.objectContaining({
            plain_text: "three",
          }),
        ],
      },
    });
    expect(pastePlan?.focusOffset).toBe("three".length);
  });

  it("moves selected-text suffix into the final pasted block", () => {
    const pastePlan = plan({
      currentText: "Hello world",
      selectionStart: 6,
      selectionEnd: 6,
      plainText: "first\nsecond",
    });
    expect(pastePlan?.currentUpdate).toMatchObject({
      type: "paragraph",
      paragraph: {
        rich_text: [
          expect.objectContaining({
            plain_text: "Hello first",
          }),
        ],
      },
    });
    expect(pastePlan?.appendedBlocks[0]).toMatchObject({
      paragraph: {
        rich_text: [
          expect.objectContaining({
            plain_text: "secondworld",
          }),
        ],
      },
    });
    expect(pastePlan?.focusOffset).toBe("second".length);
  });

  it("converts pasted markdown line prefixes into canonical block types", () => {
    const pastePlan = plan({
      plainText: [
        "# Heading",
        "#### Fine print",
        "- Bullet",
        "1. Number",
        "- [ ] Todo",
        "- [x] Done",
        "> Quote",
        "\" Quote",
        "---",
      ].join("\n"),
    });
    expect(pastePlan?.currentUpdate.type).toBe("heading_1");
    expect(pastePlan?.appendedBlocks.map((block) => block.type)).toEqual([
      "heading_4",
      "bulleted_list_item",
      "numbered_list_item",
      "to_do",
      "to_do",
      "quote",
      "paragraph",
      "divider",
    ]);
    const doneBlock = pastePlan?.appendedBlocks[4];
    expect(doneBlock).toMatchObject({
      type: "to_do",
      to_do: { checked: true },
    });
  });

  it("keeps a fenced code paste as one code block", () => {
    const pastePlan = plan({
      plainText: [
        "```ts",
        "const value = 1;",
        "console.log(value);",
        "```",
      ].join("\n"),
    });
    expect(pastePlan?.currentUpdate).toMatchObject({
      type: "code",
      code: {
        language: "ts",
        rich_text: [
          expect.objectContaining({
            plain_text: "const value = 1;\nconsole.log(value);",
          }),
        ],
      },
    });
  });
});
