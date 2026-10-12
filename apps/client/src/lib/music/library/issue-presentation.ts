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
