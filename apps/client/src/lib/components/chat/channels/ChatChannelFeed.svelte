<script lang="ts">
  import { tick, untrack } from "svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { buildChannelFeedRows, channelFeedMessageRowId, channelFeedScrollAnchorRowId } from "$lib/chat/timeline/channel-feed-rows";
  import { unreadMessageStartIndex } from "$lib/chat/timeline/organizational-message-model";
  import { organizationalScrollFollowsEnd } from "$lib/chat/timeline/organizational-scroll";
  import { computeTimelineVirtualWindow, scrollTopForPreservedAnchor, virtualRowTop } from "$lib/chat/timeline/virtualization";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import ChatMessageComposer from "$lib/components/chat/composer/ChatMessageComposer.svelte";
  import ChatOrganizationalMessage from "$lib/components/chat/timeline/ChatOrganizationalMessage.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /** Height assumed for a feed row until it is measured. */
  const MESSAGE_ROW_ESTIMATE_PX = 72;
  /** Extra rows rendered above and below the viewport, in pixels. */
  const FEED_OVERSCAN_PX = 600;

  let { onThreadOpened = () => undefined }: { onThreadOpened?: (trigger: HTMLElement) => void } = $props();
  const chat = useChat();
  const localization = getLocalization();
  const { t } = localization;
  let feed = $state<HTMLDivElement | null>(null);
  let rowList = $state<HTMLDivElement | null>(null);
  let followingEnd = true;
  let restoredDestination: string | null = null;
  let actionError = $state<string | null>(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let rowListOffset = $state(0);
  let measuredHeights = $state<ReadonlyMap<string, number>>(new Map());
  let rowObserver: ResizeObserver | null = null;
  const channel = $derived(chat.selectedChannel);
  const destination = $derived(channel ? `channel:${channel.id}` : "channel:none");
  const unreadStart = $derived(unreadMessageStartIndex(
    chat.channelMessages,
    channel?.unreadCount ?? 0,
  ));
  const rows = $derived(buildChannelFeedRows(
    chat.channelMessages,
    unreadStart,
    channel?.unreadCount ?? 0,
    localization.locale,
  ));
  const virtualWindow = $derived(computeTimelineVirtualWindow(
    rows,
    measuredHeights,
    scrollTop - rowListOffset,
    viewportHeight,
    MESSAGE_ROW_ESTIMATE_PX,
    FEED_OVERSCAN_PX,
  ));

  async function openThread(threadId: string, trigger: HTMLElement): Promise<void> {
    onThreadOpened(trigger);
    await chat.openReplyThread(threadId);
  }

  /** Reads the scroll geometry the virtual window depends on. */
  function syncViewport(element: HTMLDivElement): void {
    scrollTop = element.scrollTop;
    viewportHeight = element.clientHeight;
    if (rowList) {
      rowListOffset = rowList.getBoundingClientRect().top - element.getBoundingClientRect().top + element.scrollTop;
    }
  }

  function setFeedScrollTop(element: HTMLDivElement, key: string, value: number): void {
    element.scrollTop = value;
    syncViewport(element);
    chat.setOrganizationalScrollPosition(key, element.scrollTop);
  }

  function rememberScrollPosition(): void {
    if (!feed) return;
    followingEnd = organizationalScrollFollowsEnd(feed);
    syncViewport(feed);
    chat.setOrganizationalScrollPosition(destination, feed.scrollTop);
  }

  function scrollToBottomForUserAction(): void {
    const element = feed;
    const key = destination;
    if (!element) return;
    followingEnd = true;
    void tick().then(() => {
      if (element !== feed || key !== destination) return;
      setFeedScrollTop(element, key, element.scrollHeight);
    });
  }

  function followNewContentIfAtBottom(): void {
    const element = feed;
    const key = destination;
    if (!element || restoredDestination !== key || !followingEnd) return;
    void tick().then(() => {
      if (element !== feed || key !== destination || !followingEnd) return;
      setFeedScrollTop(element, key, element.scrollHeight);
    });
  }

  function reportActionError(error: unknown): void {
    actionError = chatErrorMessage(error);
  }

  /**
   * Records measured row heights. A row that starts above the viewport moves the content below it, so the scroll
   * offset absorbs its change and the visible message stays in place; a feed that follows its end stays pinned.
   */
  function recordRowHeights(entries: readonly ResizeObserverEntry[]): void {
    const element = feed;
    if (!element) return;
    const viewportTop = element.getBoundingClientRect().top;
    const next = new Map(measuredHeights);
    let shiftAbove = 0;
    let changed = false;
    for (const entry of entries) {
      if (!(entry.target instanceof HTMLElement)) continue;
      const id = entry.target.dataset.feedRowId;
      if (!id) continue;
      const box = entry.target.getBoundingClientRect();
      if (box.height <= 0) continue;
      const assumed = next.get(id) ?? MESSAGE_ROW_ESTIMATE_PX;
      if (assumed === box.height && next.has(id)) continue;
      next.set(id, box.height);
      changed = true;
      if (box.top < viewportTop) shiftAbove += box.height - assumed;
    }
    if (!changed) return;
    measuredHeights = next;
    const key = destination;
    if (restoredDestination !== key) return;
    if (followingEnd) {
      void tick().then(() => {
        if (element !== feed || key !== destination || !followingEnd) return;
        setFeedScrollTop(element, key, element.scrollHeight);
      });
    } else if (shiftAbove !== 0) {
      setFeedScrollTop(element, key, element.scrollTop + shiftAbove);
    }
  }

  /** Observes one rendered feed row with the shared row observer. */
  function observeRow(node: HTMLElement): { destroy(): void } {
    rowObserver?.observe(node);
    return { destroy: () => rowObserver?.unobserve(node) };
  }

  async function loadOlderMessages(): Promise<void> {
    const element = feed;
    const key = destination;
    const anchorId = channelFeedScrollAnchorRowId(virtualWindow.items, scrollTop - rowListOffset);
    const anchorRowTopBefore = anchorId ? virtualRowTop(rows, anchorId, measuredHeights, MESSAGE_ROW_ESTIMATE_PX) : null;
    // The load older button sits above the row list and disappears with the last page, so the anchor is measured from
    // the feed top, including the row list offset.
    const anchorTopBefore = anchorRowTopBefore === null ? null : rowListOffset + anchorRowTopBefore;
    const scrollTopBefore = element?.scrollTop ?? 0;
    try {
      await chat.loadOlderChannelMessages();
    } catch {
      // The store shows the failure as the feed error notice with a retry action.
      return;
    }
    await tick();
    if (!element || element !== feed || key !== destination || !anchorId || anchorTopBefore === null) return;
    syncViewport(element);
    const anchorRowTopAfter = virtualRowTop(rows, anchorId, measuredHeights, MESSAGE_ROW_ESTIMATE_PX);
    if (anchorRowTopAfter === null) return;
    followingEnd = false;
    setFeedScrollTop(
      element,
      key,
      scrollTopForPreservedAnchor(scrollTopBefore, anchorTopBefore, rowListOffset + anchorRowTopAfter),
    );
  }

  $effect(() => {
    destination;
    actionError = null;
  });

  $effect(() => {
    const element = feed;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(recordRowHeights);
    rowObserver = observer;
    for (const row of element.querySelectorAll<HTMLElement>("[data-feed-row-id]")) observer.observe(row);
    return () => {
      observer.disconnect();
      if (rowObserver === observer) rowObserver = null;
    };
  });

  $effect(() => {
    const element = feed;
    const key = destination;
    if (!element) return;
    restoredDestination = null;
    void tick().then(() => {
      if (element !== feed || key !== destination) return;
      element.scrollTop = chat.organizationalScrollPositions[key] ?? element.scrollHeight;
      syncViewport(element);
      followingEnd = organizationalScrollFollowsEnd(element);
      restoredDestination = key;
    });
  });

  $effect(() => {
    const element = feed;
    const key = destination;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      syncViewport(element);
      if (restoredDestination !== key || !followingEnd) return;
      setFeedScrollTop(element, key, element.scrollHeight);
    });
    observer.observe(element);
    return () => observer.disconnect();
  });

  $effect(() => {
    chat.channelMessages;
    followNewContentIfAtBottom();
  });

  $effect(() => {
    const element = feed;
    const anchorId = chat.messageAnchorId;
    if (!element || !anchorId || chat.openReplyThreadId) return;
    const rowId = channelFeedMessageRowId(anchorId);
    const rowTop = virtualRowTop(rows, rowId, untrack(() => measuredHeights), MESSAGE_ROW_ESTIMATE_PX);
    if (rowTop === null) return;
    followingEnd = false;
    element.scrollTop = Math.max(0, untrack(() => rowListOffset) + rowTop - element.clientHeight / 2);
    syncViewport(element);
    void tick().then(() => {
      const target = element.querySelector<HTMLElement>(`[data-message-item-id="${CSS.escape(anchorId)}"]`);
      if (!target) return;
      target.scrollIntoView({ block: "center" });
      target.focus({ preventScroll: true });
      chat.clearMessageAnchor(anchorId);
    });
  });

  /** Reloads the channel history that failed and rejects when the load fails again. */
  function retryChannelMessages(): Promise<void> {
    if (chat.channelMessages.length > 0) return chat.loadOlderChannelMessages();
    if (!channel) return Promise.reject(new Error("No Chat channel is selected to reload"));
    return chat.loadChannelMessages(channel.id, true);
  }
