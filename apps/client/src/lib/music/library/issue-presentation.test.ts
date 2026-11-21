import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { groupMusicIssues, MUSIC_ISSUE_MESSAGE_KEYS, musicIssueGroup, musicIssueMessageKey } from "./issue-presentation";
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

function catalogText(catalog: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>(
    (node, part) => (node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined),
    catalog,
  );
}

describe("music issue messages", () => {
  it("has English and Spanish text for every known issue kind and the generic message", () => {
    for (const key of [...Object.values(MUSIC_ISSUE_MESSAGE_KEYS), "music.issueMessages.unknown"]) {
      expect(typeof catalogText(en, key), `en ${key}`).toBe("string");
      expect(typeof catalogText(es, key), `es ${key}`).toBe("string");
    }
  });

  it("explains each native issue kind with its own message", () => {
    expect(musicIssueMessageKey("item-missing")).toBe("music.issueMessages.itemMissing");
    expect(musicIssueMessageKey("youtube-embedding-blocked")).toBe("music.issueMessages.youtubeEmbeddingBlocked");
    expect(musicIssueMessageKey("relink-ambiguous")).toBe("music.issueMessages.relinkAmbiguous");
  });

  it("uses the generic message for kinds this client does not know", () => {
    expect(musicIssueMessageKey("item-unknown")).toBe("music.issueMessages.unknown");
    expect(musicIssueMessageKey("toString")).toBe("music.issueMessages.unknown");
  });
});
