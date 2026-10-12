import { describe, expect, it } from "vitest";
import { readNotesDatabaseViewTabDisplay, withNotesDatabaseViewTabDisplay } from "./view-tab-display.svelte";

describe("notes database view tab display", () => {
  it("reads a stored display and falls back to text and icon", () => {
    expect(readNotesDatabaseViewTabDisplay({ a: "icon" }, "a")).toBe("icon");
    expect(readNotesDatabaseViewTabDisplay({ a: "icon" }, "b")).toBe("text_and_icon");
  });

  it("ignores malformed stored maps and values", () => {
    expect(readNotesDatabaseViewTabDisplay(undefined, "a")).toBe("text_and_icon");
    expect(readNotesDatabaseViewTabDisplay(["icon"], "0")).toBe("text_and_icon");
    expect(readNotesDatabaseViewTabDisplay("icon", "a")).toBe("text_and_icon");
    expect(readNotesDatabaseViewTabDisplay({ a: "huge" }, "a")).toBe("text_and_icon");
  });

  it("replaces one view and keeps other valid views", () => {
    expect(withNotesDatabaseViewTabDisplay({ a: "icon", b: "text" }, "a", "text")).toEqual({ a: "text", b: "text" });
  });

  it("omits the default display and forgotten views", () => {
    expect(withNotesDatabaseViewTabDisplay({ a: "icon", b: "text" }, "a", "text_and_icon")).toEqual({ b: "text" });
    expect(withNotesDatabaseViewTabDisplay({ a: "icon" }, "a", null)).toEqual({});
  });

  it("drops invalid entries while writing", () => {
    expect(withNotesDatabaseViewTabDisplay({ a: 3, b: "icon", c: "huge" }, "d", "text")).toEqual({ b: "icon", d: "text" });
    expect(withNotesDatabaseViewTabDisplay(null, "a", "icon")).toEqual({ a: "icon" });
  });
});