</script>

<section class="channel-surface" aria-label={channel ? `#${channel.name}` : t("chat.title")}>
  {#if actionError}<Notice class="feed-action-error" message={actionError} onDismiss={() => { actionError = null; }} />{/if}
  <div
    bind:this={feed}
    data-mobile-scroll-fade class="channel-feed"
    role="feed"
    aria-busy={chat.channelMessagesLoading}
    onpointerdown={() => void chat.markSelectedChannelRead()}
    onscroll={rememberScrollPosition}
  >
    <div class="feed-canvas">
      {#if chat.channelMessagesError}<Notice class="feed-error" message={chat.channelMessagesError} onRetry={retryChannelMessages} />{/if}
      {#if chat.channelPages[0]?.previousCursor}<button type="button" class="load-older" onclick={() => void loadOlderMessages()}>{t("chat.organization.loadOlder")}</button>{/if}
      <div
        bind:this={rowList}
        class="feed-rows"
        style={`padding-top:${virtualWindow.paddingTop}px;padding-bottom:${virtualWindow.paddingBottom}px`}
      >
        {#each virtualWindow.items as item (item.row.id)}
          {@const row = item.row}
          <div class="feed-row" data-feed-row-id={row.id} use:observeRow>
            {#if row.kind === "unread-divider"}
              <div class="divider unread"><span>{t("chat.organization.unreadMessages")}</span></div>
            {:else if row.kind === "date-divider"}
              <div class="divider" class:first-date={row.first}><span>{row.label}</span></div>
            {:else}
              {@const message = row.message}
              <ChatOrganizationalMessage {message} grouped={row.grouped} onOpenThread={(trigger) => { if (message.replyThread) void openThread(message.replyThread.id, trigger); }} onActionError={reportActionError} />
            {/if}
          </div>
        {/each}
      </div>
      {#if chat.channelMessagesLoading && chat.channelMessages.length === 0}<Loading layout="fill" />{/if}
    </div>
  </div>
  {#if channel && !channel.archivedAt}
    <div class="channel-composer-dock">
      {#key destination}
        <ChatMessageComposer {destination} placeholder={t("chat.organization.messageChannel")} onRequestScrollToBottom={scrollToBottomForUserAction} />
      {/key}
    </div>
  {/if}
</section>

<style>
  .channel-surface { display:flex; min-height:0; flex:1; flex-direction:column; overflow:hidden; }
  .channel-feed { min-height:0; flex:1; overflow-y:scroll; overscroll-behavior:contain; overflow-anchor:none; }
  .feed-canvas { --channel-feed-section-space:1.2rem; --chat-message-row-padding-inline:0rem; width:min(calc(100% - var(--chat-conversation-gutter,1rem) - var(--chat-conversation-gutter,1rem)),var(--chat-conversation-max-width,60rem)); min-height:100%; margin-inline:auto; padding-bottom:1rem; }
  /* Each row is its own block formatting context, so divider margins count toward the measured row height. */
  .feed-row { display:flow-root; }
  .divider { display:flex; align-items:center; gap:0.5rem; margin:0.75rem 0; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .divider.first-date { margin-top:var(--channel-feed-section-space); margin-bottom:calc(var(--channel-feed-section-space) - var(--chat-conversation-entry-space,0.45rem)); }
  .divider::before,.divider::after { height:1px; flex:1; background:var(--border); content:""; }.divider.unread { color:var(--primary); }.divider.unread::before,.divider.unread::after { background:color-mix(in srgb,var(--primary) 55%,var(--border)); }
  .load-older { display:block; margin:0.6rem auto; border-radius:0.4rem; padding:0.3rem 0.55rem; color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }.load-older:hover { background:var(--accent); }
  .channel-composer-dock { display:flex; flex:0 0 auto; justify-content:center; padding:0.5rem var(--chat-conversation-gutter,1rem) 0.75rem; background:linear-gradient(to bottom,transparent,var(--cal-bg) 18%); }
  .channel-surface > :global(.feed-action-error) { flex:0 0 auto; padding:0.6rem; font-size: calc(0.7rem * var(--type-scale)); }
  .feed-canvas > :global(.feed-error) { width:fit-content; max-width:100%; margin-inline:auto; padding:1rem; font-size: calc(0.78rem * var(--type-scale)); }
  @container chat-shell (max-width:440px) { .channel-surface { --chat-conversation-gutter:0.5rem; } }
</style>
