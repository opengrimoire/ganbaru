import type { TranslationKey } from "$lib/i18n/translator.svelte";
import type { MusicIssue } from "./contracts";

export type MusicIssueGroup =
  | "missing-local-file"
  | "root-unavailable"
  | "ambiguous-match"
  | "youtube-unavailable"
  | "embedding-blocked"
  | "refresh-incomplete";

export const MUSIC_ISSUE_GROUPS: readonly MusicIssueGroup[] = [
  "missing-local-file",
  "root-unavailable",
  "ambiguous-match",
  "youtube-unavailable",
  "embedding-blocked",
  "refresh-incomplete",
];

export function musicIssueGroup(issue: MusicIssue): MusicIssueGroup {
  const kind = issue.issueKind.toLocaleLowerCase();
  if (kind.includes("root") && (kind.includes("missing") || kind.includes("unavailable"))) return "root-unavailable";
  if (kind.includes("ambiguous")) return "ambiguous-match";
  if (kind.includes("embedding") || kind.includes("embed")) return "embedding-blocked";
  if (kind.includes("youtube") || kind.includes("video")) return "youtube-unavailable";
  if (kind.includes("refresh") || kind.includes("scan") || kind.includes("partial")) return "refresh-incomplete";
  return "missing-local-file";
}

/** Catalog keys under `music.issueMessages`. */
export type MusicIssueMessageKey = Extract<TranslationKey, `music.issueMessages.${string}`>;

/**
 * Message for every issue kind the native library reports. The stored issue text is English
 * diagnostic detail, so the person sees these messages instead.
 */
export const MUSIC_ISSUE_MESSAGE_KEYS = {
  "youtube-embedding-blocked": "music.issueMessages.youtubeEmbeddingBlocked",
  "youtube-timed-out": "music.issueMessages.youtubeTimedOut",
  "youtube-unavailable": "music.issueMessages.youtubeUnavailable",
  "item-missing": "music.issueMessages.itemMissing",
  "item-ambiguous": "music.issueMessages.itemAmbiguous",
  "item-unavailable": "music.issueMessages.itemUnavailable",
  "ambiguous-identity": "music.issueMessages.ambiguousIdentity",
  "metadata-fallback": "music.issueMessages.metadataFallback",
  "media-inspection-failed": "music.issueMessages.mediaInspectionFailed",
  "refresh-incomplete": "music.issueMessages.refreshIncomplete",
  "relink-ambiguous": "music.issueMessages.relinkAmbiguous",
  "relink-missing": "music.issueMessages.relinkMissing",
} as const satisfies Readonly<Record<string, MusicIssueMessageKey>>;

export type KnownMusicIssueKind = keyof typeof MUSIC_ISSUE_MESSAGE_KEYS;

/**
 * The catalog key that explains an issue, with a generic message for kinds this client does not
 * know.
 *
 * @param issueKind The native issue kind.
 */
export function musicIssueMessageKey(issueKind: string): MusicIssueMessageKey {
  return isKnownMusicIssueKind(issueKind) ? MUSIC_ISSUE_MESSAGE_KEYS[issueKind] : "music.issueMessages.unknown";
}

function isKnownMusicIssueKind(issueKind: string): issueKind is KnownMusicIssueKind {
  return Object.hasOwn(MUSIC_ISSUE_MESSAGE_KEYS, issueKind);
}

export function groupMusicIssues(issues: readonly MusicIssue[]): Map<MusicIssueGroup, MusicIssue[]> {
  const collected = new Map<MusicIssueGroup, MusicIssue[]>();
  for (const issue of issues) {
    const group = musicIssueGroup(issue);
    collected.set(group, [...(collected.get(group) ?? []), issue]);
  }
  return new Map(MUSIC_ISSUE_GROUPS.flatMap((group) => {
    const entries = collected.get(group);
    return entries ? [[group, entries] as const] : [];
  }));
}
