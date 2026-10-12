import { describe, expect, it } from "vitest";
import { createSyncedBlockPayload } from "$lib/notes/blocks/factory";
import { notesSyncedBlockStatus } from "./synced";

describe("notes synced block status", () => {
  it("treats original synced blocks as local child containers without fanout editing", () => {
    expect(notesSyncedBlockStatus(createSyncedBlockPayload())).toEqual({
      role: "original",
      sourceBlockId: null,
      canOwnChildren: true,
      canEditLoadedChildren: true,
      canEditSyncedCopies: false,
    });
  });

  it("treats duplicate synced blocks as preserved references without local child edits", () => {
    const sourceBlockId = "11111111-1111-4111-8111-111111111111";

    expect(notesSyncedBlockStatus(createSyncedBlockPayload(sourceBlockId))).toEqual({
      role: "duplicate",
      sourceBlockId,
      canOwnChildren: false,
      canEditLoadedChildren: false,
      canEditSyncedCopies: false,
    });
  });
});
