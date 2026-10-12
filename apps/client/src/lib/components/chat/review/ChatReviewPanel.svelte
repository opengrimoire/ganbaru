<script lang="ts">
  import { onMount, untrack } from "svelte";
  import AlertTriangle from "@lucide/svelte/icons/triangle-alert";
  import Check from "@lucide/svelte/icons/check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FileDiff from "@lucide/svelte/icons/file-diff";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import Minus from "@lucide/svelte/icons/minus";
  import PanelTop from "@lucide/svelte/icons/panel-top";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import Search from "@lucide/svelte/icons/search";
  import Space from "@lucide/svelte/icons/space";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import WrapText from "@lucide/svelte/icons/wrap-text";
  import * as chatApi from "$lib/api/chat";
  import type {
    ChatChangedFileRead,
    ChatReviewCommentRead,
    ChatReviewFileRead,
    ChatReviewPatchRead,
    ReviewDiffSource,
  } from "$lib/chat/contracts";
  import {
    findReviewSearchMatches,
    reviewCommentIsOutdated,
    reviewCommentMatchesSnapshot,
    reviewFileActionVisible,
    reviewFileAsChangedFile,
    reviewSelectionUsesMultipleSides,
    reviewSourceSupportsOperation,
    resolveReviewDiffStyle,
    resolveReviewLayout,
    retainReviewFile,
    reviewSourceKey,
    selectedReviewHunkIds,
    stableReviewHash,
    type ReviewDiffPreference,
    type ReviewLayoutPreference,
  } from "$lib/chat/review/model";
  import {
    ChatReviewSession,
    reviewSessionScopeToken,
    type ReviewSessionScope,
  } from "$lib/chat/review/session.svelte";
  import type { ReviewDiffRenderItem, ReviewLineSelection } from "$lib/chat/review/diff-runtime";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { subscribeChatWorkspaceChanges } from "$lib/chat/workspace/observer-client";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import ChatChangedFileTree from "./ChatChangedFileTree.svelte";
  import ChatFileIcon from "$lib/components/chat/workspace/ChatFileIcon.svelte";
  import ChatPierreDiff, { type ChatDiffViewport } from "./ChatPierreDiff.svelte";

  let {
    active = true,
    source = null,
    sourceThreadId = null,
    sourceWorkingFolderId = null,
    sourceExecutionEnvironmentId = null,
    selectedFile = null,
    layoutPreference = "auto",
    whitespaceIgnored = false,
    diffView = "auto",
    onStateChange = () => {},
  }: {
    active?: boolean;
    source?: ReviewDiffSource | null;
    sourceThreadId?: string | null;
    sourceWorkingFolderId?: string | null;
    sourceExecutionEnvironmentId?: string | null;
    selectedFile?: string | null;
    layoutPreference?: ReviewLayoutPreference;
    whitespaceIgnored?: boolean;
    diffView?: ReviewDiffPreference;
    onStateChange?: (update: {
      source?: ReviewDiffSource;
      selectedFile?: string | null;
      layoutPreference?: ReviewLayoutPreference;
      whitespaceIgnored?: boolean;
      diffView?: ReviewDiffPreference;
    }) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  let panel: HTMLElement | undefined = $state();
  let content: HTMLElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let panelWidth = $state(0);
  let contentWidth = $state(0);
  let comments: ChatReviewCommentRead[] = $state([]);
  let includeResolved = $state(false);
  let commentsVisible = $state(false);
  let fileFilter = $state("");
  let diffQuery = $state("");
  let settledDiffQuery = $state("");
  let matchIndex = $state(0);
  let selection: ReviewLineSelection | null = $state(null);
  let commentDraft = $state("");
  let error = $state<string | null>(null);
  let busyAction: string | null = $state(null);
  let busyCommentId: string | null = $state(null);
  let attachingCommentIds: string[] = $state([]);
  let viewport: ChatDiffViewport | null = null;
  let loadedKey = "";
  let commentsKey = "";
  let commentsRequest = 0;
  let wrap = $state(false);
  let workspaceRefreshTimer: number | null = null;
  let workspaceGeneration: number | null = null;
  let workspaceScopeKey = "";
  let commentAttachmentIds: Record<string, string> = $state({});
  let destroyed = false;

  const threadId = $derived(sourceThreadId ?? chat.selectedThreadId);
  const workingFolderId = $derived(sourceThreadId
    ? sourceWorkingFolderId
    : chat.selectedWorkingFolderId ?? chat.draftWorkingFolderId);
  const executionEnvironmentId = $derived(sourceThreadId
    ? sourceExecutionEnvironmentId
    : chat.selectedExecutionEnvironmentId);
  const effectiveSource = $derived.by<ReviewDiffSource>(() => source
    ?? (threadId
      ? { kind: "checkpoint", range: "turn", turnId: null }
      : { kind: "working_tree", mode: "all" }));
  const reviewSourceOptions = $derived.by(() => {
    const options: { value: string; label: string }[] = [];
    const currentKey = reviewSourceKey(effectiveSource);
    if (effectiveSource.kind === "checkpoint" && effectiveSource.range === "turn" && effectiveSource.turnId) {
      options.push({ value: currentKey, label: sourceOptionLabel(effectiveSource) });
    }
    if (threadId) {
      options.push({ value: "checkpoint:turn:", label: t("chat.inspector.currentTurn") });
      options.push({ value: "checkpoint:thread:", label: t("chat.inspector.entireThread") });
    }
    options.push(
      { value: "working_tree:staged", label: t("chat.review.stagedChanges") },
      { value: "working_tree:unstaged", label: t("chat.review.unstagedChanges") },
      { value: "working_tree:all", label: t("chat.review.allChanges") },
    );
    if (effectiveSource.kind !== "checkpoint" && effectiveSource.kind !== "working_tree") {
      options.push({ value: currentKey, label: sourceOptionLabel(effectiveSource) });
    }
    return options;
  });
  const reviewSession = new ChatReviewSession({
    scope: currentReviewScope,
    hasPendingEdit: () => Boolean(commentDraft.trim() || selection),
    onSelectedFile: (relativePath) => onStateChange({ selectedFile: relativePath }),
    onSourceFallback: chooseSource,
    onSelectionReset: () => {
      selection = null;
      commentDraft = "";
      viewport?.clearSelection();
    },
    onError: (reason) => { error = message(reason); },
    timeoutMessage: () => t("chat.review.loadTimedOut"),
  });
  const snapshot = $derived(reviewSession.snapshot);
  const patches = $derived(reviewSession.patches);
  const loadingInitial = $derived(reviewSession.loadingInitial);
  const refreshing = $derived(reviewSession.refreshing);
  const loadingPatchIds = $derived(reviewSession.loadingPatchIds);
  const selectedReviewFile = $derived(snapshot ? retainReviewFile(snapshot, null, selectedFile) : null);
  const resolvedLayout = $derived(resolveReviewLayout(
    layoutPreference,
    panelWidth,
    snapshot?.totals.files ?? 0,
    (snapshot?.totals.additions ?? 0) + (snapshot?.totals.deletions ?? 0),
  ));
  const resolvedDiffStyle = $derived(resolveReviewDiffStyle(diffView, contentWidth));
  const filteredFiles = $derived.by(() => {
    const query = fileFilter.trim().toLocaleLowerCase();
    const currentSnapshot = snapshot;
    return query && currentSnapshot
      ? currentSnapshot.files.filter((file) => file.relativePath.toLocaleLowerCase().includes(query))
      : currentSnapshot?.files ?? [];
  });
  const changedFiles = $derived(filteredFiles.map(reviewFileAsChangedFile));
  const visibleComments = $derived(includeResolved ? comments : comments.filter((comment) => comment.state === "open"));
  const renderComments = $derived.by(() => {
    const currentSnapshot = snapshot;
    if (!currentSnapshot) return [];
    const stateVisible = includeResolved ? comments : comments.filter((comment) => comment.state === "open");
    return stateVisible.filter((comment) => reviewCommentMatchesSnapshot(comment, currentSnapshot));
  });
  const visiblePatches = $derived.by(() => resolvedLayout === "continuous"
    ? patches
    : patches.filter((patch) => patch.fileId === selectedReviewFile?.fileId));
  const renderItems = $derived.by<ReviewDiffRenderItem[]>(() => {
    const currentSnapshot = snapshot;
    if (!currentSnapshot) return [];
    const itemList: ReviewDiffRenderItem[] = [];
    const filesToRender = resolvedLayout === "continuous"
      ? filteredFiles
      : selectedReviewFile ? [selectedReviewFile] : [];
    for (const file of filesToRender) {
      if (resolvedLayout === "file" && file.fileId !== selectedReviewFile?.fileId) continue;
      const filePatches = visiblePatches.filter((patch) => patch.fileId === file.fileId && patch.patch);
      filePatches.forEach((patch, pageIndex) => {
        const fileComments = renderComments.filter((comment) => comment.relativePath === file.relativePath);
        const patchVersion = stableReviewHash(
          `${currentSnapshot.reviewRevision}\0${file.fileId}\0${pageIndex}\0${patch.patch?.length ?? 0}`,
        );
        itemList.push({
          key: `${currentSnapshot.reviewRevision}:${file.fileId}:${pageIndex}`,
          fileId: file.fileId,
          fileName: file.relativePath,
          previousFileName: file.previousRelativePath,
          patch: patch.patch ?? "",
          pageIndex,
          comments: fileComments,
          patchVersion,
          version: stableReviewHash(`${patchVersion}\0${fileComments.map((comment) => `${comment.id}:${comment.updatedAt}`).join("\0")}`),
        });
      });
    }
    return itemList;
  });
  const searchMatches = $derived(findReviewSearchMatches(snapshot?.files ?? [], patches, settledDiffQuery));
  const selectedPatch = $derived.by<ChatReviewPatchRead | null>(() => {
    if (!selectedReviewFile) return null;
    const pages = patches.filter((patch) => patch.fileId === selectedReviewFile.fileId);
    const first = pages[0];
    return first ? { ...first, hunks: pages.flatMap((page) => page.hunks) } : null;
  });
  /** True when the diff area has nothing else to show, so the load failure takes its place. */
  const reviewContentFailed = $derived(
    Boolean(error)
      && renderItems.length === 0
      && !selectedReviewFile?.flags.binary
      && selectedPatch?.state !== "oversized_hunk"
      && selectedPatch?.state !== "unavailable"
      && !(selectedReviewFile && loadingPatchIds.includes(selectedReviewFile.fileId))
      && !loadingInitial,
  );
  const selectionHunkIds = $derived.by(() => {
    const currentSelection = selection as ReviewLineSelection | null;
    return selectedReviewHunkIds(
      currentSelection?.fileId === selectedReviewFile?.fileId ? selectedPatch : null,
      currentSelection?.range ?? null,
    );
  });
  const selectionCrossesSides = $derived.by(() => reviewSelectionUsesMultipleSides(
    selection as ReviewLineSelection | null,
  ));
  const actionsReady = $derived(Boolean(
    snapshot && snapshot.freshness === "current" && !refreshing && !commentDraft.trim(),
  ));
  const canStageScope = $derived(reviewSourceSupportsOperation(snapshot, "stage"));
  const canUnstageScope = $derived(reviewSourceSupportsOperation(snapshot, "unstage"));
  const canDiscardScope = $derived(reviewSourceSupportsOperation(snapshot, "discard"));

  onMount(() => {
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.target === panel) panelWidth = entry.contentRect.width;
        if (entry.target === content) contentWidth = entry.contentRect.width;
      }
    });
    if (panel) observer.observe(panel);
    if (content) observer.observe(content);
    return () => {
      destroyed = true;
      reviewSession.destroy();
      observer.disconnect();
      if (workspaceRefreshTimer !== null) window.clearTimeout(workspaceRefreshTimer);
    };
  });

  $effect(() => {
    if (!active) return;
    const unsubscribe = subscribeChatWorkspaceChanges((batch) => {
      if (effectiveSource.kind !== "working_tree"
        || batch.workingFolderId !== workingFolderId
        || batch.executionEnvironmentId !== executionEnvironmentId
        || (workspaceGeneration !== null && batch.generation < workspaceGeneration)
        || (batch.relativePaths.length === 0 && !batch.gitMetadataChanged && !batch.overflowed)) return;
      workspaceGeneration = batch.generation;
      reviewSession.markOutdated();
      if (workspaceRefreshTimer !== null) window.clearTimeout(workspaceRefreshTimer);
      workspaceRefreshTimer = window.setTimeout(() => {
        workspaceRefreshTimer = null;
        void openReview(true);
      }, 100);
    });
    return () => {
      unsubscribe();
      if (workspaceRefreshTimer !== null) {
        window.clearTimeout(workspaceRefreshTimer);
        workspaceRefreshTimer = null;
      }
    };
  });

  $effect(() => {
    const folder = workingFolderId;
    const sourceKey = reviewSourceKey(effectiveSource);
    const persistedThreadId = threadId;
    const environmentId = executionEnvironmentId;
    const ignoreWhitespace = whitespaceIgnored;
    const isActive = active;
    untrack(() => {
      const nextWorkspaceScopeKey = `${folder ?? ""}:${environmentId ?? ""}`;
      if (nextWorkspaceScopeKey !== workspaceScopeKey) {
        workspaceScopeKey = nextWorkspaceScopeKey;
        workspaceGeneration = null;
      }
      if (!isActive) {
        loadedKey = "";
        reviewSession.cancelLoading();
        return;
      }
      const nextKey = `${persistedThreadId ?? ""}:${folder ?? ""}:${environmentId ?? ""}:${sourceKey}:${ignoreWhitespace}`;
      if (nextKey === loadedKey) return;
      loadedKey = nextKey;
      reviewSession.cancelLoading();
      commentsRequest += 1;
      busyAction = null;
      busyCommentId = null;
      attachingCommentIds = [];
      commentAttachmentIds = {};
      if (!folder) {
        reviewSession.reset();
        selection = null;
        commentDraft = "";
        return;
      }
      void openReview(snapshot !== null);
    });
  });

  $effect(() => {
    const thread = threadId;
    const nextKey = `${thread ?? ""}:${includeResolved}`;
    const isActive = active;
    untrack(() => {
      if (!isActive) {
        commentsKey = "";
        commentsRequest += 1;
        return;
      }
      if (nextKey === commentsKey) return;
      commentsKey = nextKey;
      const request = ++commentsRequest;
      if (!thread) {
        comments = [];
        return;
      }
      void loadComments(thread, nextKey, request);
    });
  });

  $effect(() => {
    const currentSnapshot = snapshot;
    const currentFile = selectedReviewFile;
    const layout = resolvedLayout;
    if (!active || !currentSnapshot || !currentFile) return;
    const fileIds = layout === "file"
      ? [currentFile.fileId]
      : filteredFiles.map((file) => file.fileId);
    untrack(() => void reviewSession.ensurePatches(fileIds));
  });

  $effect(() => {
    const matchCount = searchMatches.length;
    if (matchCount === 0) {
      if (matchIndex !== 0) matchIndex = 0;
      return;
    }
    if (matchIndex >= matchCount) matchIndex = matchCount - 1;
  });

  $effect(() => {
    const query = diffQuery;
    const timer = window.setTimeout(() => {
      settledDiffQuery = query;
      matchIndex = 0;
    }, 140);
    return () => window.clearTimeout(timer);
  });

  $effect(() => {
    if (active && reviewSession.refreshPending && !commentDraft.trim() && !selection) {
      reviewSession.resumePendingRefresh();
    }
  });

  function openReview(preserveVisible: boolean): Promise<void> {
    error = null;
    return reviewSession.open(preserveVisible);
  }

  async function retryReview(): Promise<void> {
    await openReview(true);
    if (error) throw new Error(error);
  }

  async function loadComments(persistedThreadId: string, key: string, request: number): Promise<void> {
    try {
      const nextComments = await chatApi.listChatReviewComments(persistedThreadId, includeResolved);
      if (!destroyed && request === commentsRequest && commentsKey === key && threadId === persistedThreadId) {
        comments = nextComments;
      }
    } catch (reason: unknown) {
      if (!destroyed && request === commentsRequest && commentsKey === key && threadId === persistedThreadId) {
        error = message(reason);
      }
    }
  }

  function chooseSource(next: ReviewDiffSource): void {
    onStateChange({ source: next, selectedFile: null });
  }

  /**
   * Applies a diff source picked by its option key; keys for the current custom source leave it unchanged.
   *
   * @param value Option key produced by `reviewSourceKey` or one of the fixed source keys.
   */
  function chooseSourceKey(value: string): void {
    if (value === "checkpoint:turn:") chooseSource({ kind: "checkpoint", range: "turn", turnId: null });
    else if (value === "checkpoint:thread:") chooseSource({ kind: "checkpoint", range: "thread", turnId: null });
    else if (value === "working_tree:staged") chooseSource({ kind: "working_tree", mode: "staged" });
    else if (value === "working_tree:unstaged") chooseSource({ kind: "working_tree", mode: "unstaged" });
    else if (value === "working_tree:all") chooseSource({ kind: "working_tree", mode: "all" });
  }

  function selectFile(file: ChatChangedFileRead): void {
    const reviewFile = snapshot?.files.find((candidate) => candidate.relativePath === file.relativePath);
    if (!reviewFile) return;
    activateFile(reviewFile.fileId);
  }

  function activateFile(fileId: string, scrollIntoView = true): void {
    const file = snapshot?.files.find((candidate) => candidate.fileId === fileId);
    if (!file) return;
    if (file.relativePath !== selectedFile) onStateChange({ selectedFile: file.relativePath });
    if (scrollIntoView && resolvedLayout === "continuous") viewport?.scrollToFile(fileId);
  }

  function openInEditor(file: ChatReviewFileRead): void {
    if (commentDraft.trim()) return;
    window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-open-file", { detail: { relativePath: file.relativePath } }));
  }

  function stepFile(direction: -1 | 1): void {
    if (filteredFiles.length === 0) return;
    const current = selectedReviewFile ? filteredFiles.findIndex((file) => file.fileId === selectedReviewFile.fileId) : -1;
    const base = current >= 0 ? current : direction > 0 ? -1 : 0;
    const next = (base + direction + filteredFiles.length) % filteredFiles.length;
    const file = filteredFiles[next];
    if (file) activateFile(file.fileId);
  }

  function stepMatch(direction: -1 | 1): void {
    if (searchMatches.length === 0) return;
    matchIndex = (matchIndex + direction + searchMatches.length) % searchMatches.length;
    const match = searchMatches[matchIndex];
    if (!match) return;
    activateFile(match.fileId, false);
    requestAnimationFrame(() => viewport?.scrollToLine(match.fileId, match.lineNumber, match.side));
  }

  async function applyAction(
    operation: "stage" | "unstage" | "discard",
    file: ChatReviewFileRead | null,
    hunkIds: string[] = [],
  ): Promise<void> {
    const currentSnapshot = snapshot;
    const folder = workingFolderId;
    if (!currentSnapshot || !folder || busyAction || !actionsReady) return;
    const scope = reviewScopeToken();
    const confirmed = operation !== "discard" || window.confirm(
      file ? t("chat.sourceControl.confirmDiscard", file.relativePath) : t("chat.review.confirmDiscardAll"),
    );
    if (!confirmed) return;
    const actionId = `${operation}:${file?.fileId ?? "all"}:${crypto.randomUUID()}`;
    busyAction = actionId;
    error = null;
    try {
      const result = await chatApi.applyChatReviewAction({
        threadId,
        workingFolderId: folder,
        executionEnvironmentId,
        snapshotId: currentSnapshot.snapshotId,
        expectedReviewRevision: currentSnapshot.reviewRevision,
        operation,
        fileId: file?.fileId ?? null,
        hunkIds,
        confirmed: operation === "discard",
        clientOperationId: crypto.randomUUID(),
      });
      if (reviewScopeMatches(scope)) reviewSession.applySnapshot(result.snapshot);
    } catch (reason: unknown) {
      if (reviewScopeMatches(scope)) {
        error = message(reason);
        await openReview(true);
      }
    } finally {
      if (busyAction === actionId && reviewScopeMatches(scope)) busyAction = null;
    }
  }

  async function createComment(): Promise<void> {
    const currentSelection = selection;
    const currentSnapshot = snapshot;
    const file = currentSelection
      ? currentSnapshot?.files.find((candidate) => candidate.fileId === currentSelection.fileId)
      : null;
    if (!threadId || !currentSnapshot || !currentSelection || !file || !commentDraft.trim() || busyCommentId || reviewSelectionUsesMultipleSides(currentSelection)) return;
    const scope = reviewScopeToken();
    const persistedThreadId = threadId;
    const commentId = crypto.randomUUID();
    busyCommentId = commentId;
    error = null;
    try {
      const side = currentSelection.range.side ?? "additions";
      const startLine = Math.min(currentSelection.range.start, currentSelection.range.end);
      const endLine = Math.max(currentSelection.range.start, currentSelection.range.end);
      const created = await chatApi.createChatReviewComment({
        id: commentId,
        threadId: persistedThreadId,
        relativePath: file.relativePath,
        contentRevision: currentSnapshot.reviewRevision,
        startLine,
        startColumn: 1,
        endLine,
        endColumn: 1,
        selectedText: "",
        commentText: commentDraft.trim(),
        sourceKind: currentSnapshot.source.kind,
        sourceData: currentSnapshot.source,
        snapshotId: currentSnapshot.snapshotId,
        reviewRevision: currentSnapshot.reviewRevision,
        fileId: file.fileId,
        selectionSide: side === "deletions" ? "old" : "new",
        previousRelativePath: file.previousRelativePath,
      });
      if (!reviewScopeMatches(scope) || threadId !== persistedThreadId) return;
      comments = [...comments, created];
      await attachComment(created, scope);
      if (!reviewScopeMatches(scope)) return;
      commentDraft = "";
      selection = null;
      viewport?.clearSelection();
    } catch (reason: unknown) {
      if (reviewScopeMatches(scope)) error = message(reason);
    } finally {
      if (busyCommentId === commentId && reviewScopeMatches(scope)) busyCommentId = null;
    }
  }

  async function attachComment(
    comment: ChatReviewCommentRead,
    expectedScope = reviewScopeToken(),
  ): Promise<void> {
    if (!threadId || attachingCommentIds.includes(comment.id)) return;
    const persistedThreadId = threadId;
    const deterministicId = `review-context:${comment.id}`;
    const knownAttachmentId = commentAttachmentIds[comment.id]
      ?? (chat.composer.attachmentIds.includes(deterministicId) ? deterministicId : null);
    if (knownAttachmentId) {
      if (!reviewScopeMatches(expectedScope) || threadId !== persistedThreadId) return;
      if (!chat.composer.attachmentIds.includes(knownAttachmentId)) {
        chat.setComposerAttachments([...chat.composer.attachmentIds, knownAttachmentId]);
      }
      return;
    }
    attachingCommentIds = [...attachingCommentIds, comment.id];
    try {
      const attachment = await chatApi.attachChatReviewComment(persistedThreadId, comment.id, deterministicId);
      if (!reviewScopeMatches(expectedScope) || threadId !== persistedThreadId) return;
      commentAttachmentIds = { ...commentAttachmentIds, [comment.id]: attachment.id };
      if (!chat.composer.attachmentIds.includes(attachment.id)) {
        chat.setComposerAttachments([...chat.composer.attachmentIds, attachment.id]);
      }
    } catch (reason: unknown) {
      if (reviewScopeMatches(expectedScope) && threadId === persistedThreadId) error = message(reason);
    } finally {
      attachingCommentIds = attachingCommentIds.filter((commentId) => commentId !== comment.id);
    }
  }

  async function setCommentResolved(comment: ChatReviewCommentRead, resolved: boolean): Promise<void> {
    if (!threadId || busyCommentId || attachingCommentIds.includes(comment.id)) return;
    const scope = reviewScopeToken();
    const persistedThreadId = threadId;
    const operationId = `${comment.id}:${resolved}:${crypto.randomUUID()}`;
    busyCommentId = operationId;
    try {
      const updated = await chatApi.setChatReviewCommentResolved(persistedThreadId, comment.id, resolved);
      if (!reviewScopeMatches(scope) || threadId !== persistedThreadId) return;
      comments = comments.map((entry) => entry.id === updated.id ? updated : entry);
      if (resolved) {
        const attachmentId = commentAttachmentIds[comment.id]
          ?? `review-context:${comment.id}`;
        if (chat.composer.attachmentIds.includes(attachmentId)) {
          chat.setComposerAttachments(chat.composer.attachmentIds.filter((id) => id !== attachmentId));
        }
        const nextAttachmentIds = { ...commentAttachmentIds };
        delete nextAttachmentIds[comment.id];
        commentAttachmentIds = nextAttachmentIds;
      }
    } catch (reason: unknown) {
      if (reviewScopeMatches(scope) && threadId === persistedThreadId) error = message(reason);
    } finally {
      if (busyCommentId === operationId && reviewScopeMatches(scope)) busyCommentId = null;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (!panel || !event.composedPath().includes(panel)) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "f") {
      event.preventDefault();
      searchInput?.focus();
      searchInput?.select();
      return;
    }
    if (event.key !== "F7") return;
    event.preventDefault();
    if (diffQuery.trim()) stepMatch(event.shiftKey ? -1 : 1);
    else stepFile(event.shiftKey ? -1 : 1);
  }

  function sourceOptionLabel(value: ReviewDiffSource): string {
    if (value.kind === "working_tree") {
      if (value.mode === "staged") return t("chat.review.stagedChanges");
      if (value.mode === "unstaged") return t("chat.review.unstagedChanges");
      return t("chat.review.allChanges");
    }
    if (value.kind === "checkpoint") return value.range === "thread"
      ? t("chat.inspector.entireThread")
      : t("chat.inspector.currentTurn");
    if (value.kind === "commit") return t("chat.review.commitSource", value.revision);
    if (value.kind === "branch") return t("chat.review.branchSource", value.headRef);
    if (value.kind === "provider_turn") return t("chat.review.providerTurnSource");
    return t("chat.review.changeRequestSource", value.number);
  }

  function lastPatchForFile(fileId: string): ChatReviewPatchRead | null {
    return patches.filter((patch) => patch.fileId === fileId).at(-1) ?? null;
  }

  function currentReviewScope(): ReviewSessionScope {
    return {
      threadId,
      workingFolderId,
      executionEnvironmentId,
      source: effectiveSource,
      ignoreWhitespace: whitespaceIgnored,
      selectedRelativePath: selectedFile,
    };
  }

  function reviewScopeToken(): string {
    return reviewSessionScopeToken(currentReviewScope());
  }

  function reviewScopeMatches(scope: string): boolean {
    return !destroyed && reviewScopeToken() === scope;
  }

  function message(reason: unknown): string {
    if (reason instanceof Error) return reason.message;
    if (typeof reason === "object" && reason !== null && !Array.isArray(reason)) {
      const value = (reason as Record<string, unknown>).message;
      if (typeof value === "string") return value;
    }
    return t("chat.review.reviewUnavailable");
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<section bind:this={panel} class="review-panel" aria-label={t("chat.inspector.review")}>
  <header class="review-toolbar">
    <Select
      inline
      class="source-picker"
      ariaLabel={t("chat.review.diffSource")}
      value={reviewSourceKey(effectiveSource)}
      options={reviewSourceOptions}
      onChange={chooseSourceKey}
    />

    <div class="toolbar-group" role="group" aria-label={t("chat.review.layout")}>
      <button type="button" class:active={resolvedLayout === "continuous"} aria-pressed={resolvedLayout === "continuous"} aria-label={t("chat.review.continuousLayout")} title={t("chat.review.continuousLayout")} onclick={() => onStateChange({ layoutPreference: resolvedLayout === "continuous" ? "file" : "continuous" })}><PanelTop size={13} /></button>
      <button type="button" class:active={resolvedDiffStyle === "split"} aria-pressed={resolvedDiffStyle === "split"} aria-label={t("chat.inspector.splitDiff")} title={resolvedDiffStyle === "split" ? t("chat.inspector.unifiedDiff") : t("chat.inspector.splitDiff")} onclick={() => onStateChange({ diffView: resolvedDiffStyle === "split" ? "unified" : "split" })}>{#if resolvedDiffStyle === "split"}<Rows3 size={13} />{:else}<Columns2 size={13} />{/if}</button>
      <button type="button" class:active={wrap} aria-pressed={wrap} aria-label={t("chat.review.wrapLines")} title={t("chat.review.wrapLines")} onclick={() => { wrap = !wrap; }}><WrapText size={13} /></button>
      <button type="button" class:active={whitespaceIgnored} aria-pressed={whitespaceIgnored} aria-label={t("chat.inspector.ignoreWhitespace")} title={t("chat.inspector.ignoreWhitespace")} onclick={() => onStateChange({ whitespaceIgnored: !whitespaceIgnored })}><Space size={13} /></button>
    </div>

    {#if canStageScope || canUnstageScope || canDiscardScope}
      <div class="scope-actions" role="group" aria-label={t("chat.review.scopeActions")}>
        {#if canUnstageScope}<button type="button" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("unstage", null)}><Minus size={12} /><span>{t("chat.review.unstageAll")}</span></button>{/if}
        {#if canStageScope}<button type="button" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("stage", null)}><Plus size={12} /><span>{t("chat.review.stageAll")}</span></button>{/if}
        {#if canDiscardScope}<button type="button" class="destructive" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("discard", null)}><Trash2 size={12} /><span>{t("chat.review.discardAll")}</span></button>{/if}
      </div>
    {/if}

    {#if snapshot}
      <span class="diff-stats" aria-label={t("chat.review.changeTotals", snapshot.totals.additions, snapshot.totals.deletions)}>
        <span>+{formatNumber(localization.locale, snapshot.totals.additions)}</span>
        <span>−{formatNumber(localization.locale, snapshot.totals.deletions)}</span>
      </span>
    {/if}
    <button type="button" class="chat-icon-button" class:spinning={refreshing} disabled={refreshing} aria-label={t("chat.review.refresh")} title={t("chat.review.refresh")} onclick={() => void openReview(true)}><RefreshCw size={13} /></button>
    <button type="button" class="comments-toggle" class:active={commentsVisible} aria-pressed={commentsVisible} aria-label={t("chat.review.comments", visibleComments.length)} title={t("chat.review.comments", visibleComments.length)} onclick={() => { commentsVisible = !commentsVisible; }}><MessagesSquare size={13} /><span>{formatNumber(localization.locale, visibleComments.length)}</span></button>
  </header>

  <div class="search-toolbar">
    <label><ListFilter size={12} /><input type="search" bind:value={fileFilter} placeholder={t("chat.review.filterFiles")} aria-label={t("chat.review.filterFiles")} /></label>
    <label><Search size={12} /><input bind:this={searchInput} type="search" bind:value={diffQuery} placeholder={t("chat.review.searchDiff")} aria-label={t("chat.review.searchDiff")} oninput={() => { matchIndex = 0; }} /></label>
    {#if diffQuery.trim()}
      <span>{searchMatches.length ? `${matchIndex + 1}/${searchMatches.length}` : "0/0"}</span>
      <button type="button" disabled={searchMatches.length === 0} aria-label={t("chat.review.previousMatch")} title={t("chat.review.previousMatch")} onclick={() => stepMatch(-1)}><ChevronLeft size={12} /></button>
      <button type="button" disabled={searchMatches.length === 0} aria-label={t("chat.review.nextMatch")} title={t("chat.review.nextMatch")} onclick={() => stepMatch(1)}><ChevronRight size={12} /></button>
    {/if}
  </div>

  {#if error && !reviewContentFailed}<Notice class="review-error" message={error} onRetry={retryReview} />{/if}
  {#if snapshot?.freshness === "outdated" || refreshing}
    <p role="status" class="freshness"><RefreshCw size={12} class={refreshing ? "spinning" : undefined} />{refreshing ? t("chat.review.updating") : t("chat.review.outdated")}</p>
  {/if}

  <div class="review-workspace" class:comments-open={commentsVisible}>
    <div class="review-main">
      <aside class="review-file-list" aria-label={t("chat.review.changedFiles")}>
        {#if loadingInitial && !snapshot}
          <p>{t("common.loading")}</p>
        {:else if changedFiles.length > 0}
          <ChatChangedFileTree files={changedFiles} selectedFile={selectedReviewFile?.relativePath ?? null} onSelect={selectFile} />
        {:else if error && !snapshot}
          <p>{t("chat.review.reviewUnavailable")}</p>
        {:else}
          <p>{t("chat.inspector.noChanges")}</p>
        {/if}
      </aside>

      <main bind:this={content} class="review-content">
        {#if selectedReviewFile}
          <header class="file-toolbar">
            <button type="button" aria-label={t("chat.review.previousFile")} title={t("chat.review.previousFile")} onclick={() => stepFile(-1)}><ChevronLeft size={13} /></button>
            <button type="button" aria-label={t("chat.review.nextFile")} title={t("chat.review.nextFile")} onclick={() => stepFile(1)}><ChevronRight size={13} /></button>
            <ChatFileIcon path={selectedReviewFile.relativePath} />
            <strong title={selectedReviewFile.relativePath}>{selectedReviewFile.relativePath}</strong>
            {#if selectedReviewFile.flags.readOnly}<span class="read-only">{t("chat.review.readOnly")}</span>{/if}
            <button type="button" class="chat-icon-button" aria-label={t("chat.inspector.copyPath")} title={t("chat.inspector.copyPath")} onclick={() => void writeTextToClipboard(selectedReviewFile.relativePath).catch((reason) => { error = message(reason); })}><Copy size={13} /></button>
            {#if reviewFileActionVisible(selectedReviewFile, "openEditor")}<button type="button" class="chat-icon-button" disabled={refreshing || Boolean(commentDraft.trim())} aria-disabled={!selectedReviewFile.capabilities.openEditor || refreshing || Boolean(commentDraft.trim())} aria-label={t("chat.review.openInEditor")} title={selectedReviewFile.capabilityReasons.openEditor ?? t("chat.review.openInEditor")} onclick={() => { if (selectedReviewFile.capabilities.openEditor) openInEditor(selectedReviewFile); }}><ExternalLink size={13} /></button>{/if}
            {#if reviewFileActionVisible(selectedReviewFile, "unstage")}<button type="button" class="chat-icon-button" disabled={busyAction !== null || !actionsReady} aria-disabled={!selectedReviewFile.capabilities.unstage || busyAction !== null || !actionsReady} aria-label={t("chat.sourceControl.unstageFile", selectedReviewFile.relativePath)} title={selectedReviewFile.capabilityReasons.unstage ?? t("chat.sourceControl.unstageFile", selectedReviewFile.relativePath)} onclick={() => { if (selectedReviewFile.capabilities.unstage) void applyAction("unstage", selectedReviewFile); }}><Minus size={13} /></button>{/if}
            {#if reviewFileActionVisible(selectedReviewFile, "stage")}<button type="button" class="chat-icon-button" disabled={busyAction !== null || !actionsReady} aria-disabled={!selectedReviewFile.capabilities.stage || busyAction !== null || !actionsReady} aria-label={t("chat.sourceControl.stageFile", selectedReviewFile.relativePath)} title={selectedReviewFile.capabilityReasons.stage ?? t("chat.sourceControl.stageFile", selectedReviewFile.relativePath)} onclick={() => { if (selectedReviewFile.capabilities.stage) void applyAction("stage", selectedReviewFile); }}><Plus size={13} /></button>{/if}
            {#if reviewFileActionVisible(selectedReviewFile, "discard")}<button type="button" class="chat-icon-button destructive" disabled={busyAction !== null || !actionsReady} aria-disabled={!selectedReviewFile.capabilities.discard || busyAction !== null || !actionsReady} aria-label={t("chat.sourceControl.discardFile", selectedReviewFile.relativePath)} title={selectedReviewFile.capabilityReasons.discard ?? t("chat.sourceControl.discardFile", selectedReviewFile.relativePath)} onclick={() => { if (selectedReviewFile.capabilities.discard) void applyAction("discard", selectedReviewFile); }}><Trash2 size={13} /></button>{/if}
          </header>
        {/if}

        {#if selection && selectedReviewFile && selection.fileId === selectedReviewFile.fileId}
          <form class="comment-composer" onsubmit={(event) => { event.preventDefault(); void createComment(); }}>
            <div>
              <MessageSquarePlus size={13} />
              <strong>{t("chat.review.commentOnSelection", Math.min(selection.range.start, selection.range.end), Math.max(selection.range.start, selection.range.end))}</strong>
              {#if selectionHunkIds.length > 0 && selectedReviewFile.capabilities.unstage}<button type="button" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("unstage", selectedReviewFile, selectionHunkIds)}><Minus size={12} />{t("chat.review.unstageHunks", selectionHunkIds.length)}</button>{/if}
              {#if selectionHunkIds.length > 0 && selectedReviewFile.capabilities.stage}<button type="button" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("stage", selectedReviewFile, selectionHunkIds)}><Plus size={12} />{t("chat.review.stageHunks", selectionHunkIds.length)}</button>{/if}
              {#if selectionHunkIds.length > 0 && selectedReviewFile.capabilities.discard}<button type="button" class="destructive" disabled={!actionsReady || busyAction !== null} onclick={() => void applyAction("discard", selectedReviewFile, selectionHunkIds)}><Trash2 size={12} />{t("chat.review.discardHunks", selectionHunkIds.length)}</button>{/if}
            </div>
            {#if selectionCrossesSides}
              <p role="status">{t("chat.review.crossSideCommentUnavailable")}</p>
            {:else if threadId && selectedReviewFile.capabilities.comment}
              <textarea class="field" bind:value={commentDraft} maxlength="65536" rows="3" placeholder={t("chat.review.commentPlaceholder")} aria-label={t("chat.review.commentPlaceholder")}></textarea>
              <footer>
                <button type="button" onclick={() => { selection = null; commentDraft = ""; viewport?.clearSelection(); }}>{t("common.cancel")}</button>
                <button type="submit" class="primary" disabled={!commentDraft.trim() || busyCommentId !== null}>{t("chat.review.add")}</button>
              </footer>
            {:else}
              <p>{threadId ? selectedReviewFile.capabilityReasons.comment ?? t("chat.review.commentsUnavailable") : t("chat.review.startThreadToComment")}</p>
            {/if}
          </form>
        {/if}

        {#if renderItems.length > 0}
          <ChatPierreDiff
            {active}
            items={renderItems}
            selectedFileId={selectedReviewFile?.fileId ?? null}
            diffStyle={resolvedDiffStyle}
            {wrap}
            labels={{ attach: t("chat.review.attach"), resolve: t("chat.review.resolve"), reopen: t("chat.review.reopen"), outdated: t("chat.review.outdated") }}
            fallbackLabel={t("chat.review.plainTextFallback")}
            rawLoadMoreLabel={t("chat.review.loadMoreRawDiff")}
            onSelection={(next) => {
              selection = next;
              if (next) activateFile(next.fileId, false);
              if (!next || reviewSelectionUsesMultipleSides(next)) commentDraft = "";
            }}
            onActiveFile={(fileId) => activateFile(fileId, false)}
            onAttachComment={(comment) => void attachComment(comment)}
            onResolveComment={(comment, resolved) => void setCommentResolved(comment, resolved)}
            onViewport={(next) => { viewport = next; }}
          />
        {:else if selectedReviewFile?.flags.binary}
          <div class="review-notice"><FileDiff size={22} /><p>{t("chat.inspector.binary")}</p></div>
        {:else if selectedPatch?.state === "oversized_hunk"}
          <div class="review-notice"><AlertTriangle size={22} /><p>{t("chat.review.oversizedHunk")}</p></div>
        {:else if selectedPatch?.state === "unavailable"}
          <div class="review-notice"><AlertTriangle size={22} /><p>{t("chat.review.patchUnavailable")}</p></div>
        {:else if selectedReviewFile && loadingPatchIds.includes(selectedReviewFile.fileId)}
          <div class="review-notice"><RefreshCw class="spinning" size={18} /><p>{t("chat.review.loadingPatch")}</p></div>
        {:else if loadingInitial}
          <div class="review-notice"><RefreshCw class="spinning" size={18} /><p>{t("common.loading")}</p></div>
        {:else if error}
          <div class="review-failure"><Notice layout="fill" title={t("chat.review.reviewUnavailable")} message={error} onRetry={retryReview} /></div>
        {:else}
          <div class="review-notice"><FileDiff size={22} /><p>{t("chat.inspector.noChanges")}</p></div>
        {/if}

        {#if selectedReviewFile && lastPatchForFile(selectedReviewFile.fileId)?.continuationCursor}
          {@const continuation = lastPatchForFile(selectedReviewFile.fileId)}
          {#if continuation}<button type="button" class="load-more" disabled={loadingPatchIds.includes(selectedReviewFile.fileId)} onclick={() => void reviewSession.loadContinuation(continuation)}>{t("chat.review.loadMoreDiff")}</button>{/if}
        {/if}
      </main>
    </div>

    {#if commentsVisible}
      <aside class="comments-panel" aria-label={t("chat.review.comments", visibleComments.length)}>
        <header><strong>{t("chat.review.comments", visibleComments.length)}</strong><label><Checkbox bind:checked={includeResolved} />{t("chat.review.showResolved")}</label></header>
        <div>
          {#each visibleComments as comment (comment.id)}
            <article class:resolved={comment.state === "resolved"}>
              <button type="button" class="comment-location" disabled={Boolean(commentDraft.trim())} onclick={() => {
                const file = snapshot?.files.find((candidate) => candidate.relativePath === comment.relativePath);
                if (file) {
                  activateFile(file.fileId);
                  if (!reviewCommentIsOutdated(comment, snapshot)) requestAnimationFrame(() => viewport?.scrollToLine(file.fileId, comment.endLine, comment.selectionSide === "old" ? "deletions" : "additions"));
                } else window.dispatchEvent(new CustomEvent("ganbaru-ai:chat-open-file", { detail: { relativePath: comment.relativePath } }));
              }}><strong title={comment.relativePath}>{comment.relativePath}</strong><span>{t("chat.review.lines", comment.startLine, comment.endLine)}</span></button>
              {#if reviewCommentIsOutdated(comment, snapshot)}<span class="comment-state">{t("chat.review.outdated")}</span>{/if}
              <p>{comment.commentText}</p>
              <footer>
                <button type="button" disabled={busyCommentId !== null || attachingCommentIds.includes(comment.id)} onclick={() => void attachComment(comment)}><Paperclip size={12} />{t("chat.review.attach")}</button>
                {#if comment.state === "open"}<button type="button" disabled={busyCommentId !== null} onclick={() => void setCommentResolved(comment, true)}><Check size={12} />{t("chat.review.resolve")}</button>
                {:else}<button type="button" disabled={busyCommentId !== null} onclick={() => void setCommentResolved(comment, false)}><RotateCcw size={12} />{t("chat.review.reopen")}</button>{/if}
              </footer>
            </article>
          {:else}
            <p class="empty">{t("chat.inspector.reviewEmpty")}</p>
          {/each}
        </div>
      </aside>
    {/if}
  </div>
</section>

<style>
  .review-panel { container-type: inline-size; display: flex; height: 100%; min-height: 0; flex-direction: column; overflow: hidden; background: var(--cal-bg); }
  .review-toolbar, .search-toolbar, .file-toolbar { display: flex; min-width: 0; flex: 0 0 auto; align-items: center; gap: 0.3rem; border-bottom: 1px solid var(--border); padding: 0.35rem 0.45rem; }
  .review-toolbar { min-height: 2.65rem; overflow-x: auto; }
  .review-toolbar :global(.source-picker) { width: auto; min-width: 7rem; max-width: 13rem; flex: 0 1 13rem; }
  .toolbar-group { display: flex; border: 1px solid var(--border); border-radius: 0.4rem; padding: 0.1rem; }
  .scope-actions { display: flex; align-items: center; gap: 0.2rem; }
  .scope-actions button { display: inline-flex; height: 1.7rem; align-items: center; gap: 0.2rem; border-radius: 0.3rem; padding-inline: 0.35rem; color: var(--muted-foreground); font-size: calc(0.633333rem * var(--type-scale)); white-space: nowrap; }
  .scope-actions button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  .scope-actions button.destructive:hover:not(:disabled), .comment-composer button.destructive:hover:not(:disabled) { background: color-mix(in srgb, var(--destructive) 15%, transparent); color: var(--destructive); }
  .toolbar-group button, .review-toolbar > button, .search-toolbar > button, .file-toolbar button { display: inline-grid; width: 1.7rem; height: 1.7rem; flex: 0 0 auto; place-items: center; border-radius: 0.3rem; color: var(--foreground); }
  .toolbar-group button:hover, .toolbar-group button.active, .review-toolbar > button:hover, .review-toolbar > button.active, .file-toolbar button:hover, .search-toolbar > button:hover { background: var(--accent); }
  .diff-stats { display: flex; margin-left: auto; gap: 0.3rem; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.633333rem * var(--type-scale)); }
  .diff-stats span:first-child { color: var(--action-confirm); }
  .diff-stats span:last-child { color: var(--destructive); }
  .comments-toggle { display: inline-flex !important; width: auto !important; padding-inline: 0.35rem; gap: 0.25rem; font-size: calc(0.633333rem * var(--type-scale)); }
  .search-toolbar { min-height: 2.35rem; }
  .search-toolbar label { display: flex; min-width: 5rem; max-width: 18rem; flex: 1; align-items: center; gap: 0.3rem; border: 1px solid var(--border); border-radius: 0.35rem; padding: 0.22rem 0.35rem; color: var(--muted-foreground); }
  .search-toolbar input { min-width: 0; flex: 1; background: transparent; color: var(--foreground); font-size: calc(0.666667rem * var(--type-scale)); outline: none; }
  .search-toolbar > span { color: var(--muted-foreground); font-family: monospace; font-size: calc(0.633333rem * var(--type-scale)); }
  .freshness { display: flex; flex: 0 0 auto; align-items: center; gap: 0.4rem; border-bottom: 1px solid color-mix(in srgb, var(--status-tentative) 30%, var(--border)); padding: 0.4rem 0.55rem; color: var(--status-tentative); font-size: calc(0.7rem * var(--type-scale)); }
  .review-panel > :global(.review-error) { flex: 0 0 auto; border-bottom: 1px solid var(--border); padding: 0.4rem 0.55rem; font-size: calc(0.7rem * var(--type-scale)); }
  .review-failure { display: grid; min-height: 0; flex: 1; font-size: calc(0.733333rem * var(--type-scale)); }
  .review-workspace { display: grid; min-width: 0; min-height: 0; flex: 1; grid-template-columns: minmax(0, 1fr); overflow: hidden; }
  .review-workspace.comments-open { grid-template-columns: minmax(0, 1fr) minmax(13rem, 22rem); }
  .review-main { display: grid; min-width: 0; min-height: 0; grid-template-columns: 13.5rem minmax(0, 1fr); overflow: hidden; }
  .review-file-list { min-width: 0; min-height: 0; overflow: auto; border-right: 1px solid var(--border); padding: 0.35rem; }
  .review-file-list > p { padding: 0.7rem; color: var(--muted-foreground); text-align: center; font-size: calc(0.7rem * var(--type-scale)); }
  .review-content { display: flex; min-width: 0; min-height: 0; flex-direction: column; overflow: hidden; }
  .file-toolbar { min-height: 2.45rem; }
  .file-toolbar strong { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.7rem * var(--type-scale)); font-weight: 500; }
  .file-toolbar .read-only { border-radius: 0.25rem; background: var(--muted); padding: 0.15rem 0.3rem; color: var(--muted-foreground); font-size: calc(0.583333rem * var(--type-scale)); }
  .file-toolbar button.destructive:hover { background: color-mix(in srgb, var(--destructive) 15%, transparent); color: var(--destructive); }
  .file-toolbar button[aria-disabled="true"] { opacity: 0.48; }
  .comment-composer { display: grid; flex: 0 0 auto; gap: 0.4rem; border-bottom: 1px solid var(--border); background: var(--background); padding: 0.5rem; }
  .comment-composer > div, .comment-composer footer { display: flex; align-items: center; gap: 0.35rem; }
  .comment-composer > div strong { margin-right: auto; font-size: calc(0.7rem * var(--type-scale)); }
  .comment-composer button { display: inline-flex; align-items: center; gap: 0.2rem; border-radius: 0.3rem; padding: 0.25rem 0.4rem; color: var(--muted-foreground); font-size: calc(0.633333rem * var(--type-scale)); }
  .comment-composer button:hover { background: var(--accent); color: var(--foreground); }
  .comment-composer textarea { min-height: 3.5rem; max-height: 9rem; resize: vertical; color: var(--foreground); font-size: var(--panel-detail-font-size); }
  .comment-composer footer { justify-content: flex-end; }
  .comment-composer button.primary { background: var(--primary); color: var(--primary-foreground); }
  .comment-composer > p { color: var(--muted-foreground); font-size: calc(0.666667rem * var(--type-scale)); }
  .review-notice { display: grid; min-height: 0; flex: 1; place-content: center; justify-items: center; gap: 0.5rem; padding: 1rem; color: var(--muted-foreground); text-align: center; font-size: calc(0.733333rem * var(--type-scale)); }
  .load-more { flex: 0 0 auto; border-top: 1px solid var(--border); padding: 0.45rem; color: var(--primary); font-size: calc(0.7rem * var(--type-scale)); }
  .load-more:hover { background: var(--accent); }
  .comments-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; overflow: hidden; border-left: 1px solid var(--border); background: var(--background); }
  .comments-panel > header { display: flex; min-height: 2.45rem; align-items: center; justify-content: space-between; gap: 0.4rem; border-bottom: 1px solid var(--border); padding: 0.4rem 0.55rem; font-size: calc(0.7rem * var(--type-scale)); }
  .comments-panel > header label { display: flex; align-items: center; gap: 0.3rem; color: var(--muted-foreground); font-size: calc(0.633333rem * var(--type-scale)); }
  .comments-panel > div { display: grid; min-height: 0; gap: 0.45rem; overflow: auto; padding: 0.5rem; }
  .comments-panel article { display: grid; gap: 0.35rem; align-self: start; border: 1px solid var(--border); border-radius: 0.45rem; padding: 0.5rem; font-size: calc(0.7rem * var(--type-scale)); }
  .comments-panel article.resolved { opacity: 0.64; }
  .comment-location { display: flex; min-width: 0; align-items: center; gap: 0.35rem; text-align: left; }
  .comment-location strong { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .comment-location span, .comment-state { flex: 0 0 auto; color: var(--muted-foreground); font-family: monospace; font-size: calc(0.6rem * var(--type-scale)); }
  .comment-state { color: var(--status-tentative); }
  .comments-panel article > p { white-space: pre-wrap; overflow-wrap: anywhere; }
  .comments-panel article footer { display: flex; justify-content: flex-end; gap: 0.25rem; }
  .comments-panel article footer button { display: inline-flex; align-items: center; gap: 0.2rem; border-radius: 0.3rem; padding: 0.2rem 0.35rem; color: var(--muted-foreground); font-size: calc(0.633333rem * var(--type-scale)); }
  .comments-panel article footer button:hover { background: var(--accent); color: var(--foreground); }
  .empty { margin: auto; padding: 1rem; color: var(--muted-foreground); text-align: center; font-size: calc(0.7rem * var(--type-scale)); }
  .spinning { animation: review-spin 900ms linear infinite; }
  @keyframes review-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinning { animation: none; } }

  @container (max-width: 719px) {
    .review-main { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(5rem, 9rem) minmax(0, 1fr); }
    .review-file-list { border-right: 0; border-bottom: 1px solid var(--border); }
    .review-workspace.comments-open { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) minmax(8rem, 38%); }
    .comments-panel { border-top: 1px solid var(--border); border-left: 0; }
    .review-toolbar { padding-right: 0.35rem; }
    .diff-stats { margin-left: 0; }
    .search-toolbar label:first-child { display: none; }
  }

  @container (max-width: 430px) {
    .review-toolbar { flex-wrap: wrap; }
    .review-toolbar :global(.source-picker) { order: -1; max-width: none; flex: 1 1 calc(100% - 4rem); }
    .toolbar-group { margin-left: 0; }
    .scope-actions button span { display: none; }
    .file-toolbar .read-only, .diff-stats { display: none; }
    .comment-composer > div { flex-wrap: wrap; }
  }
</style>
