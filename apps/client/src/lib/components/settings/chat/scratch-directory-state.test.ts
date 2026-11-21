import { describe, expect, it } from "vitest";
import type { ChatScratchDirectoryEntryRead, ChatScratchDirectoryPageRead, ChatScratchGenerationId } from "$lib/chat/contracts";
import {
  IDLE_SCRATCH_DIRECTORY,
  transitionScratchDirectory,
  visibleScratchDirectory,
  type ScratchDirectoryState,
} from "./scratch-directory-state";

const FIRST: ChatScratchGenerationId = "scratch-generation:first";
const SECOND: ChatScratchGenerationId = "scratch-generation:second";

function entry(relativePath: string): ChatScratchDirectoryEntryRead {
  return {
    relativePath,
    displayName: relativePath.split("/").at(-1) ?? relativePath,
    kind: "file",
    byteSize: 1,
    contentRevision: `revision:${relativePath}`,
    promotable: true,
  };
}

function page(
  generationId: ChatScratchGenerationId, relativePath: string, paths: string[], nextCursor: string | null = null,
): ChatScratchDirectoryPageRead {
  return { scratchGenerationId: generationId, relativePath, entries: paths.map(entry), nextCursor };
}

function requested(
  state: ScratchDirectoryState, request: number, generationId: ChatScratchGenerationId,
  relativePath = "", cursor: string | null = null,
): ScratchDirectoryState {
  return transitionScratchDirectory(state, { type: "requested", request, generationId, relativePath, cursor });
}

describe("transitionScratchDirectory", () => {
  it("shows a requested listing once it loads", () => {
    const loading = requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST);
    expect(loading.load).toEqual({ phase: "loading", request: 1, generationId: FIRST, relativePath: "", cursor: null });
    const ready = transitionScratchDirectory(loading, { type: "loaded", request: 1, page: page(FIRST, "", ["a.txt"]) });
    expect(ready.load).toEqual({ phase: "idle" });
    expect(visibleScratchDirectory(ready, FIRST)?.entries.map((item) => item.relativePath)).toEqual(["a.txt"]);
  });

  it("drops a late listing of a generation the user moved away from", () => {
    const first = requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST);
    const second = requested(first, 2, SECOND);
    const ready = transitionScratchDirectory(second, { type: "loaded", request: 2, page: page(SECOND, "", ["new.txt"]) });
    const late = transitionScratchDirectory(ready, { type: "loaded", request: 1, page: page(FIRST, "", ["old.txt"]) });
    expect(late).toBe(ready);
    expect(visibleScratchDirectory(late, SECOND)?.entries.map((item) => item.relativePath)).toEqual(["new.txt"]);
  });

  it("keeps the newer load in flight when an older one settles", () => {
    const second = requested(requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST), 2, SECOND);
    expect(transitionScratchDirectory(second, { type: "loaded", request: 1, page: page(FIRST, "", []) })).toBe(second);
    expect(transitionScratchDirectory(second, { type: "failed", request: 1 })).toBe(second);
    expect(second.load.phase).toBe("loading");
  });

  it("drops a listing that lands after the scope was cleared", () => {
    const loading = requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST);
    const cleared = transitionScratchDirectory(loading, { type: "cleared" });
    expect(cleared).toEqual(IDLE_SCRATCH_DIRECTORY);
    expect(transitionScratchDirectory(cleared, { type: "loaded", request: 1, page: page(FIRST, "", ["a.txt"]) })).toBe(cleared);
  });

  it("hides a listing while another generation is selected", () => {
    const ready = transitionScratchDirectory(requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST), {
      type: "loaded", request: 1, page: page(FIRST, "", ["a.txt"]),
    });
    expect(visibleScratchDirectory(ready, SECOND)).toBeNull();
    expect(visibleScratchDirectory(ready, null)).toBeNull();
  });

  it("appends the next page of the same folder", () => {
    const ready = transitionScratchDirectory(requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST, "docs"), {
      type: "loaded", request: 1, page: page(FIRST, "docs", ["docs/a.txt"], "cursor"),
    });
    const more = transitionScratchDirectory(requested(ready, 2, FIRST, "docs", "cursor"), {
      type: "loaded", request: 2, page: page(FIRST, "docs", ["docs/b.txt"]),
    });
    expect(more.page?.entries.map((item) => item.relativePath)).toEqual(["docs/a.txt", "docs/b.txt"]);
    expect(more.page?.nextCursor).toBeNull();
  });

  it("replaces the listing when a cursor page belongs to another folder", () => {
    const ready = transitionScratchDirectory(requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST, "docs"), {
      type: "loaded", request: 1, page: page(FIRST, "docs", ["docs/a.txt"], "cursor"),
    });
    const other = transitionScratchDirectory(requested(ready, 2, FIRST, "src", "cursor"), {
      type: "loaded", request: 2, page: page(FIRST, "src", ["src/b.txt"]),
    });
    expect(other.page?.entries.map((item) => item.relativePath)).toEqual(["src/b.txt"]);
  });

  it("keeps the shown listing when a load fails", () => {
    const ready = transitionScratchDirectory(requested(IDLE_SCRATCH_DIRECTORY, 1, FIRST), {
      type: "loaded", request: 1, page: page(FIRST, "", ["a.txt"]),
    });
    const failed = transitionScratchDirectory(requested(ready, 2, FIRST, "docs"), { type: "failed", request: 2 });
    expect(failed.load).toEqual({ phase: "idle" });
    expect(failed.page).toBe(ready.page);
  });

  it("ignores results when nothing is loading", () => {
    expect(transitionScratchDirectory(IDLE_SCRATCH_DIRECTORY, { type: "failed", request: 1 })).toBe(IDLE_SCRATCH_DIRECTORY);
    expect(transitionScratchDirectory(IDLE_SCRATCH_DIRECTORY, { type: "cleared" })).toBe(IDLE_SCRATCH_DIRECTORY);
  });
});
