import { describe, expect, it } from "vitest";
import {
  isWorkingMarkdownDirty,
  workingMarkdownRefreshDecision,
} from "./editor";

describe("working Markdown editor state", () => {
  it("tracks dirty text only after a file has loaded", () => {
    expect(isWorkingMarkdownDirty(null, "draft")).toBe(false);
    expect(isWorkingMarkdownDirty("saved", "saved")).toBe(false);
    expect(isWorkingMarkdownDirty("saved", "edited")).toBe(true);
  });

  it("preserves local edits unless the disk revision creates a conflict", () => {
    expect(workingMarkdownRefreshDecision("saved", "revision-1", "saved", "revision-2"))
      .toBe("replace");
    expect(workingMarkdownRefreshDecision("saved", "revision-1", "edited", "revision-1"))
      .toBe("preserve_local");
    expect(workingMarkdownRefreshDecision("saved", "revision-1", "edited", "revision-2"))
      .toBe("conflict");
  });
});
