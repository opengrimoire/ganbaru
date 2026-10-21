import { describe, expect, it } from "vitest";

const NOTES_SOURCES = import.meta.glob<string>(
  ["/src/lib/components/notes/**/*.svelte", "/src/lib/components/notes/**/*.ts", "!/src/**/*.test.ts"],
  { eager: true, query: "?raw", import: "default" },
);

/**
 * Components that open only fixed help URLs, which the opener capability grants by exact URL. User-authored
 * Notes links go through the `notes_open_external_url` command, because the capability cannot list them.
 */
const FIXED_URL_OPENERS = new Set(["/src/lib/components/notes/transfer/NotesTransferFieldRow.svelte"]);

const OPENER_IMPORT = /from\s*["']@tauri-apps\/plugin-opener["']/u;

describe("Notes external links", () => {
  it("open user-authored links through the validated Notes command", () => {
    const sources = Object.entries(NOTES_SOURCES);
    expect(sources.length).toBeGreaterThan(0);
    const directOpeners = sources
      .filter(([path, source]) => OPENER_IMPORT.test(source) && !FIXED_URL_OPENERS.has(path))
      .map(([path]) => path);
    expect(directOpeners).toEqual([]);

    for (const path of [
      "/src/lib/components/notes/blocks/NotesCardBlock.svelte",
      "/src/lib/components/notes/blocks/NotesMediaBlock.svelte",
      "/src/lib/components/notes/working-markdown/NotesWorkingMarkdownEditor.svelte",
    ]) {
      expect(NOTES_SOURCES[path], path).toContain("openNotesExternalUrl(");
    }
  });
});
