import {
  acceptNotesSuggestion,
  createNotesComment,
  createNotesSuggestion,
  deleteNotesComment,
  getNotesLocalUser,
  getNotesPageNotificationSettings,
  listNotesComments,
  listNotesSuggestions,
  markNotesCommentThreadsRead,
  rejectNotesSuggestion,
  resolveNotesCommentThread,
  setNotesCommentThreadMuted,
  setNotesPageNotificationLevel,
  toggleNotesCommentReaction,
  updateNotesComment,
  updateNotesLocalUser,
} from "$lib/api/notes";
import { blockPlainText } from "$lib/notes/blocks/factory";
import {
  notesCommentAnchorDraft,
  notesCommentBodyRichText,
  notesCommentIsEmpty,
  notesCommentParentKey,
  notesCommentParentMatches,
} from "$lib/notes/collaboration/comments";
import {
  notesApplySuggestionToBlock,
  notesSuggestionCreateRequest,
  notesSuggestionDraft,
  type NotesSuggestionDraft,
} from "$lib/notes/collaboration/suggestions";
import type {
  NotesBlock,
  NotesCommentAnchorCreate,
  NotesCommentParent,
  NotesCommentThread,
  NotesLocalUser,
  NotesPageNotificationLevel,
  NotesRichText,
  NotesSuggestion,
} from "$lib/notes/types";

/** Stored attachment payloads, as the backend accepts and returns them. */
type NotesCommentAttachments = readonly Record<string, unknown>[];

