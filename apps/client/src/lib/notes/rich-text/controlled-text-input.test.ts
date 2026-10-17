import { describe, expect, it } from "vitest";
import { planNotesControlledTextEdit } from "./controlled-text-input";

describe("notes controlled text input", () => {
  it("inserts typed text at the current selection", () => {
    expect(planNotesControlledTextEdit({
      inputType: "insertText",
      data: "x",
      text: "Eample",
      selectionStart: 1,
      selectionEnd: 1,
    })).toEqual({
      text: "Example",
      selection: { start: 2, end: 2 },
    });
  });

  it("replaces selected text with inserted text", () => {
    expect(planNotesControlledTextEdit({
      inputType: "insertText",
      data: "world",
      text: "Hello selection",
      selectionStart: 6,
      selectionEnd: 15,
    })).toEqual({
      text: "Hello world",
      selection: { start: 11, end: 11 },
    });
  });

  it("deletes backward and forward without splitting surrogate pairs", () => {
    expect(planNotesControlledTextEdit({
      inputType: "deleteContentBackward",
      data: null,
      text: "A😀B",
      selectionStart: 3,
      selectionEnd: 3,
    })).toEqual({
      text: "AB",
      selection: { start: 1, end: 1 },
    });
    expect(planNotesControlledTextEdit({
      inputType: "deleteContentForward",
      data: null,
      text: "A😀B",
      selectionStart: 1,
      selectionEnd: 1,
    })).toEqual({
      text: "AB",
      selection: { start: 1, end: 1 },
    });
  });

  it("deletes selected ranges", () => {
    expect(planNotesControlledTextEdit({
      inputType: "deleteContentBackward",
      data: null,
      text: "Hello world",
      selectionStart: 5,
      selectionEnd: 11,
    })).toEqual({
      text: "Hello",
      selection: { start: 5, end: 5 },
    });
  });

  it("cuts only the selected text and keeps neighboring Unicode intact", () => {
    expect(planNotesControlledTextEdit({
      inputType: "deleteByCut",
      data: null,
      text: "A😀 selection B",
      selectionStart: 4,
      selectionEnd: 14,
    })).toEqual({
      text: "A😀 B",
      selection: { start: 4, end: 4 },
    });
  });

  it("inserts controlled soft newlines", () => {
    expect(planNotesControlledTextEdit({
      inputType: "insertLineBreak",
      data: null,
      text: "Hello",
      selectionStart: 5,
      selectionEnd: 5,
    })).toEqual({
      text: "Hello\n",
      selection: { start: 6, end: 6 },
    });
  });

  describe("with mention tokens", () => {
    // "Eat " + [Eating] + " now", the token spans 4..10.
    const text = "Eat Eating now";
    const atomicRanges = [{ start: 4, end: 10 }];
    const plan = (inputType: string, start: number, end = start, data: string | null = null) =>
      planNotesControlledTextEdit({
        inputType,
        data,
        text,
        selectionStart: start,
        selectionEnd: end,
        atomicRanges,
      });

    it("removes the whole token with one Backspace at its end", () => {
      expect(plan("deleteContentBackward", 10)).toEqual({
        text: "Eat  now",
        selection: { start: 4, end: 4 },
      });
    });

    it("removes the whole token with one Delete at its start", () => {
      expect(plan("deleteContentForward", 4)).toEqual({
        text: "Eat  now",
        selection: { start: 4, end: 4 },
      });
    });

    it("deletes a token as its own word without eating neighboring text", () => {
      expect(plan("deleteWordBackward", 10)?.text).toBe("Eat  now");
      expect(plan("deleteWordForward", 4)?.text).toBe("Eat  now");
      expect(plan("deleteWordBackward", 14)?.text).toBe("Eat Eating ");
    });

    it("keeps ordinary single character deletion next to a token", () => {
      expect(plan("deleteContentBackward", 4)?.text).toBe("EatEating now");
      expect(plan("deleteContentForward", 10)?.text).toBe("Eat Eatingnow");
    });

    it("expands a selection that cuts into a token before deleting or replacing it", () => {
      expect(plan("deleteContentBackward", 2, 6)).toEqual({
        text: "Ea now",
        selection: { start: 2, end: 2 },
      });
      expect(plan("insertText", 8, 12, "x")).toEqual({
        text: "Eat xow",
        selection: { start: 5, end: 5 },
      });
    });

    it("types at the nearest token edge instead of inside it", () => {
      expect(plan("insertText", 5, 5, "x")?.text).toBe("Eat xEating now");
      expect(plan("insertText", 9, 9, "x")?.text).toBe("Eat Eatingx now");
    });

    it("removes only the adjacent token when two tokens touch", () => {
      expect(planNotesControlledTextEdit({
        inputType: "deleteContentBackward",
        data: null,
        text: "UntitledUntitled",
        selectionStart: 8,
        selectionEnd: 8,
        atomicRanges: [{ start: 0, end: 8 }, { start: 8, end: 16 }],
      })).toEqual({
        text: "Untitled",
        selection: { start: 0, end: 0 },
      });
    });
  });
});
