<script lang="ts" module>
  /** Delay that lets several threads scrolled into view together share one read request. */
  const READ_BATCH_MS = 150;

  type MarkThreadsRead = (discussionIds: readonly string[]) => Promise<void>;

  const pendingReads = new Map<MarkThreadsRead, Set<string>>();
  let readTimer: ReturnType<typeof setTimeout> | null = null;

  function flushReads(): void {
    readTimer = null;
    const batches = [...pendingReads];
    pendingReads.clear();
    for (const [markRead, ids] of batches) {
      markRead([...ids]).catch((error: unknown) => console.warn("notes comment read marker failed", error));
    }
  }

  /** Queue a discussion to be marked read, merging the threads that become visible within one batch window. */
  function queueRead(markRead: MarkThreadsRead, discussionId: string): void {
    let ids = pendingReads.get(markRead);
    if (!ids) {
      ids = new Set();
      pendingReads.set(markRead, ids);
    }
    ids.add(discussionId);
    readTimer ??= setTimeout(flushReads, READ_BATCH_MS);
  }
</script>

<script lang="ts">
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesCommentThread } from "$lib/notes/contracts/collaboration";
  import type { NotesActivityTimeLabels } from "$lib/notes/pages/activity";
  import type { NotesRichText } from "$lib/notes/types";
  import { tick } from "svelte";
  import NotesCommentComposer from "./NotesCommentComposer.svelte";
  import NotesCommentItem from "./NotesCommentItem.svelte";

  /**
   * One discussion: its comments joined by a thread line, then a reply composer. The composer is either always shown,
   * as in the discussions panel, or opened on demand from a comment, as under the page title.
   */
  let {
    thread,
    now,
    timeLabels,
    replyMode = "always",
    menuPlacement = "below",
    resolveControl = false,
    markReadWhenVisible = true,
  }: {
    thread: NotesCommentThread;
    now: Date;
    timeLabels: NotesActivityTimeLabels;
    replyMode?: "always" | "on-demand";
    menuPlacement?: "below" | "above";
    /** Lets the first comment resolve or reopen the discussion. */
    resolveControl?: boolean;
    /** Marks an unread discussion read once it scrolls into view. */
    markReadWhenVisible?: boolean;
  } = $props();

  const notes = getNotesEditor();
  const { t } = getLocalization();

  let root = $state<HTMLElement | null>(null);
  let replyComposer = $state<ReturnType<typeof NotesCommentComposer> | null>(null);
  let replyOpen = $state(false);

  const replyVisible = $derived(replyMode === "always" || replyOpen);

  $effect(() => {
    const element = root;
    if (!element || !markReadWhenVisible || !thread.unread) return;
    const discussionId = thread.id;
    const markRead = notes.markCommentThreadsRead;
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      queueRead(markRead, discussionId);
    });
    observer.observe(element);
    return () => observer.disconnect();
  });

  /** Show the reply composer when needed and move focus into it. */
  export async function focusReply(): Promise<void> {
    replyOpen = true;
    await tick();
    replyComposer?.focus();
  }

  function submitReply(richText: NotesRichText[], attachments: Record<string, unknown>[]): Promise<boolean> {
    return notes.replyToCommentThread(thread.id, richText, attachments);
  }

  function closeEmptyReply(): void {
    if (replyMode === "on-demand") replyOpen = false;
  }

  /**
   * Clicking the thread outside its controls starts a reply, as reading a discussion usually leads to answering it.
   * Each comment's reply button is the keyboard path to the same action.
   */
  function startReplyFromThread(event: MouseEvent): void {
    if (replyMode !== "on-demand" || replyOpen) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest("button, a, input, textarea, select, [contenteditable='true'], [role='button'], [role='menu'], [role='dialog']")) return;
    if (window.getSelection()?.isCollapsed === false) return;
    void focusReply();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<section
  bind:this={root}
  class="comment-thread min-w-0"
  data-notes-discussion-id={thread.id}
  onclick={startReplyFromThread}
>
  {#each thread.comments as comment, index (comment.id)}
    <NotesCommentItem
      {thread}
      {comment}
      {now}
      {timeLabels}
      connectNext={index < thread.comments.length - 1}
      resolveControl={resolveControl && index === 0}
      onReply={replyMode === "on-demand" ? () => void focusReply() : undefined}
    />
  {/each}
  {#if replyVisible}
    <div class="mt-1">
      <NotesCommentComposer
        bind:this={replyComposer}
        label={t("notes.replyCommentInput")}
        placeholder={t("notes.replyPlaceholder")}
        {menuPlacement}
        onSubmit={submitReply}
        onLeaveEmpty={closeEmptyReply}
      />
    </div>
  {/if}
</section>