interface NotesCollaborationControllerContext {
  readSelectedPageId: () => string | null;
  readBlocksById: () => Record<string, NotesBlock>;
  flushBlockSave: (blockId: string) => Promise<void>;
  requestBlockFocus: (blockId: string) => void;
  updateBlockRichText: (blockId: string, richText: readonly NotesRichText[]) => Promise<void>;
  isPanelOpen: (panel: "suggestions") => boolean;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Own Notes comments, suggestions, and local collaboration identity. */
export function createNotesCollaborationController(
  context: NotesCollaborationControllerContext,
) {
  let commentThreads = $state<NotesCommentThread[]>([]);
  let activeCommentParent = $state<NotesCommentParent | null>(null);
  let activeCommentAnchor = $state<NotesCommentAnchorCreate | null>(null);
  let commentsLoading = $state(false);
  let commentsError = $state<string | null>(null);
  let commentsIncludeResolved = $state(false);
  let commentsPageId: string | null = null;
  const visibleCommentThreads = $derived(
    commentsIncludeResolved
      ? commentThreads
      : commentThreads.filter((thread) => thread.status === "open"),
  );
  let suggestions = $state<NotesSuggestion[]>([]);
  let activeSuggestionDraft = $state<NotesSuggestionDraft | null>(null);
  let suggestionsLoading = $state(false);
  let suggestionsError = $state<string | null>(null);
  let suggestionsIncludeDecided = $state(false);
  let localUser = $state<NotesLocalUser | null>(null);
  let localUserLoading = $state(false);
  let localUserError = $state<string | null>(null);
  let pageNotificationLevel = $state<NotesPageNotificationLevel | null>(null);
  let commentsRequestId = 0;
  let suggestionsRequestId = 0;
  let localUserRequestId = 0;
  let pageNotificationRequestId = 0;

  /** Load every live discussion on the page, including ones on blocks that are not hydrated yet. */
  async function reloadComments(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++commentsRequestId;
    if (!pageId) {
      commentsPageId = null;
      commentThreads = [];
      activeCommentParent = null;
      activeCommentAnchor = null;
      commentsError = null;
      commentsLoading = false;
      return;
    }
    if (pageId !== commentsPageId) {
      commentsPageId = pageId;
      commentThreads = [];
    }
    const previousThreads = commentThreads;
    commentsLoading = true;
    commentsError = null;
    try {
      const next = await listNotesComments(pageId, true);
      if (requestId !== commentsRequestId || pageId !== context.readSelectedPageId()) return;
      commentThreads = [...next];
    } catch (error) {
      if (requestId !== commentsRequestId || pageId !== context.readSelectedPageId()) return;
      commentThreads = previousThreads;
      commentsError = errorMessage(error);
    } finally {
      if (requestId === commentsRequestId) commentsLoading = false;
    }
  }

  async function reloadSuggestions(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++suggestionsRequestId;
    if (!pageId) {
      suggestions = [];
      activeSuggestionDraft = null;
      suggestionsError = null;
      suggestionsLoading = false;
      return;
    }
    suggestionsLoading = true;
    suggestionsError = null;
    try {
      const next = await listNotesSuggestions(pageId, suggestionsIncludeDecided);
      if (requestId !== suggestionsRequestId || pageId !== context.readSelectedPageId()) return;
      suggestions = [...next];
    } catch (error) {
      if (requestId !== suggestionsRequestId || pageId !== context.readSelectedPageId()) return;
      suggestions = [];
      suggestionsError = errorMessage(error);
    } finally {
      if (requestId === suggestionsRequestId) suggestionsLoading = false;
    }
  }

  async function loadLocalUser(): Promise<NotesLocalUser | null> {
    const requestId = ++localUserRequestId;
    localUserLoading = true;
    localUserError = null;
    try {
      const next = await getNotesLocalUser();
      if (requestId !== localUserRequestId) return localUser;
      localUser = next;
      return next;
    } catch (error) {
      if (requestId !== localUserRequestId) return localUser;
      localUserError = errorMessage(error);
      return null;
    } finally {
      if (requestId === localUserRequestId) localUserLoading = false;
    }
  }

  async function updateLocalUserDisplayName(displayName: string): Promise<NotesLocalUser | null> {
    const requestId = ++localUserRequestId;
    localUserLoading = true;
    localUserError = null;
    try {
      const next = await updateNotesLocalUser({ display_name: displayName });
      if (requestId !== localUserRequestId) return localUser;
      localUser = next;
      await Promise.all([
        commentsPageId !== null ? reloadComments() : Promise.resolve(),
        context.isPanelOpen("suggestions") ? reloadSuggestions() : Promise.resolve(),
      ]);
      return next;
    } catch (error) {
      if (requestId !== localUserRequestId) return localUser;
      localUserError = errorMessage(error);
      return null;
    } finally {
      if (requestId === localUserRequestId) localUserLoading = false;
    }
  }

  async function loadPageNotificationSettings(pageId = context.readSelectedPageId()): Promise<void> {
    const requestId = ++pageNotificationRequestId;
    if (!pageId) {
      pageNotificationLevel = null;
      return;
    }
    const settings = await getNotesPageNotificationSettings(pageId);
    if (requestId !== pageNotificationRequestId || pageId !== context.readSelectedPageId()) return;
    pageNotificationLevel = settings.level;
  }

  /** Choose which comment activity on the selected page notifies the local user, reverting on failure. */
  async function setPageNotificationLevel(level: NotesPageNotificationLevel): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) return;
    const requestId = ++pageNotificationRequestId;
    const previous = pageNotificationLevel;
    pageNotificationLevel = level;
    try {
      const settings = await setNotesPageNotificationLevel(pageId, level);
      if (requestId !== pageNotificationRequestId || pageId !== context.readSelectedPageId()) return;
      pageNotificationLevel = settings.level;
    } catch (error) {
      if (requestId === pageNotificationRequestId && pageId === context.readSelectedPageId()) {
        pageNotificationLevel = previous;
      }
      throw error;
    }
  }

  function updateCommentThread(thread: NotesCommentThread): void {
    if (thread.page_id !== context.readSelectedPageId()) return;
    if (thread.comments.length === 0) {
      commentThreads = commentThreads.filter((candidate) => candidate.id !== thread.id);
    } else {
      commentThreads = commentThreads.some((candidate) => candidate.id === thread.id)
        ? commentThreads.map((candidate) => candidate.id === thread.id ? thread : candidate)
        : [...commentThreads, thread];
    }
    if (commentsLoading) void reloadComments();
  }

  function updateSuggestion(suggestion: NotesSuggestion): void {
    if (suggestion.status !== "open" && !suggestionsIncludeDecided) {
      suggestions = suggestions.filter((candidate) => candidate.id !== suggestion.id);
      return;
    }
    suggestions = suggestions.some((candidate) => candidate.id === suggestion.id)
      ? suggestions.map((candidate) => candidate.id === suggestion.id ? suggestion : candidate)
      : [...suggestions, suggestion];
  }

  function setActiveCommentParent(parent: NotesCommentParent | null): void {
    activeCommentParent = parent;
    activeCommentAnchor = null;
  }

  async function startBlockComment(blockId: string): Promise<void> {
    if (!context.readBlocksById()[blockId]) return;
    await context.flushBlockSave(blockId);
    activeCommentParent = { type: "block_id", block_id: blockId };
    activeCommentAnchor = null;
    context.requestBlockFocus(blockId);
  }

  async function startInlineComment(blockId: string, start: number, end: number): Promise<void> {
    const block = context.readBlocksById()[blockId];
    if (!block) return;
    const anchor = notesCommentAnchorDraft(blockPlainText(block), start, end);
    if (!anchor) return;
    await context.flushBlockSave(blockId);
    activeCommentParent = { type: "block_id", block_id: blockId };
    activeCommentAnchor = anchor;
    context.requestBlockFocus(blockId);
  }

  async function startInlineSuggestion(blockId: string, start: number, end: number): Promise<void> {
    const block = context.readBlocksById()[blockId];
    if (!block) return;
    const draft = notesSuggestionDraft(block.id, blockPlainText(block), start, end);
    if (!draft) return;
    await context.flushBlockSave(blockId);
    activeSuggestionDraft = draft;
    context.requestBlockFocus(blockId);
  }

  async function createSuggestion(proposedText: string): Promise<void> {
    const draft = activeSuggestionDraft;
    if (!draft || proposedText === draft.original_text) return;
    await context.flushBlockSave(draft.block_id);
    updateSuggestion(await createNotesSuggestion(
      notesSuggestionCreateRequest(crypto.randomUUID(), draft, proposedText),
    ));
    activeSuggestionDraft = null;
    context.requestBlockFocus(draft.block_id);
  }

  async function acceptSuggestion(suggestionId: string): Promise<void> {
    const suggestion = suggestions.find((candidate) => candidate.id === suggestionId);
    if (!suggestion || suggestion.status !== "open") return;
    const block = context.readBlocksById()[suggestion.block_id];
    if (!block) return;
    await context.flushBlockSave(suggestion.block_id);
    const plan = notesApplySuggestionToBlock(block, suggestion);
    if (!plan) {
      suggestionsError = "target_missing";
      return;
    }
    await context.updateBlockRichText(suggestion.block_id, plan.richText);
    await context.flushBlockSave(suggestion.block_id);
    updateSuggestion(await acceptNotesSuggestion(suggestion.id));
    context.requestBlockFocus(suggestion.block_id);
  }

  async function rejectSuggestion(suggestionId: string): Promise<void> {
    const suggestion = suggestions.find((candidate) => candidate.id === suggestionId);
    if (!suggestion || suggestion.status !== "open") return;
    await context.flushBlockSave(suggestion.block_id);
    updateSuggestion(await rejectNotesSuggestion(suggestion.id));
    context.requestBlockFocus(suggestion.block_id);
  }

  /**
   * Start a discussion. Without an explicit parent it targets the pending block comment, or the page.
   * Returns false when the comment has neither visible text nor attachments.
   */
  async function createComment(
    richText: readonly NotesRichText[],
    attachments: NotesCommentAttachments = [],
    parent: NotesCommentParent | null = activeCommentParent
      ?? (context.readSelectedPageId()
        ? { type: "page_id", page_id: context.readSelectedPageId() as string }
        : null),
  ): Promise<boolean> {
    const body = notesCommentBodyRichText(richText);
    if (notesCommentIsEmpty(body, attachments) || !parent) return false;
    if (parent.type === "block_id") await context.flushBlockSave(parent.block_id);
    const pendingParent = activeCommentParent
      && notesCommentParentKey(activeCommentParent) === notesCommentParentKey(parent);
    const anchor = pendingParent ? activeCommentAnchor : null;
    updateCommentThread(await createNotesComment({
      id: crypto.randomUUID(),
      parent,
      anchor: anchor ?? undefined,
      rich_text: body,
      attachments: [...attachments],
    }));
    if (pendingParent) {
      activeCommentParent = null;
      activeCommentAnchor = null;
    }
    return true;
  }

  /** Reply to a discussion. Returns false when the reply has neither visible text nor attachments. */
  async function replyToCommentThread(
    discussionId: string,
    richText: readonly NotesRichText[],
    attachments: NotesCommentAttachments = [],
  ): Promise<boolean> {
    const body = notesCommentBodyRichText(richText);
    if (notesCommentIsEmpty(body, attachments)) return false;
    updateCommentThread(await createNotesComment({
      id: crypto.randomUUID(),
      discussion_id: discussionId,
      rich_text: body,
      attachments: [...attachments],
    }));
    return true;
  }

  /**
   * Replace a comment body. Attachments are always sent so an edit keeps the ones the comment still has.
   * Returns false when the edit would leave neither visible text nor attachments.
   */
  async function updateComment(
    commentId: string,
    richText: readonly NotesRichText[],
    attachments: NotesCommentAttachments,
  ): Promise<boolean> {
    const body = notesCommentBodyRichText(richText);
    if (notesCommentIsEmpty(body, attachments)) return false;
    updateCommentThread(await updateNotesComment(commentId, {
      rich_text: body,
      attachments: [...attachments],
    }));
    return true;
  }

  async function toggleCommentReaction(commentId: string, emoji: string): Promise<void> {
    updateCommentThread(await toggleNotesCommentReaction(commentId, emoji));
  }

  async function setCommentThreadMuted(discussionId: string, muted: boolean): Promise<void> {
    updateCommentThread(await setNotesCommentThreadMuted(discussionId, muted));
  }

  async function deleteComment(commentId: string): Promise<void> {
    updateCommentThread(await deleteNotesComment(commentId));
  }

  async function setCommentThreadResolved(discussionId: string, resolved: boolean): Promise<void> {
    updateCommentThread(await resolveNotesCommentThread(discussionId, resolved));
  }

  async function markCommentThreadsRead(discussionIds: readonly string[]): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) return;
    const requestId = commentsRequestId;
    const ids = [...new Set(discussionIds.map((id) => id.trim()).filter(Boolean))];
    if (ids.length === 0) return;
    const next = await markNotesCommentThreadsRead({
      page_id: pageId,
      discussion_ids: ids,
      include_resolved: true,
    });
    if (requestId !== commentsRequestId || pageId !== context.readSelectedPageId()) return;
    commentThreads = [...next];
    commentsError = null;
  }

  async function markVisibleCommentThreadsRead(parent: NotesCommentParent | null = null): Promise<void> {
    const visible = parent
      ? visibleCommentThreads.filter((thread) => notesCommentParentMatches(thread.parent, parent))
      : visibleCommentThreads;
    await markCommentThreadsRead(visible.filter((thread) => thread.unread).map((thread) => thread.id));
  }

  function resetPageState(): void {
    commentsRequestId += 1;
    suggestionsRequestId += 1;
    pageNotificationRequestId += 1;
    commentThreads = [];
    commentsPageId = null;
    pageNotificationLevel = null;
    suggestions = [];
    activeCommentParent = null;
    activeCommentAnchor = null;
    activeSuggestionDraft = null;
    commentsError = null;
    suggestionsError = null;
    commentsLoading = false;
    suggestionsLoading = false;
  }

  return {
    get commentThreads(): NotesCommentThread[] { return visibleCommentThreads; },
    /** Every loaded discussion, independent of the panel's resolved filter. */
    get allCommentThreads(): NotesCommentThread[] { return commentThreads; },
    get pageNotificationLevel(): NotesPageNotificationLevel | null { return pageNotificationLevel; },
    get commentsLoading(): boolean { return commentsLoading; },
    get commentsError(): string | null { return commentsError; },
    get commentsIncludeResolved(): boolean { return commentsIncludeResolved; },
    get activeCommentParent(): NotesCommentParent | null { return activeCommentParent; },
    get activeCommentAnchor(): NotesCommentAnchorCreate | null { return activeCommentAnchor; },
    get suggestions(): NotesSuggestion[] { return suggestions; },
    get suggestionsLoading(): boolean { return suggestionsLoading; },
    get suggestionsError(): string | null { return suggestionsError; },
    get suggestionsIncludeDecided(): boolean { return suggestionsIncludeDecided; },
    get activeSuggestionDraft(): NotesSuggestionDraft | null { return activeSuggestionDraft; },
    get localUser(): NotesLocalUser | null { return localUser; },
    get localUserLoading(): boolean { return localUserLoading; },
    get localUserError(): string | null { return localUserError; },
    reloadComments,
    reloadSuggestions,
    loadLocalUser,
    updateLocalUserDisplayName,
    setActiveCommentParent,
    startBlockComment,
    startInlineComment,
    startInlineSuggestion,
    cancelSuggestionDraft(): void { activeSuggestionDraft = null; },
    createSuggestion,
    acceptSuggestion,
    rejectSuggestion,
    async setSuggestionsIncludeDecided(value: boolean): Promise<void> {
      suggestionsIncludeDecided = value;
      await reloadSuggestions();
    },
    createComment,
    replyToCommentThread,
    updateComment,
    deleteComment,
    toggleCommentReaction,
    setCommentThreadMuted,
    loadPageNotificationSettings,
    setPageNotificationLevel,
    setCommentThreadResolved,
    markCommentThreadsRead,
    markVisibleCommentThreadsRead,
    async setCommentsIncludeResolved(value: boolean): Promise<void> {
      commentsIncludeResolved = value;
    },
    resetPageState,
    notesCommentParentKey,
  };
}
