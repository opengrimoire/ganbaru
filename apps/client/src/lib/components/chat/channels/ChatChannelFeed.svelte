<script lang="ts">
  import { tick } from "svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { unreadMessageStartIndex } from "$lib/chat/timeline/organizational-message-model";
  import { organizationalScrollFollowsEnd } from "$lib/chat/timeline/organizational-scroll";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import ChatMessageComposer from "$lib/components/chat/composer/ChatMessageComposer.svelte";
  import ChatOrganizationalMessage from "$lib/components/chat/timeline/ChatOrganizationalMessage.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let { onThreadOpened = () => undefined }: { onThreadOpened?: (trigger: HTMLElement) => void } = $props();
  const chat = useChat();
  const localization = getLocalization();
  const { t } = localization;
  let feed = $state<HTMLDivElement | null>(null);
  let followingEnd = true;
  let restoredDestination: string | null = null;
  const channel = $derived(chat.selectedChannel);
  const destination = $derived(channel ? `channel:${channel.id}` : "channel:none");
  const unreadStart = $derived(unreadMessageStartIndex(
    chat.channelMessages,
    channel?.unreadCount ?? 0,
  ));

  function isSameDay(left: string, right: string): boolean {
    return new Date(left).toDateString() === new Date(right).toDateString();
  }

  function shouldGroupWithPrevious(index: number): boolean {
    const current = chat.channelMessages[index];
    const previous = chat.channelMessages[index - 1];
    if (!current || !previous) return false;
    return current.author !== null && current.author.id === previous.author?.id
      && isSameDay(current.createdAt, previous.createdAt)
      && Date.parse(current.createdAt) - Date.parse(previous.createdAt) < 5 * 60_000;
  }

  async function openThread(threadId: string, trigger: HTMLElement): Promise<void> {
    onThreadOpened(trigger);
    await chat.openReplyThread(threadId);
  }

  function rememberScrollPosition(): void {
    if (!feed) return;
    followingEnd = organizationalScrollFollowsEnd(feed);
    chat.setOrganizationalScrollPosition(destination, feed.scrollTop);
  }

  function scrollToBottomForUserAction(): void {
    const element = feed;
    const key = destination;
    if (!element) return;
    followingEnd = true;
    void tick().then(() => {
      if (element !== feed || key !== destination) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
    });
  }

  function followNewContentIfAtBottom(): void {
    const element = feed;
    const key = destination;
    if (!element || restoredDestination !== key || !followingEnd) return;
    void tick().then(() => {
      if (element !== feed || key !== destination || !followingEnd) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
    });
  }

  $effect(() => {
    const element = feed;
    const key = destination;
    if (!element) return;
    restoredDestination = null;
    void tick().then(() => {
      if (element !== feed || key !== destination) return;
      element.scrollTop = chat.organizationalScrollPositions[key] ?? element.scrollHeight;
      followingEnd = organizationalScrollFollowsEnd(element);
      restoredDestination = key;
    });
  });

  $effect(() => {
    const element = feed;
    const key = destination;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (restoredDestination !== key || !followingEnd) return;
      element.scrollTop = element.scrollHeight;
      chat.setOrganizationalScrollPosition(key, element.scrollTop);
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
  <div
    bind:this={feed}
    class="channel-feed"
    role="feed"
    aria-busy={chat.channelMessagesLoading}
    onpointerdown={() => void chat.markSelectedChannelRead()}
    onscroll={rememberScrollPosition}
  >
    <div class="feed-canvas">
      {#if channel}
        <header class="channel-introduction">
          <h1>#{channel.name}</h1>
          <p>{channel.topic || t("chat.channels.welcomeDescription")}</p>
        </header>
      {/if}
      {#if chat.channelMessagesError}<Notice class="feed-error" message={chat.channelMessagesError} onRetry={retryChannelMessages} />{/if}
      {#if chat.channelPages[0]?.previousCursor}<button type="button" class="load-older" onclick={() => void chat.loadOlderChannelMessages()}>{t("chat.organization.loadOlder")}</button>{/if}
      {#each chat.channelMessages as message, index (message.itemId)}
        {#if index === unreadStart && (channel?.unreadCount ?? 0) > 0}<div class="divider unread"><span>{t("chat.organization.unreadMessages")}</span></div>{/if}
        {#if index === 0 || !isSameDay(chat.channelMessages[index - 1].createdAt, message.createdAt)}
          <div class="divider" class:first-date={index === 0}><span>{formatDateTime(localization.locale, Date.parse(message.createdAt), { dateStyle: "full" })}</span></div>
        {/if}
        <ChatOrganizationalMessage {message} grouped={shouldGroupWithPrevious(index)} onOpenThread={(trigger) => { if (message.replyThread) void openThread(message.replyThread.id, trigger); }} />
      {/each}
      {#if chat.channelMessagesLoading && chat.channelMessages.length === 0}<Loading layout="fill" />{/if}
    </div>
  </div>
  {#if channel && !channel.archivedAt}
    <div class="channel-composer-dock">
      {#key destination}
        <ChatMessageComposer {destination} placeholder={t("chat.organization.messageChannel", channel.name)} onRequestScrollToBottom={scrollToBottomForUserAction} />
      {/key}
    </div>
  {/if}
</section>

<style>
  .channel-surface { display:flex; min-height:0; flex:1; flex-direction:column; overflow:hidden; }
  .channel-feed { min-height:0; flex:1; overflow-y:scroll; overscroll-behavior:contain; }
  .feed-canvas { --channel-feed-section-space:1.2rem; --chat-message-row-padding-inline:0rem; width:min(calc(100% - var(--chat-conversation-gutter,1rem) - var(--chat-conversation-gutter,1rem)),var(--chat-conversation-max-width,60rem)); min-height:100%; margin-inline:auto; padding-bottom:1rem; }
  .channel-introduction { width:100%; padding-top:var(--channel-feed-section-space); }
  .channel-introduction h1 { font-size: calc(1.25rem * var(--type-scale)); font-weight:700; }
  .channel-introduction p { margin-top:0.3rem; color:var(--muted-foreground); font-size: calc(0.82rem * var(--type-scale)); line-height: calc(1.35rem * var(--type-scale)); }
  .divider { display:flex; align-items:center; gap:0.5rem; margin:0.75rem 0; color:var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); }
  .divider.first-date { margin-top:var(--channel-feed-section-space); margin-bottom:calc(var(--channel-feed-section-space) - var(--chat-conversation-entry-space,0.45rem)); }
  .divider::before,.divider::after { height:1px; flex:1; background:var(--border); content:""; }.divider.unread { color:var(--primary); }.divider.unread::before,.divider.unread::after { background:color-mix(in srgb,var(--primary) 55%,var(--border)); }
  .load-older { display:block; margin:0.6rem auto; border-radius:0.4rem; padding:0.3rem 0.55rem; color:var(--muted-foreground); font-size: calc(0.7rem * var(--type-scale)); }.load-older:hover { background:var(--accent); }
  .channel-composer-dock { display:flex; flex:0 0 auto; justify-content:center; padding:0.5rem var(--chat-conversation-gutter,1rem) 0.75rem; background:linear-gradient(to bottom,transparent,var(--cal-bg) 18%); }
  .feed-canvas > :global(.feed-error) { width:fit-content; max-width:100%; margin-inline:auto; padding:1rem; font-size: calc(0.78rem * var(--type-scale)); }
  @container chat-shell (max-width:440px) { .channel-surface { --chat-conversation-gutter:0.5rem; } }
</style>
