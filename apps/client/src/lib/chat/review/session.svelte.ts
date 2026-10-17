import * as chatApi from "$lib/api/chat";
import type {
  ChatReviewPatchRead,
  ChatReviewSnapshotRead,
  ReviewDiffSource,
} from "$lib/chat/contracts";
import { isWorkspaceRelativeChangedFilePath } from "./changed-files";
import { openChatReviewSingleFlight } from "./prefetch";
import {
  appendReviewPatchPages,
  retainReviewFile,
  reviewSourceKey,
} from "./model";

const REVIEW_REQUEST_TIMEOUT_MS = 30_000;

export interface ReviewSessionScope {
  threadId: string | null;
  workingFolderId: string | null;
  executionEnvironmentId: string | null;
  source: ReviewDiffSource;
  ignoreWhitespace: boolean;
  selectedRelativePath: string | null;
}

export interface ReviewSessionOptions {
  scope: () => ReviewSessionScope;
  hasPendingEdit: () => boolean;
  onSelectedFile: (relativePath: string | null) => void;
  onSourceFallback: (source: ReviewDiffSource) => void;
  onSelectionReset: () => void;
  onError: (reason: unknown) => void;
  timeoutMessage: () => string;
}

/** Owns cancellable Review snapshot and patch requests for one mounted panel. */
export class ChatReviewSession {
  snapshot = $state<ChatReviewSnapshotRead | null>(null);
  patches = $state<ChatReviewPatchRead[]>([]);
  loadingInitial = $state(false);
  refreshing = $state(false);
  loadingPatchIds = $state<string[]>([]);
  refreshPending = $state(false);

  private requestSequence = 0;
  private patchSequence = 0;
  private destroyed = false;

  constructor(private readonly options: ReviewSessionOptions) {}

  cancelLoading(): void {
    this.requestSequence += 1;
    this.patchSequence += 1;
    this.loadingInitial = false;
    this.refreshing = false;
    this.loadingPatchIds = [];
  }

  reset(): void {
    this.cancelLoading();
    this.snapshot = null;
    this.patches = [];
    this.refreshPending = false;
  }

  destroy(): void {
    this.destroyed = true;
    this.cancelLoading();
  }

  markOutdated(): void {
    if (this.snapshot) this.snapshot = { ...this.snapshot, freshness: "outdated" };
  }

  resumePendingRefresh(): void {
    if (!this.refreshPending || this.refreshing || this.options.hasPendingEdit()) return;
    this.refreshPending = false;
    queueMicrotask(() => void this.open(true));
  }

  async open(preserveVisible: boolean): Promise<void> {
    const scope = this.options.scope();
    if (!scope.workingFolderId) return;
    const scopeToken = reviewSessionScopeToken(scope);
    if (preserveVisible && this.options.hasPendingEdit()) {
      this.markOutdated();
      this.refreshPending = true;
      return;
    }
    if (preserveVisible && this.refreshing) {
      this.refreshPending = true;
      return;
    }
    const sequence = ++this.requestSequence;
    if (!this.snapshot) {
      this.loadingInitial = true;
    } else {
      this.refreshing = true;
      this.markOutdated();
    }
    try {
      const next = await this.withTimeout(openChatReviewSingleFlight({
        threadId: scope.threadId,
        workingFolderId: scope.workingFolderId,
        executionEnvironmentId: scope.executionEnvironmentId,
        source: scope.source,
        ignoreWhitespace: scope.ignoreWhitespace,
        contextLines: 3,
        preferredRelativePath: scope.selectedRelativePath
          && isWorkspaceRelativeChangedFilePath(scope.selectedRelativePath)
          ? scope.selectedRelativePath
          : null,
      }));
      if (sequence !== this.requestSequence || !this.scopeMatches(scopeToken)) return;
      this.applySnapshot(next);
    } catch (reason: unknown) {
      if (sequence !== this.requestSequence || !this.scopeMatches(scopeToken)) return;
      const fallback = reviewSourceFallback(reason, scope.source);
      if (fallback) {
        this.options.onSourceFallback(fallback);
        return;
      }
      this.options.onError(reason);
    } finally {
      if (sequence === this.requestSequence && this.scopeMatches(scopeToken)) {
        this.loadingInitial = false;
        this.refreshing = false;
        this.resumePendingRefresh();
      }
    }
  }

  applySnapshot(next: ChatReviewSnapshotRead): void {
    const scope = this.options.scope();
    const currentFileId = retainReviewFile(
      this.snapshot ?? next,
      null,
      scope.selectedRelativePath,
    )?.fileId ?? null;
    const retained = retainReviewFile(next, currentFileId, scope.selectedRelativePath);
    this.snapshot = next;
    this.patches = next.preferredPatch ? [next.preferredPatch] : [];
    this.loadingPatchIds = [];
    this.options.onSelectionReset();
    if (retained?.relativePath !== scope.selectedRelativePath) {
      this.options.onSelectedFile(retained?.relativePath ?? null);
    }
  }

