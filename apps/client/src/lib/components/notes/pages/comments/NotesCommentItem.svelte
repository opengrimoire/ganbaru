<script lang="ts">
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import type IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesCommentBodyRichText } from "$lib/notes/collaboration/comments";
  import type { NotesComment, NotesCommentReaction, NotesCommentThread } from "$lib/notes/contracts/collaboration";
  import { buildNotesDiscussionLink } from "$lib/notes/links/block-link";
  import { formatNotesActivityTime, type NotesActivityTimeLabels } from "$lib/notes/pages/activity";
  import type { NotesRichText } from "$lib/notes/types";
  import { parseProjectIcon } from "$lib/projects/icons/values";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { portal } from "$lib/utils/portal";
  import Bell from "@lucide/svelte/icons/bell";
  import BellOff from "@lucide/svelte/icons/bell-off";
  import Check from "@lucide/svelte/icons/check";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import LinkIcon from "@lucide/svelte/icons/link";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Reply from "@lucide/svelte/icons/reply";
  import SmilePlus from "@lucide/svelte/icons/smile-plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { onDestroy } from "svelte";
  import NotesCommentAttachment from "./NotesCommentAttachment.svelte";
  import NotesCommentAvatar from "./NotesCommentAvatar.svelte";
  import NotesCommentComposer from "./NotesCommentComposer.svelte";

  /** Avatar size shared with the thread line, which starts below it. */
  const AVATAR_SIZE = 22;
  /** Saves within this window of creation do not mark the comment as edited. */
  const EDIT_MARK_THRESHOLD_MS = 1_000;
  const COPIED_RESET_MS = 2_000;

  /**
   * One comment in a discussion: author, relative time, body, attachments, and reactions, with a hover toolbar to react
   * and a menu to edit, copy a link to the discussion, mute replies, or delete. A line joins it to the next comment.
   */
  let {
    thread,
    comment,
    now,
    timeLabels,
    connectNext = false,
    resolveControl = false,
    onReply,
  }: {
    thread: NotesCommentThread;
    comment: NotesComment;
    /** Reference time for relative timestamps, refreshed by the editor. */
    now: Date;
    timeLabels: NotesActivityTimeLabels;
    /** Draws the thread line from this comment's avatar to the next comment. */
    connectNext?: boolean;
    /** Offers resolving or reopening the discussion, which the first comment of a thread carries. */
    resolveControl?: boolean;
    /** Adds a reply action, for threads that only show their reply composer on demand. */
    onReply?: () => void;
  } = $props();

  const notes = getNotesEditor();
  const localization = getLocalization();
  const { t } = localization;

  let editing = $state(false);
  let confirmingDelete = $state(false);
  let errorMessage = $state<string | null>(null);
  let linkCopied = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;
  let ReactionPicker = $state<typeof IconPicker | null>(null);
  let reactionPickerValue = $state("emoji:👍");
  let reactionsPending = $state(false);
  let editor = $state<ReturnType<typeof NotesCommentComposer> | null>(null);
  let article = $state<HTMLElement | null>(null);

  const ownComment = $derived(notes.localUser !== null && comment.created_by.id === notes.localUser.id);
  const authorName = $derived(comment.display_name.resolved_name.trim() || t("notes.commentAuthorUnknown"));
  const body = $derived(notesCommentBodyRichText(comment.rich_text));
  const createdLabel = $derived(formatNotesActivityTime(comment.created_time, {
    locale: localization.locale,
    now,
    labels: timeLabels,
  }));
  const createdTitle = $derived(formatAbsoluteTime(comment.created_time));
  const edited = $derived(
    Date.parse(comment.last_edited_time) - Date.parse(comment.created_time) > EDIT_MARK_THRESHOLD_MS,
  );
  /** Toolbar buttons: reaction and more actions, plus optional reply and resolve. */
  const toolCount = $derived(2 + (onReply ? 1 : 0) + (resolveControl ? 1 : 0));

  $effect(() => {
    if (editing) void Promise.resolve().then(() => editor?.focus());
  });

  onDestroy(() => {
    if (copiedTimer) clearTimeout(copiedTimer);
  });

  function formatAbsoluteTime(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    return new Intl.DateTimeFormat(localization.locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
  }

  function reportError(error: unknown): void {
    errorMessage = t("notes.commentActionFailed", error instanceof Error ? error.message : String(error));
  }

  async function run(action: () => Promise<unknown>): Promise<void> {
    errorMessage = null;
    try {
      await action();
    } catch (error) {
      reportError(error);
    }
  }

  function reactionTooltip(reaction: NotesCommentReaction): string {
    return t("notes.commentReactedBy", formatList(localization.locale, reaction.names), reaction.count);
  }

  async function toggleReaction(emoji: string): Promise<void> {
    if (reactionsPending) return;
    reactionsPending = true;
    try {
      await run(() => notes.toggleCommentReaction(comment.id, emoji));
    } finally {
      reactionsPending = false;
    }
  }

  async function openReactionPicker(): Promise<void> {
    if (ReactionPicker) return;
    try {
      ReactionPicker = (await import("$lib/components/icon-picker/IconPicker.svelte")).default;
    } catch (error) {
      reportError(error);
    }
  }

  function pickReaction(value: string): void {
    reactionPickerValue = value;
    const icon = parseProjectIcon(value);
    if (icon.kind !== "emoji") return;
    const existing = comment.reactions.find((reaction) => reaction.emoji === icon.emoji);
    if (existing?.reacted_by_me) return;
    void toggleReaction(icon.emoji);
  }

  async function saveEdit(richText: NotesRichText[], attachments: Record<string, unknown>[]): Promise<boolean> {
    const saved = await notes.updateComment(comment.id, richText, attachments);
    if (saved) editing = false;
    return saved;
  }

  async function copyDiscussionLink(): Promise<void> {
    await run(async () => {
      await writeTextToClipboard(buildNotesDiscussionLink(window.location.href, {
        pageId: thread.page_id,
        discussionId: thread.id,
      }));
      linkCopied = true;
      if (copiedTimer) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => {
        linkCopied = false;
        copiedTimer = null;
      }, COPIED_RESET_MS);
    });
  }

  function confirmDelete(): void {
    confirmingDelete = false;
    void run(() => notes.deleteComment(comment.id));
  }
