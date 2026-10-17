import { describe, expect, it } from "vitest";
import { groupMusicIssues, musicIssueGroup } from "./issue-presentation";
import type { MusicIssue } from "./contracts";

function issue(issueKind: string): MusicIssue {
  return { id: issueKind, issueKind, itemId: null, playlistId: null, collectionId: null, rootId: null, relativePath: null, actionRequired: true, message: issueKind, createdAtMs: 1 };
}

describe("music issue presentation", () => {
  it.each([
    ["missing-local-file", "missing-local-file"],
    ["root-unavailable", "root-unavailable"],
    ["ambiguous-match", "ambiguous-match"],
    ["youtube-unavailable", "youtube-unavailable"],
    ["embedding-blocked", "embedding-blocked"],
    ["refresh-partial", "refresh-incomplete"],
  ] as const)("maps %s to %s", (kind, expected) => {
    expect(musicIssueGroup(issue(kind))).toBe(expected);
  });

  it("preserves stable issue order inside each group", () => {
    const grouped = groupMusicIssues([issue("missing-first"), issue("missing-second")]);
    expect(grouped.get("missing-local-file")?.map((entry) => entry.id)).toEqual(["missing-first", "missing-second"]);
  });

  it("keeps categories stable when newer issues arrive in a different order", () => {
    const grouped = groupMusicIssues([
      issue("refresh-partial"),
      issue("youtube-unavailable"),
      issue("missing-local-file"),
    ]);
    expect([...grouped.keys()]).toEqual([
      "missing-local-file",
      "youtube-unavailable",
      "refresh-incomplete",
    ]);
  });
});
