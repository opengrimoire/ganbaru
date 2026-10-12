import type { ChatWorkspaceChangeBatch, ChatWorkspaceRename } from "$lib/chat/contracts";

export type ChatWorkspacePreviewImpact =
  | { kind: "none" }
  | { kind: "refresh" }
  | { kind: "renamed"; relativePath: string }
  | { kind: "deleted_after_rename"; relativePath: string };

const MAX_COALESCED_WORKSPACE_PATHS = 512;

/** Returns loaded directory caches that need a bounded refresh for a change batch. */
export function workspaceDirectoryRefreshTargets(
  batch: ChatWorkspaceChangeBatch,
  loadedDirectories: readonly string[],
): string[] {
  const loaded = new Set(["", ...loadedDirectories]);
  if (batch.overflowed) return [...loaded].sort(compareDirectoryDepth);
  const targets = new Set<string>();
  for (const parent of batch.affectedParentDirectories) {
    if (loaded.has(parent)) targets.add(parent);
  }
  for (const path of batch.relativePaths) {
    if (loaded.has(path)) targets.add(path);
  }
  return [...targets].sort(compareDirectoryDepth);
}

/** Returns loaded directory caches made unreachable by deletes or renames. */
export function workspaceDirectoryCachesToDrop(
  batch: ChatWorkspaceChangeBatch,
  loadedDirectories: readonly string[],
): string[] {
  const removedRoots = batch.renames.map((rename) => rename.previousRelativePath);
  return loadedDirectories.filter((directory) => removedRoots.some(
    (root) => directory === root || directory.startsWith(`${root}/`),
  ));
}

/** Maps a path through the most specific directory or file rename in a batch. */
export function workspacePathAfterRenames(
  path: string,
  renames: readonly ChatWorkspaceRename[],
): string {
  let current = path;
  const visited = new Set([current]);
  for (let index = 0; index < renames.length; index += 1) {
    const rename = matchingRename(current, renames);
    if (!rename) break;
    const next = `${rename.relativePath}${current.slice(rename.previousRelativePath.length)}`;
    if (visited.has(next)) break;
    current = next;
    visited.add(current);
  }
  return current;
}

/** Describes how the selected file view should react to a workspace batch. */
export function workspacePreviewImpact(
  batch: ChatWorkspaceChangeBatch,
  selectedPath: string,
  dirty: boolean,
): ChatWorkspacePreviewImpact {
  const rename = matchingRename(selectedPath, batch.renames);
  if (rename) {
    const renamedPath = workspacePathAfterRenames(selectedPath, batch.renames);
    return dirty
      ? { kind: "deleted_after_rename", relativePath: renamedPath }
      : { kind: "renamed", relativePath: renamedPath };
  }
  if (batch.overflowed || batch.relativePaths.some((path) => pathAffectsFile(path, selectedPath))) {
    return { kind: "refresh" };
  }
  return { kind: "none" };
}

function matchingRename(
  path: string,
  renames: readonly ChatWorkspaceRename[],
): ChatWorkspaceRename | undefined {
  let match: ChatWorkspaceRename | undefined;
  for (const entry of renames) {
    const matches = path === entry.previousRelativePath
      || path.startsWith(`${entry.previousRelativePath}/`);
    if (matches && (!match
      || entry.previousRelativePath.length > match.previousRelativePath.length)) match = entry;
  }
  return match;
}

/** Coalesces queued observer batches while keeping memory and refresh work bounded. */
export function mergeWorkspaceChangeBatches(
  current: ChatWorkspaceChangeBatch | null,
  incoming: ChatWorkspaceChangeBatch,
): ChatWorkspaceChangeBatch {
  if (!current
    || current.generation !== incoming.generation
    || current.workingFolderId !== incoming.workingFolderId
    || current.executionEnvironmentId !== incoming.executionEnvironmentId) return incoming;

  const relativePaths = mergeBoundedStrings(current.relativePaths, incoming.relativePaths);
  const affectedParentDirectories = mergeBoundedStrings(
    current.affectedParentDirectories,
    incoming.affectedParentDirectories,
  );
  const renames = new Map<string, ChatWorkspaceChangeBatch["renames"][number]>();
  for (const rename of [...current.renames, ...incoming.renames]) {
    renames.set(`${rename.previousRelativePath}\0${rename.relativePath}`, rename);
  }
  const pathOverflow = relativePaths.overflowed || affectedParentDirectories.overflowed;
  return {
    ...incoming,
    relativePaths: relativePaths.values,
    affectedParentDirectories: affectedParentDirectories.values,
    renames: [...renames.values()].slice(0, MAX_COALESCED_WORKSPACE_PATHS),
    gitMetadataChanged: current.gitMetadataChanged || incoming.gitMetadataChanged,
    overflowed: current.overflowed
      || incoming.overflowed
      || pathOverflow
      || renames.size > MAX_COALESCED_WORKSPACE_PATHS,
    degradedReason: incoming.degradedReason ?? current.degradedReason,
  };
}

function pathAffectsFile(changedPath: string, selectedPath: string): boolean {
  return changedPath === selectedPath || selectedPath.startsWith(`${changedPath}/`);
}

function mergeBoundedStrings(
  left: readonly string[],
  right: readonly string[],
): { values: string[]; overflowed: boolean } {
  const values = new Set<string>();
  let overflowed = false;
  for (const value of [...left, ...right]) {
    if (values.has(value)) continue;
    if (values.size >= MAX_COALESCED_WORKSPACE_PATHS) {
      overflowed = true;
      continue;
    }
    values.add(value);
  }
  return { values: [...values], overflowed };
}

function compareDirectoryDepth(left: string, right: string): number {
  const leftDepth = left ? left.split("/").length : 0;
  const rightDepth = right ? right.split("/").length : 0;
  return leftDepth - rightDepth || left.localeCompare(right);
}
