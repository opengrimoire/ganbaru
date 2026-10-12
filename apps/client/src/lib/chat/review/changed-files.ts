import type { ChangedFileSummary } from "$lib/chat/contracts";

const MAX_CHANGED_FILES = 512;
const MAX_CHANGED_FILE_PATH_BYTES = 4_096;

/**
 * Removes provider paths that cannot safely identify a workspace-relative file and
 * merges repeated summaries using the last, usually most complete, provider value.
 */
export function normalizeChangedFileSummaries(
  files: readonly ChangedFileSummary[],
): ChangedFileSummary[] {
  const normalized = new Map<string, ChangedFileSummary>();
  for (const file of files.slice(0, MAX_CHANGED_FILES)) {
    if (!isWorkspaceRelativeChangedFilePath(file.relativePath)) continue;
    normalized.set(file.relativePath, {
      ...file,
      previousRelativePath: file.previousRelativePath
        && isWorkspaceRelativeChangedFilePath(file.previousRelativePath)
        ? file.previousRelativePath
        : null,
    });
  }
  return [...normalized.values()];
}

/** Returns whether a provider path is a bounded canonical workspace-relative path. */
export function isWorkspaceRelativeChangedFilePath(path: string): boolean {
  if (!path
    || new TextEncoder().encode(path).length > MAX_CHANGED_FILE_PATH_BYTES
    || path.startsWith("/")
    || path.startsWith("\\")
    || /^[A-Za-z]:[\\/]/.test(path)
    || path.includes("\\")
    || /\p{Cc}/u.test(path)) {
    return false;
  }
  return path.split("/").every((component) => (
    component.length > 0 && component !== "." && component !== ".."
  ));
}
