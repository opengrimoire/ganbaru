<script lang="ts">
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { blockPlainText } from "$lib/notes/blocks/factory";
  import { notesCommentBodyRichText } from "$lib/notes/collaboration/comments";
  import { filterNotesDiscussions, notesDiscussionQuote } from "$lib/notes/collaboration/discussions";
  import { formatNotesActivityTime, type NotesActivityTimeLabels } from "$lib/notes/pages/activity";
  import Bell from "@lucide/svelte/icons/bell";
  import BellOff from "@lucide/svelte/icons/bell-off";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import { tick } from "svelte";
  import NotesCommentAvatar from "./NotesCommentAvatar.svelte";
  import NotesPageNotificationLevels from "./NotesPageNotificationLevels.svelte";

  const AVATAR_SIZE = 20;

  /**
   * The page's discussion summary under the header button: the latest comment of the most recently active open
   * discussion, the page's notification settings, and a way into the All discussions panel.
   */
  let {
    anchor,
    pageId,
    now,
    timeLabels,
    onClose,
    onSeeAll,
    onOpenDiscussion,
  }: {
    anchor: HTMLElement;
    pageId: string;
    now: Date;
    timeLabels: NotesActivityTimeLabels;
    onClose: () => void;
    onSeeAll: () => void;
    /** Opens the All discussions panel on one discussion, optionally ready to reply. */
    onOpenDiscussion: (discussionId: string, reply: boolean) => void;
  } = $props();

  const notes = getNotesEditor();
  const localization = getLocalization();
  const { t } = localization;
  const id = $props.id();
  const headingId = `${id}-heading`;

  let root = $state<HTMLElement | null>(null);
  let view = $state<"latest" | "notifications">("latest");

  const openThreads = $derived(filterNotesDiscussions(
    notes.allCommentThreads.filter((thread) => thread.page_id === pageId),
    "open",
  ));
  const thread = $derived(openThreads[0] ?? null);
  const comment = $derived(thread?.comments.at(-1) ?? null);
  const quote = $derived.by(() => {
    if (!thread?.block_id) return null;
    const block = notes.blockById(thread.block_id);
    return notesDiscussionQuote(thread, block ? blockPlainText(block) : null);
  });
  const authorName = $derived(comment?.display_name.resolved_name.trim() || t("notes.commentAuthorUnknown"));
  const ownComment = $derived(comment !== null && notes.localUser !== null && comment.created_by.id === notes.localUser.id);
  const body = $derived(comment ? notesCommentBodyRichText(comment.rich_text) : []);
  const replies = $derived(thread ? thread.comments.length - 1 : 0);
  const timeLabel = $derived(comment
    ? formatNotesActivityTime(comment.created_time, { locale: localization.locale, now, labels: timeLabels })
    : "");

  async function showView(next: "latest" | "notifications"): Promise<void> {
    view = next;
    await tick();
    root?.querySelector<HTMLElement>("[data-discussions-view-focus]")?.focus({ preventScroll: true });
  }
</script>

<div
  bind:this={root}
  class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
  role="dialog"
  aria-labelledby={headingId}
  tabindex="-1"
  data-floating-root
  data-app-floating-surface
  data-notes-discussions-popover
  use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign: "end", onDismiss: onClose }}
