import { describe, expect, it } from "vitest";
import { idleAsyncResource, resolveAsyncRequest } from "$lib/utils/async-resource";
import {
  isCurrentWorkingMarkdownRevision,
  isWorkingMarkdownDirty,
  replaceWorkingMarkdownRevision,
  startWorkingMarkdownRead,
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

describe("working Markdown revision guards", () => {
  const first = { revision: "revision-1", content: "first" };
  const saved = { revision: "revision-2", content: "saved" };

  it("applies a save to the revision it started from", () => {
    const loaded = idleAsyncResource<typeof first | null>(first);
    expect(replaceWorkingMarkdownRevision(loaded, first, saved)?.data).toBe(saved);
  });

  it("drops a save that settles after the editor switched to another file", () => {
    const started = startWorkingMarkdownRead<typeof first>();
    expect(started.resource.data).toBeNull();
    expect(replaceWorkingMarkdownRevision(started.resource, first, saved)).toBeNull();
    const other = { revision: "other-1", content: "other" };
    const switched = resolveAsyncRequest(started.resource, started.token, other);
    expect(replaceWorkingMarkdownRevision(switched, first, saved)).toBeNull();
    expect(isCurrentWorkingMarkdownRevision(switched, other)).toBe(true);
  });

  it("drops a response when another write replaced the loaded revision", () => {
    const refreshed = idleAsyncResource<typeof first | null>({ ...first });
    expect(isCurrentWorkingMarkdownRevision(refreshed, first)).toBe(false);
  });
});