</script>

<article
  bind:this={article}
  class="comment-item relative flex min-w-0 gap-2 rounded-md py-1.5"
  aria-label={authorName}
  data-notes-comment-id={comment.id}
>
  <div class="relative flex shrink-0 flex-col items-center">
    <NotesCommentAvatar own={ownComment} name={authorName} size={AVATAR_SIZE} />
    {#if connectNext}
      <span class="comment-thread-line" aria-hidden="true"></span>
    {/if}
  </div>
  <div class="min-w-0 flex-1">
    <header class="comment-header flex min-h-5.5 min-w-0 items-baseline gap-1.5" style={`--comment-tool-count:${toolCount}`}>
      <span class="min-w-0 truncate font-medium text-foreground">{authorName}</span>
      <time class="shrink-0 text-panel-detail text-muted-foreground" datetime={comment.created_time} title={createdTitle}>{createdLabel}</time>
      {#if edited}
        <span class="shrink-0 text-panel-detail text-muted-foreground" title={formatAbsoluteTime(comment.last_edited_time)}>{t("notes.commentEdited")}</span>
      {/if}
    </header>
    {#if editing}
      <div class="mt-1">
        <NotesCommentComposer
          bind:this={editor}
          label={t("notes.editCommentInput")}
          placeholder={t("notes.commentPlaceholder")}
          initialRichText={comment.rich_text}
          initialAttachments={comment.attachments}
          mode="edit"
          showAvatar={false}
          onSubmit={saveEdit}
          onCancel={() => { editing = false; }}
        />
      </div>
    {:else}
      {#if body.length > 0}
        <div class="comment-body min-w-0 whitespace-pre-wrap wrap-break-word text-foreground">
          <NotesRichTextInline richText={body} />
        </div>
      {/if}
      {#if comment.attachments.length > 0}
        <div class="mt-1.5 flex flex-wrap gap-2">
          {#each comment.attachments as attachment, index (index)}
            <NotesCommentAttachment {attachment} />
          {/each}
        </div>
      {/if}
      {#if comment.reactions.length > 0}
        <div class="mt-1.5 flex flex-wrap items-center gap-1">
          {#each comment.reactions as reaction (reaction.emoji)}
            <button
              type="button"
              class="reaction-chip"
              class:selected={reaction.reacted_by_me}
              aria-pressed={reaction.reacted_by_me}
              aria-label={reactionTooltip(reaction)}
              data-app-tooltip={reactionTooltip(reaction)}
              disabled={reactionsPending}
              onclick={() => void toggleReaction(reaction.emoji)}
            >
              <span class="reaction-emoji" aria-hidden="true">{reaction.emoji}</span>
              <span>{reaction.count}</span>
            </button>
          {/each}
        </div>
      {/if}
    {/if}
    {#if errorMessage}
      <p class="mt-1 text-panel-detail text-destructive" role="alert">{errorMessage}</p>
    {/if}
  </div>
  {#if !editing}
    <div class="comment-toolbar surface-floating absolute right-0 top-0.5 flex items-center overflow-hidden">
      {#if ReactionPicker}
        <ReactionPicker
          value={reactionPickerValue}
          onChange={pickReaction}
          ariaLabel={t("notes.commentAddReaction")}
          showIcons={false}
          showUpload={false}
          showRemove={false}
          nested
          initiallyOpen
        >
          {#snippet trigger({ open, toggle, panelId })}
            <button
              type="button"
              class="comment-toolbar-button"
              class:active={open}
              aria-label={t("notes.commentAddReaction")}
              aria-haspopup="dialog"
              aria-expanded={open}
              aria-controls={panelId}
              data-app-tooltip={t("notes.commentAddReaction")}
              onclick={toggle}
            ><SmilePlus class="size-3.5" aria-hidden="true" /></button>
          {/snippet}
        </ReactionPicker>
      {:else}
        <button
          type="button"
          class="comment-toolbar-button"
          aria-label={t("notes.commentAddReaction")}
          data-app-tooltip={t("notes.commentAddReaction")}
          onclick={() => void openReactionPicker()}
        ><SmilePlus class="size-3.5" aria-hidden="true" /></button>
      {/if}
      {#if onReply}
        <button
          type="button"
          class="comment-toolbar-button"
          aria-label={t("notes.reply")}
          data-app-tooltip={t("notes.reply")}
          onclick={onReply}
        ><Reply class="size-3.5" aria-hidden="true" /></button>
      {/if}
      {#if resolveControl}
        {@const resolved = thread.status === "resolved"}
        <button
          type="button"
          class="comment-toolbar-button"
          aria-label={resolved ? t("notes.reopenCommentThread") : t("notes.resolveCommentThread")}
          data-app-tooltip={resolved ? t("notes.reopenCommentThread") : t("notes.resolveCommentThread")}
          onclick={() => void run(() => notes.setCommentThreadResolved(thread.id, !resolved))}
        >
          {#if resolved}
            <RotateCcw class="size-3.5" aria-hidden="true" />
          {:else}
            <CircleCheck class="size-3.5" aria-hidden="true" />
          {/if}
        </button>
      {/if}
      <CollectionMenu
        kind="actions"
        iconOnly
        showHeader={false}
        icon={Ellipsis}
        label={t("notes.commentMoreActions")}
        triggerClass="comment-toolbar-button justify-center rounded-none px-0"
      >
        {#if ownComment}
          <CollectionMenuItem icon={Pencil} label={t("notes.commentEdit")} onclick={() => { editing = true; }} />
        {/if}
        <button type="button" class="menu-item min-w-0" data-collection-menu-keep-open onclick={() => void copyDiscussionLink()}>
          {#if linkCopied}
            <Check class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />
          {:else}
            <LinkIcon class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />
          {/if}
          <span class="min-w-0 flex-1 truncate" aria-live="polite">{linkCopied ? t("notes.blockLinkCopied") : t("notes.commentCopyLink")}</span>
        </button>
        <CollectionMenuItem
          icon={thread.muted ? Bell : BellOff}
          label={thread.muted ? t("notes.commentUnmuteReplies") : t("notes.commentMuteReplies")}
          onclick={() => void run(() => notes.setCommentThreadMuted(thread.id, !thread.muted))}
        />
        {#if ownComment}
          <CollectionMenuSeparator />
          <CollectionMenuItem icon={Trash2} label={t("notes.commentDelete")} destructive onclick={() => { confirmingDelete = true; }} />
        {/if}
      </CollectionMenu>
    </div>
  {/if}
</article>

{#if confirmingDelete}
  <div use:portal={article?.closest<HTMLElement>("[data-floating-root]") ?? "body"} class="contents">
    <ConfirmDialog
      title={t("notes.commentDeleteConfirmTitle")}
      message={t("notes.commentDeleteConfirmMessage")}
      confirmLabel={t("notes.commentDelete")}
      cancelLabel={t("common.cancel")}
      onConfirm={confirmDelete}
      onCancel={() => { confirmingDelete = false; }}
    />
  </div>
{/if}

<style>
  .comment-item {
    --comment-tool-size: 1.75rem;
  }
  .comment-thread-line {
    position: absolute;
    top: calc(22px + 0.25rem);
    bottom: -0.5rem;
    left: 50%;
    width: 1px;
    transform: translateX(-50%);
    background-color: var(--border);
  }
  .reaction-chip {
    display: inline-flex;
    min-width: 2.35rem;
    height: 1.5rem;
    align-items: center;
    justify-content: center;
    gap: 0.3rem;
    border: 1px solid transparent;
    border-radius: 0.4rem;
    background: color-mix(in srgb, var(--accent) 76%, transparent);
    padding-inline: 0.4rem;
    color: var(--muted-foreground);
    font-size: var(--panel-detail-font-size);
    font-variant-numeric: tabular-nums;
  }
  .reaction-chip:hover,
  .reaction-chip:focus-visible {
    border-color: var(--border);
    background: var(--accent);
    color: var(--foreground);
  }
  .reaction-chip.selected {
    border-color: color-mix(in srgb, var(--primary) 60%, var(--border));
    background: color-mix(in srgb, var(--primary) 13%, transparent);
    color: var(--primary);
  }
  .reaction-emoji {
    font-size: 0.875rem;
    line-height: 1;
  }
  .comment-toolbar {
    pointer-events: none;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .comment-item:is(:hover, :focus-within) .comment-toolbar,
  .comment-toolbar:has(:global([aria-expanded="true"])) {
    pointer-events: auto;
    opacity: 1;
  }
  .comment-toolbar :global(.comment-toolbar-button) {
    display: inline-grid;
    width: var(--comment-tool-size);
    height: var(--comment-tool-size);
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0;
    color: var(--muted-foreground);
  }
  .comment-toolbar :global(.comment-toolbar-button:hover),
  .comment-toolbar :global(.comment-toolbar-button:focus-visible),
  .comment-toolbar :global(.comment-toolbar-button.active),
  .comment-toolbar :global(.comment-toolbar-button[aria-expanded="true"]) {
    background: var(--accent);
    color: var(--foreground);
  }
  /* Without hover the toolbar stays visible, so the author and time keep clear of it. */
  @media (hover: none) {
    .comment-toolbar {
      pointer-events: auto;
      opacity: 1;
    }
    .comment-header {
      padding-right: calc(var(--comment-tool-count) * var(--comment-tool-size) + 0.25rem);
    }
  }
  @media (pointer: coarse) {
    .comment-item {
      --comment-tool-size: 2.75rem;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .comment-toolbar {
      transition: none;
    }
  }
</style>
