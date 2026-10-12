import { describe, expect, it, vi } from "vitest";

vi.mock("$lib/api/notes", () => ({
  listNotesDataSources: vi.fn(async () => [
    { id: "first" },
    { id: "second" },
  ]),
  listNotesDataSourceRowPages: vi.fn(async (id: string) => {
    if (id === "first") throw new Error("unavailable");
    return [{ id: "row-2" }];
  }),
}));

import { loadNotesDatabaseMentionData } from "./mention-targets";

describe("Notes mention data loading", () => {
  it("keeps successful data-source rows when one source fails", async () => {
    const result = await loadNotesDatabaseMentionData();
    expect(result.dataSources.map((source) => source.id)).toEqual(["first", "second"]);
    expect(result.rowPages.map((page) => page.id)).toEqual(["row-2"]);
  });
});
