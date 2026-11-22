import {
  idleAsyncResource,
  isAsyncResourceLoading,
  startAsyncRequest,
  type AsyncResource,
  type StartedAsyncRequest,
} from "$lib/utils/async-resource";

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

/**
 * Starts reading a file into the editor, dropping the revision of any file shown before so its text never appears
 * under the new file, and superseding any read in flight.
 *
 * @returns The loading document and the token the read must present.
 */
export function startWorkingMarkdownRead<T>(): StartedAsyncRequest<T | null> {
  return startAsyncRequest(idleAsyncResource<T | null>(null));
}

/**
 * Whether a save, refresh, or conflict check that started from the revision `base` may still apply its response.
 * A later read, a file switch, or another write replaces the loaded revision, which makes the response stale.
 *
 * @param document - The editor document.
 * @param base - The revision the operation started from.
 */
export function isCurrentWorkingMarkdownRevision<T>(document: AsyncResource<T | null>, base: T): boolean {
  return !isAsyncResourceLoading(document) && document.data === base;
}

/**
 * Replaces the loaded revision with `next` when `base` is still the loaded revision, or returns `null` for a stale
 * response so the caller leaves the draft alone.
 *
 * @param document - The editor document.
 * @param base - The revision the operation started from.
 * @param next - The revision the operation produced.
 */
export function replaceWorkingMarkdownRevision<T>(
  document: AsyncResource<T | null>,
  base: T,
  next: T,
): AsyncResource<T | null> | null {
  return isCurrentWorkingMarkdownRevision(document, base) ? idleAsyncResource<T | null>(next) : null;
}
