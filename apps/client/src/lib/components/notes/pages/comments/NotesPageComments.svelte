<script lang="ts">
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesPageDiscussionsInReadingOrder } from "$lib/notes/collaboration/discussions";
  import type { NotesActivityTimeLabels } from "$lib/notes/pages/activity";
  import type { NotesRichText } from "$lib/notes/types";
  import { tick } from "svelte";
  import NotesCommentComposer from "./NotesCommentComposer.svelte";
  import NotesCommentThreadView from "./NotesCommentThreadView.svelte";

  /**
   * Discussions on the page itself, shown under the title and properties: open threads in reading order, then a
   * composer for a new comment. The section exists while the page has discussions or someone asked to comment.
   */
  let {
    id,
    pageId,
    now,
    timeLabels,
    focusRequest,
    onLeaveEmpty,
  }: {
    id: string;
    pageId: string;
    now: Date;
    timeLabels: NotesActivityTimeLabels;
    /** Increments whenever the composer should be revealed and focused. */
    focusRequest: number;
    /** The composer lost focus while empty and the page has no discussion to keep the section open. */
    onLeaveEmpty: () => void;
  } = $props();

  const notes = getNotesEditor();
  const { t } = getLocalization();
  const headingId = $derived(`${id}-heading`);

  let root = $state<HTMLElement | null>(null);
  let composer = $state<ReturnType<typeof NotesCommentComposer> | null>(null);
  let handledFocusRequest = 0;

  const threads = $derived(notesPageDiscussionsInReadingOrder(notes.allCommentThreads)
    .filter((thread) => thread.page_id === pageId));

  $effect(() => {
    const request = focusRequest;
    if (request <= handledFocusRequest) return;
    handledFocusRequest = request;
    void revealComposer();
  });

  async function revealComposer(): Promise<void> {
    await tick();
    composer?.focus();
    root?.querySelector("[data-notes-page-comment-composer]")?.scrollIntoView({ block: "nearest" });
  }

  function submitComment(richText: NotesRichText[], attachments: Record<string, unknown>[]): Promise<boolean> {
    return notes.createComment(richText, attachments, { type: "page_id", page_id: pageId });
  }

  /** Loads the comments again, rejecting while they are still unavailable. */
  async function retryComments(): Promise<void> {
    await notes.reloadComments();
    if (notes.commentsError) throw new Error(notes.commentsError);
  }

  function leaveEmpty(): void {
    if (threads.length === 0) onLeaveEmpty();
  }
</script>

<section bind:this={root} {id} class="notes-page-comments min-w-0 text-collection" aria-labelledby={headingId}>
  <h2 id={headingId} class="mb-1.5 text-panel-detail font-medium text-muted-foreground">{t("notes.commentsHeading")}</h2>
  {#if notes.commentsError}
    <Notice class="mb-2 text-panel-detail" message={t("notes.loadCommentsFailed", notes.commentsError)} copy={notes.commentsError} onRetry={retryComments} />
  {/if}
  {#if threads.length > 0}
    <div class="flex min-w-0 flex-col gap-2">
      {#each threads as thread (thread.id)}
        <NotesCommentThreadView {thread} {now} {timeLabels} replyMode="on-demand" resolveControl />
      {/each}
    </div>
  {/if}
  <div class={threads.length > 0 ? "mt-2" : ""} data-notes-page-comment-composer>
    <NotesCommentComposer
      bind:this={composer}
      label={t("notes.addComment")}
      placeholder={t("notes.commentPlaceholder")}
      onSubmit={submitComment}
      onLeaveEmpty={leaveEmpty}
    />
  </div>
</section>
