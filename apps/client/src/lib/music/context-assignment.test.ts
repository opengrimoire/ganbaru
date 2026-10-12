import { describe, expect, it } from "vitest";
import { parseMusicContextAssignments } from "./context-assignment";

describe("music context assignment boundary", () => {
  it("parses a complete persisted assignment", () => {
    expect(parseMusicContextAssignments([{
      ownerKind: "project-default",
      ownerId: "project-1",
      phase: "focus",
      behavior: "play-automatically",
      playlistId: "playlist-1",
      soundscapeId: null,
      soundscapeBehavior: "inherit",
      provenanceKind: "explicit",
      provenanceId: null,
      updatedAtMs: 1_700_000_000_000,
      version: 2,
    }])).toEqual([expect.objectContaining({
      ownerId: "project-1",
      phase: "focus",
      version: 2,
    })]);
  });

  it("rejects unknown enum values and unsafe versions", () => {
    const valid = {
      ownerKind: "project-default",
      ownerId: "project-1",
      phase: "focus",
      behavior: "play-automatically",
      playlistId: null,
      soundscapeId: null,
      soundscapeBehavior: "inherit",
      provenanceKind: "explicit",
      provenanceId: null,
      updatedAtMs: 1,
      version: 1,
    };
    expect(() => parseMusicContextAssignments([{ ...valid, behavior: "surprise-me" }])).toThrow("behavior");
    expect(() => parseMusicContextAssignments([{ ...valid, version: Number.MAX_SAFE_INTEGER + 1 }])).toThrow("version");
  });
});
