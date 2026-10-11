<script lang="ts" module>
  /** A discussion to reveal when the panel opens or is already open. */
  export interface NotesDiscussionsPanelFocus {
    /** Null only moves focus into the panel. */
    discussionId: string | null;
    /** Moves focus into the discussion's reply composer instead of the discussion itself. */
    reply: boolean;
    /** Increments for every request so repeating one reveals the discussion again. */
    request: number;
  }
</script>

<script lang="ts">
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { blockPlainText } from "$lib/notes/blocks/factory";
  import {
    filterNotesDiscussions,
    groupNotesDiscussionsByDay,
    NOTES_DISCUSSION_FILTERS,
    notesDiscussionQuote,
    notesDiscussionQuoteText,
    type NotesDiscussionDayGroup,
    type NotesDiscussionFilter,
  } from "$lib/notes/collaboration/discussions";
  import type { NotesCommentThread } from "$lib/notes/contracts/collaboration";
  import { notesEditorScrollTopForTarget } from "$lib/notes/editor/scroll";
  import type { NotesActivityTimeLabels } from "$lib/notes/pages/activity";
  import type { NotesPageIcon as PageIconValue, NotesRichText } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import { Temporal } from "@js-temporal/polyfill";
  import Bell from "@lucide/svelte/icons/bell";
  import BellOff from "@lucide/svelte/icons/bell-off";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import X from "@lucide/svelte/icons/x";
  import { onDestroy, tick } from "svelte";
  import NotesCommentComposer from "./NotesCommentComposer.svelte";
  import NotesCommentThreadView from "./NotesCommentThreadView.svelte";
  import NotesPageNotificationLevels from "./NotesPageNotificationLevels.svelte";

  /** Space kept between a revealed discussion and the panel edges. */
  const REVEAL_PADDING_PX = 12;
  /** How long a discussion opened from elsewhere stays highlighted. */
  const HIGHLIGHT_MS = 1_600;

  /**
   * Every discussion on the page beside the editor: a status filter, page notification settings, discussions grouped by
   * the day of their last activity, and a composer for a block comment that is being started. The panel overlays the
   * page when the editor is too narrow to show both.
   */
  let {
    pageId,
    pageTitle,
    pageIcon,
    now,
    timeLabels,
    focus = null,
    mobile = false,
    topInset = false,
    onClose,
  }: {
    pageId: string;
    pageTitle: string;
    pageIcon: PageIconValue | null;
    now: Date;
    timeLabels: NotesActivityTimeLabels;
    focus?: NotesDiscussionsPanelFocus | null;
    mobile?: boolean;
    /** Leaves room above the header for the peek controls that float over the editor's top edge. */
    topInset?: boolean;
    onClose: () => void;
  } = $props();

  const notes = getNotesEditor();
  const localization = getLocalization();
  const { t } = localization;
  const id = $props.id();
  const headingId = `${id}-heading`;
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";

  let root = $state<HTMLElement | null>(null);
  let scrollBody = $state<HTMLElement | null>(null);
  let newComposer = $state<ReturnType<typeof NotesCommentComposer> | null>(null);
  let filter = $state<NotesDiscussionFilter>("open");
  let pendingFocus = $state<NotesDiscussionsPanelFocus | null>(null);
  let highlightedId = $state<string | null>(null);
  let highlightTimer: ReturnType<typeof setTimeout> | null = null;
  let handledFocusRequest = 0;
  let focusedNewParentKey: string | null = null;
  /** Thread views by discussion, kept for focusing a reply composer. */
  const threadViews: Record<string, ReturnType<typeof NotesCommentThreadView> | null> = {};

  const filterOptions = $derived(NOTES_DISCUSSION_FILTERS.map((value) => ({ value, label: filterLabel(value) })));
  const pageThreads = $derived(notes.allCommentThreads.filter((thread) => thread.page_id === pageId));
  const visibleThreads = $derived(filterNotesDiscussions(pageThreads, filter));
  const today = $derived(Temporal.Instant.fromEpochMilliseconds(now.getTime()).toZonedDateTimeISO(timeZone).toPlainDate());
  const groups = $derived(groupNotesDiscussionsByDay(visibleThreads, { today, timeZone }));
  const newBlockId = $derived(notes.activeCommentParent?.type === "block_id" ? notes.activeCommentParent.block_id : null);
  const newQuote = $derived(newBlockId ? pendingQuote(newBlockId) : null);
  const notificationsOff = $derived(notes.pageNotificationLevel === "none");

  $effect(() => {
    const request = focus;
    if (!request || request.request <= handledFocusRequest) return;
    handledFocusRequest = request.request;
    pendingFocus = request;
  });

  $effect(() => {
    const target = pendingFocus;
    if (!target) return;
    if (!target.discussionId) {
      pendingFocus = null;
      void tick().then(() => root?.focus({ preventScroll: true }));
      return;
    }
    const thread = pageThreads.find((item) => item.id === target.discussionId);
    if (!thread) return;
    if (thread.comments.length === 0) {
      pendingFocus = null;
      return;
    }
    if (!visibleThreads.some((item) => item.id === thread.id)) {
      filter = "all";
      return;
    }
    pendingFocus = null;
    void revealThread(thread.id, target.reply);
  });

  $effect(() => {
    const blockId = newBlockId;
    const key = blockId ? `${blockId}:${notes.activeCommentAnchor?.start ?? ""}:${notes.activeCommentAnchor?.end ?? ""}` : null;
    if (key === focusedNewParentKey) return;
    focusedNewParentKey = key;
    if (key) void revealNewComposer();
  });

  onDestroy(() => {
    if (highlightTimer) clearTimeout(highlightTimer);
  });

  function filterLabel(value: NotesDiscussionFilter): string {
    switch (value) {
      case "open": return t("notes.discussionsFilterOpen");
      case "resolved": return t("notes.discussionsFilterResolved");
      case "all": return t("notes.discussionsFilterAll");
    }
  }

  function emptyLabel(value: NotesDiscussionFilter): string {
    switch (value) {
      case "open": return t("notes.discussionsEmptyOpen");
      case "resolved": return t("notes.discussionsEmptyResolved");
      case "all": return t("notes.discussionsEmptyAll");
    }
  }

  function chooseFilter(value: string): void {
    const next = NOTES_DISCUSSION_FILTERS.find((item) => item === value);
    if (next) filter = next;
  }

  function dayLabel(group: NotesDiscussionDayGroup): string {
    if (group.relative === "today") return t("notes.discussionsToday");
    if (group.relative === "yesterday") return t("notes.discussionsYesterday");
    const day = Temporal.PlainDate.from(group.day);
    return formatDateTime(localization.locale, Date.UTC(day.year, day.month - 1, day.day, 12), {
      month: "long",
      day: "numeric",
      ...(day.year === today.year ? {} : { year: "numeric" }),
      timeZone: "UTC",
    });
  }

  function blockText(blockId: string | null): string | null {
    if (!blockId) return null;
    const block = notes.blockById(blockId);
    return block ? blockPlainText(block) : null;
  }

  function threadQuote(thread: NotesCommentThread): string | null {
    return notesDiscussionQuote(thread, blockText(thread.block_id));
  }

  function pendingQuote(blockId: string): string | null {
    return notesDiscussionQuoteText(notes.activeCommentAnchor?.text || blockText(blockId) || "");
  }

  function goToBlock(blockId: string): void {
    notes.focusBlock(blockId);
    if (mobile) onClose();
  }

  /** Scroll only the panel body, so revealing a discussion never shifts the page around it. */
  function scrollIntoPanel(element: HTMLElement): void {
    const viewport = scrollBody;
    if (!viewport) return;
    const viewportRect = viewport.getBoundingClientRect();
    const targetRect = element.getBoundingClientRect();
    viewport.scrollTop = notesEditorScrollTopForTarget({
      scrollTop: viewport.scrollTop,
      maxScrollTop: viewport.scrollHeight - viewport.clientHeight,
      viewportTop: viewportRect.top,
      viewportBottom: viewportRect.bottom,
      targetTop: targetRect.top,
      targetBottom: targetRect.bottom,
      padding: REVEAL_PADDING_PX,
      alignment: "nearest",
    });
  }

  function highlight(discussionId: string): void {
    highlightedId = discussionId;
    if (highlightTimer) clearTimeout(highlightTimer);
    highlightTimer = setTimeout(() => {
      highlightedId = null;
      highlightTimer = null;
    }, HIGHLIGHT_MS);
  }

  async function revealThread(discussionId: string, reply: boolean): Promise<void> {
    await tick();
    const card = scrollBody?.querySelector<HTMLElement>(`[data-notes-discussion-card="${CSS.escape(discussionId)}"]`);
    if (!card) return;
    scrollIntoPanel(card);
    highlight(discussionId);
    if (reply) await threadViews[discussionId]?.focusReply();
    else card.focus({ preventScroll: true });
  }

  async function revealNewComposer(): Promise<void> {
    await tick();
    if (scrollBody) scrollBody.scrollTop = 0;
    requestAnimationFrame(() => newComposer?.focus());
  }

  function submitNewComment(richText: NotesRichText[], attachments: Record<string, unknown>[]): Promise<boolean> {
    return notes.createComment(richText, attachments);
  }

  function cancelNewComment(): void {
    notes.setActiveCommentParent(null);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onClose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  bind:this={root}
  class={cn(
    "discussions-panel flex min-h-0 flex-col outline-none",
    mobile && "discussions-panel-mobile",
    topInset && "discussions-panel-inset",
  )}
  aria-labelledby={headingId}
  tabindex="-1"
  data-notes-discussions-panel
  onkeydown={handleKeydown}
>
  <header class={cn("flex shrink-0 items-center gap-1 border-b border-border/70", mobile ? "min-h-14 px-2" : "h-11 pr-2 pl-4")}>
    {#if mobile}
      <button type="button" class="panel-icon-button mobile" aria-label={t("notes.discussionsClose")} onclick={onClose}>
        <ChevronLeft class="size-5" aria-hidden="true" />
      </button>
    {/if}
    <h2 id={headingId} class="min-w-0 flex-1 truncate font-semibold">{t("notes.allDiscussions")}</h2>
    <Select
      value={filter}
      options={filterOptions}
      onChange={chooseFilter}
      ariaLabel={t("notes.discussionsFilter")}
      appearance="quiet"
      textSize="collection"
      popoverAlign="end"
      inline
      class="w-auto shrink-0"
    />
    <CollectionMenu
      kind="actions"
      iconOnly
      showHeader={false}
      icon={notificationsOff ? BellOff : Bell}
      label={t("notes.pageNotifications")}
      triggerClass={cn("shrink-0 justify-center px-0 text-muted-foreground hover:text-foreground", mobile ? "size-11" : "size-7")}
    >
      <NotesPageNotificationLevels />
    </CollectionMenu>
    {#if !mobile}
      <button type="button" class="panel-icon-button" aria-label={t("notes.discussionsClose")} data-app-tooltip={t("notes.discussionsClose")} onclick={onClose}>
        <X class="size-4" aria-hidden="true" />
      </button>
    {/if}
  </header>

  <div bind:this={scrollBody} class={cn("min-h-0 flex-1 overflow-y-auto overscroll-contain", mobile ? "px-4 pb-6" : "px-4 pb-4")}>
    {#if newBlockId}
      <section class="discussion-card new mt-3" aria-label={t("notes.newDiscussion")}>
        <div class="mb-1.5 flex min-w-0 items-start gap-2">
          <p class="min-w-0 flex-1 text-panel-detail font-medium text-muted-foreground">{t("notes.newDiscussion")}</p>
          <button type="button" class="panel-icon-button small" aria-label={t("notes.cancelNewDiscussion")} data-app-tooltip={t("notes.cancelNewDiscussion")} onclick={cancelNewComment}>
            <X class="size-3.5" aria-hidden="true" />
          </button>
        </div>
        {#if newQuote}
          <p class="discussion-quote mb-2">{newQuote}</p>
        {/if}
        <NotesCommentComposer
          bind:this={newComposer}
          label={t("notes.addComment")}
          placeholder={t("notes.commentPlaceholder")}
          onSubmit={submitNewComment}
          onCancel={cancelNewComment}
        />
      </section>
    {/if}

    {#if notes.commentsError}
      <p class="mt-3 flex flex-wrap items-center gap-2 text-panel-detail text-destructive" role="alert">
        <span>{t("notes.loadCommentsFailed", notes.commentsError)}</span>
        <button type="button" class="rounded-md px-1.5 py-0.5 text-foreground hover:bg-accent" onclick={() => void notes.reloadComments()}>
          {t("common.retry")}
        </button>
      </p>
    {/if}

    {#if groups.length === 0}
      {#if notes.commentsLoading}
        <p class="mt-6 text-center text-panel-detail text-muted-foreground" aria-busy="true">{t("common.loading")}</p>
      {:else if !notes.commentsError && !newBlockId}
        <p class="mt-6 text-center text-panel-detail text-muted-foreground">{emptyLabel(filter)}</p>
      {/if}
    {:else}
      {#each groups as group (group.day)}
        <section class="mt-3" aria-label={dayLabel(group)}>
          <h3 class="mb-1 text-panel-detail font-medium text-muted-foreground">{dayLabel(group)}</h3>
          <div class="flex flex-col gap-2">
            {#each group.threads as thread (thread.id)}
              {@const quote = thread.parent.type === "block_id" ? threadQuote(thread) : null}
              {@const blockId = thread.parent.type === "block_id" ? thread.parent.block_id : null}
              <article
                class="discussion-card"
                class:resolved={thread.status === "resolved"}
                aria-label={quote ?? pageTitle}
                tabindex="-1"
                data-notes-discussion-card={thread.id}
                data-highlighted={highlightedId === thread.id || undefined}
              >
                <div class="mb-1 flex min-w-0 items-start gap-2">
                  {#if quote && blockId}
                    <button
                      type="button"
                      class="discussion-quote min-w-0 flex-1 text-left"
                      aria-label={t("notes.discussionGoToBlock", quote)}
                      onclick={() => goToBlock(blockId)}
                    >{quote}</button>
                  {:else}
                    <p class="flex min-w-0 flex-1 items-center gap-1.5 text-panel-detail text-muted-foreground">
                      <NotesPageIcon icon={pageIcon} size={14} class="shrink-0" />
                      <span class="truncate">{pageTitle}</span>
                    </p>
                  {/if}
                  {#if thread.status === "resolved"}
                    <span class="shrink-0 rounded-sm bg-accent px-1.5 text-panel-detail text-muted-foreground">{t("notes.discussionResolved")}</span>
                  {/if}
                </div>
                <NotesCommentThreadView
                  bind:this={threadViews[thread.id]}
                  {thread}
                  {now}
                  {timeLabels}
                  replyMode="always"
                  resolveControl
                />
              </article>
            {/each}
          </div>
        </section>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .discussions-panel {
    position: relative;
    z-index: 20;
    width: min(22.5rem, 100%);
    flex: 0 0 auto;
    border-left: 1px solid var(--border);
    background-color: var(--cal-bg);
    font-size: var(--panel-font-size);
  }
  @container notes-editor-body (width < 46rem) {
    .discussions-panel {
      position: absolute;
      inset-block: 0;
      right: 0;
      box-shadow: var(--floating-shadow);
    }
  }
  @container notes-editor-body (width < 26rem) {
    .discussions-panel {
      width: 100%;
      border-left: 0;
      box-shadow: none;
    }
  }
  .discussions-panel.discussions-panel-inset {
    padding-top: 2.75rem;
  }
  .discussions-panel.discussions-panel-mobile {
    position: absolute;
    inset: 0;
    width: 100%;
    border-left: 0;
    box-shadow: none;
  }
  .panel-icon-button {
    display: inline-grid;
    width: 1.75rem;
    height: 1.75rem;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0.375rem;
    color: var(--muted-foreground);
  }
  .panel-icon-button.small {
    width: 1.375rem;
    height: 1.375rem;
  }
  .panel-icon-button.mobile {
    width: 2.75rem;
    height: 2.75rem;
  }
  .panel-icon-button:hover {
    background-color: var(--accent);
    color: var(--foreground);
  }
  .discussion-card {
    border-radius: var(--floating-radius);
    padding: 0.5rem 0.25rem 0.25rem;
    outline: none;
    transition: background-color 400ms ease;
  }
  .discussion-card.new {
    border: 1px solid var(--border);
    padding: 0.5rem 0.625rem 0.625rem;
  }
  .discussion-card.resolved :global(.comment-body) {
    color: var(--muted-foreground);
  }
  .discussion-card[data-highlighted] {
    background-color: color-mix(in srgb, var(--accent) 70%, transparent);
  }
  .discussion-quote {
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
  button.discussion-quote:hover {
    color: var(--foreground);
  }
  @media (prefers-reduced-motion: reduce) {
    .discussion-card {
      transition: none;
    }
  }
</style>
