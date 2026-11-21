import { describe, expect, it } from "vitest";
import { createLazyComponentLoader } from "$lib/lazy-component-loader";
import {
  notesBlockRenderFamily,
  retryNotesAdvancedBlock,
  type NotesAdvancedBlockFamily,
} from "./editor-component-registry";
import { NOTES_BLOCK_TYPES } from "$lib/notes/types";

const BLOCK_FAMILIES = [
  "media",
  "child-database",
  "table",
  "card",
  "unsupported",
  "column-list",
  "tab",
] as const satisfies readonly NotesAdvancedBlockFamily[];

describe("Notes editor component registry", () => {
  it("registers every persisted block type in an eager or lazy render family", () => {
    const families = NOTES_BLOCK_TYPES.map((type) => notesBlockRenderFamily(type));
    expect(families).toHaveLength(NOTES_BLOCK_TYPES.length);
    expect(new Set(families)).toEqual(new Set(["eager", ...BLOCK_FAMILIES]));
  });

  it("does not cache a rejected import and retries successfully", async () => {
    let attempts = 0;
    const loader = createLazyComponentLoader({
      media: async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("temporary chunk failure");
        return { default: "loaded" };
      },
    });

    await expect(loader.load("media")).rejects.toThrow("temporary chunk failure");
    await expect(loader.retry("media")).resolves.toBe("loaded");
    expect(attempts).toBe(2);
  });

  it("keeps retry entry points type-safe after a loaded block", async () => {
    await expect(retryNotesAdvancedBlock("media")).resolves.toMatchObject({ kind: "media" });
  });
});