>
  {#if view === "latest"}
    <header class="flex min-h-10 items-center gap-1 pt-1.5 pr-1.5 pl-3">
      <h2 id={headingId} class="min-w-0 flex-1 truncate font-semibold">{t("notes.discussions")}</h2>
      <button
        type="button"
        class="popover-icon-button"
        aria-label={t("notes.pageNotifications")}
        data-app-tooltip={t("notes.pageNotifications")}
        onclick={() => void showView("notifications")}
      >
        {#if notes.pageNotificationLevel === "none"}
          <BellOff class="size-4" aria-hidden="true" />
        {:else}
          <Bell class="size-4" aria-hidden="true" />
        {/if}
      </button>
      <button
        type="button"
        class="h-7 shrink-0 rounded-md px-2 text-muted-foreground hover:bg-accent hover:text-foreground"
        data-discussions-view-focus
        onclick={onSeeAll}
      >{t("notes.discussionsSeeAll")}</button>
    </header>
    {#if thread && comment}
      <div class="relative mx-1.5 mt-1 mb-1.5 rounded-floating-item px-1.5 py-1.5 hover:bg-accent/60">
        <button
          type="button"
          class="absolute inset-0 rounded-floating-item"
          aria-label={`${t("notes.discussionOpenThread")}: ${authorName}`}
          onclick={() => onOpenDiscussion(thread.id, false)}
        ></button>
        <div class="pointer-events-none relative min-w-0">
          {#if quote}
            <p class="popover-quote mb-1.5">{quote}</p>
          {/if}
          <div class="flex min-w-0 items-center gap-2">
            <NotesCommentAvatar own={ownComment} name={authorName} size={AVATAR_SIZE} />
            <span class="min-w-0 truncate font-medium">{authorName}</span>
            <time class="shrink-0 text-panel-detail text-muted-foreground" datetime={comment.created_time}>{timeLabel}</time>
          </div>
          {#if body.length > 0}
            <div class="popover-body mt-1 min-w-0 wrap-break-word" style={`padding-left:${AVATAR_SIZE + 8}px`}>
              <NotesRichTextInline richText={body} />
            </div>
          {:else if comment.attachments.length > 0}
            <p class="mt-1 flex items-center gap-1 text-panel-detail text-muted-foreground" style={`padding-left:${AVATAR_SIZE + 8}px`}>
              <Paperclip class="size-3.5" aria-hidden="true" />
              {t("notes.commentAttachmentUnavailable")}
            </p>
          {/if}
        </div>
        <div class="relative mt-1.5 flex items-center gap-2" style={`padding-left:${AVATAR_SIZE + 8}px`}>
          {#if replies > 0}
            <span class="pointer-events-none text-panel-detail text-muted-foreground">{t("notes.discussionRepliesCount", replies)}</span>
          {/if}
          <button
            type="button"
            class="ml-auto h-6 rounded-md px-1.5 text-panel-detail text-muted-foreground hover:bg-accent hover:text-foreground"
            onclick={() => onOpenDiscussion(thread.id, true)}
          >{t("notes.reply")}</button>
        </div>
      </div>
    {:else}
      <p class="px-3 pt-1 pb-3 text-panel-detail text-muted-foreground">{t("notes.discussionsEmptyOpen")}</p>
    {/if}
  {:else}
    <header class="flex min-h-10 items-center gap-1 pt-1.5 pr-3 pl-1.5">
      <button
        type="button"
        class="popover-icon-button"
        aria-label={t("common.back")}
        data-discussions-view-focus
        onclick={() => void showView("latest")}
      >
        <ChevronLeft class="size-4" aria-hidden="true" />
      </button>
      <h2 id={headingId} class="min-w-0 flex-1 truncate font-semibold">{t("notes.pageNotifications")}</h2>
    </header>
    <div class="surface-floating-body pt-0">
      <NotesPageNotificationLevels showHeading={false} />
    </div>
  {/if}
</div>

<style>
  .popover-icon-button {
    display: inline-grid;
    width: 1.75rem;
    height: 1.75rem;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0.375rem;
    color: var(--muted-foreground);
  }
  .popover-icon-button:hover {
    background-color: var(--accent);
    color: var(--foreground);
  }
  .popover-quote {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    border-left: 2px solid color-mix(in srgb, var(--primary) 55%, var(--border));
    padding-left: 0.5rem;
    color: var(--muted-foreground);
    font-size: var(--panel-detail-font-size);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  /* Rich text renders one block per line, so the preview caps its height instead of clamping a single text run. */
  .popover-body {
    max-height: 6em;
    overflow: hidden;
    line-height: 1.5;
  }
</style>