  async ensurePatches(fileIds: readonly string[]): Promise<void> {
    const currentSnapshot = this.snapshot;
    const scope = this.options.scope();
    if (!currentSnapshot || !scope.workingFolderId) return;
    const scopeToken = reviewSessionScopeToken(scope);
    const missing = fileIds.filter((fileId) => (
      !this.patches.some((patch) => patch.fileId === fileId)
      && !this.loadingPatchIds.includes(fileId)
    ));
    if (missing.length === 0) return;
    const sequence = ++this.patchSequence;
    this.loadingPatchIds = [...this.loadingPatchIds, ...missing];
    try {
      for (let index = 0; index < missing.length; index += 8) {
        if (!this.patchRequestMatches(sequence, scopeToken, currentSnapshot.reviewRevision)) return;
        const batch = missing.slice(index, index + 8);
        const page = await this.withTimeout(chatApi.readChatReviewPatches({
          threadId: scope.threadId,
          workingFolderId: scope.workingFolderId,
          executionEnvironmentId: scope.executionEnvironmentId,
          snapshotId: currentSnapshot.snapshotId,
          reviewRevision: currentSnapshot.reviewRevision,
          fileIds: batch,
          continuationCursor: null,
        }));
        if (!this.patchRequestMatches(sequence, scopeToken, currentSnapshot.reviewRevision)) return;
        this.patches = appendReviewPatchPages(this.patches, page.patches);
        this.loadingPatchIds = this.loadingPatchIds.filter((fileId) => !batch.includes(fileId));
        await yieldForReviewRender();
      }
    } catch (reason: unknown) {
      if (sequence === this.patchSequence && this.scopeMatches(scopeToken)) {
        this.options.onError(reason);
      }
    } finally {
      if (sequence === this.patchSequence && this.scopeMatches(scopeToken)) {
        this.loadingPatchIds = this.loadingPatchIds.filter((fileId) => !missing.includes(fileId));
      }
    }
  }

  async loadContinuation(patch: ChatReviewPatchRead): Promise<void> {
    const currentSnapshot = this.snapshot;
    const scope = this.options.scope();
    if (!currentSnapshot || !scope.workingFolderId || !patch.continuationCursor
      || this.loadingPatchIds.includes(patch.fileId)) return;
    const scopeToken = reviewSessionScopeToken(scope);
    this.loadingPatchIds = [...this.loadingPatchIds, patch.fileId];
    try {
      const page = await this.withTimeout(chatApi.readChatReviewPatches({
        threadId: scope.threadId,
        workingFolderId: scope.workingFolderId,
        executionEnvironmentId: scope.executionEnvironmentId,
        snapshotId: currentSnapshot.snapshotId,
        reviewRevision: currentSnapshot.reviewRevision,
        fileIds: [patch.fileId],
        continuationCursor: patch.continuationCursor,
      }));
      if (this.scopeMatches(scopeToken)
        && this.snapshot?.reviewRevision === currentSnapshot.reviewRevision) {
        this.patches = appendReviewPatchPages(this.patches, page.patches);
      }
    } catch (reason: unknown) {
      if (this.scopeMatches(scopeToken)) this.options.onError(reason);
    } finally {
      if (this.scopeMatches(scopeToken)) {
        this.loadingPatchIds = this.loadingPatchIds.filter((fileId) => fileId !== patch.fileId);
      }
    }
  }

  private patchRequestMatches(
    sequence: number,
    scopeToken: string,
    reviewRevision: string,
  ): boolean {
    return sequence === this.patchSequence
      && this.scopeMatches(scopeToken)
      && this.snapshot?.reviewRevision === reviewRevision;
  }

  private scopeMatches(scopeToken: string): boolean {
    return !this.destroyed
      && reviewSessionScopeToken(this.options.scope()) === scopeToken;
  }

  private withTimeout<T>(request: Promise<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      const timeout = window.setTimeout(() => {
        reject(new Error(this.options.timeoutMessage()));
      }, REVIEW_REQUEST_TIMEOUT_MS);
      request.then(
        (value) => {
          window.clearTimeout(timeout);
          resolve(value);
        },
        (reason: unknown) => {
          window.clearTimeout(timeout);
          reject(reason);
        },
      );
    });
  }
}

export function reviewSessionScopeToken(scope: ReviewSessionScope): string {
  return [
    scope.threadId ?? "",
    scope.workingFolderId ?? "",
    scope.executionEnvironmentId ?? "",
    reviewSourceKey(scope.source),
    scope.ignoreWhitespace ? "ignore" : "preserve",
  ].join("\u0000");
}

/** Selects a fallback source only when the requested review source is missing, never for unrelated failures. */
export function reviewSourceFallback(error: unknown, source: ReviewDiffSource): ReviewDiffSource | null {
  if (typeof error !== "object" || error === null || !("code" in error) || error.code !== "not_found"
    || !("details" in error) || typeof error.details !== "object" || error.details === null
    || !("reason" in error.details)) return null;
  if (source.kind === "checkpoint" && error.details.reason === "checkpoint_pair") {
    return source.turnId
      ? { kind: "provider_turn", turnId: source.turnId }
      : { kind: "working_tree", mode: "all" };
  }
  return source.kind === "provider_turn" && error.details.reason === "provider_turn"
    ? { kind: "working_tree", mode: "all" }
    : null;
}

function yieldForReviewRender(): Promise<void> {
  return new Promise((resolve) => {
    if ("requestIdleCallback" in window) {
      window.requestIdleCallback(() => resolve(), { timeout: 50 });
    } else {
      globalThis.setTimeout(resolve, 0);
    }
  });
}
