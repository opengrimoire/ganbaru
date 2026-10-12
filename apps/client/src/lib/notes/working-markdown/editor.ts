export type WorkingMarkdownRefreshDecision = "replace" | "preserve_local" | "conflict";

/** Report whether raw Markdown differs from the last file revision read from disk. */
export function isWorkingMarkdownDirty(savedContent: string | null, draft: string): boolean {
  return savedContent !== null && draft !== savedContent;
}

/** Decide how an explicit refresh should combine local text and a new disk revision. */
export function workingMarkdownRefreshDecision(
  savedContent: string,
  savedRevision: string,
  draft: string,
  remoteRevision: string,
): WorkingMarkdownRefreshDecision {
  if (!isWorkingMarkdownDirty(savedContent, draft)) return "replace";
  return remoteRevision === savedRevision ? "preserve_local" : "conflict";
}
