<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Hash from "@lucide/svelte/icons/hash";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import { chatChannelPickerRows, normalizeChatChannelSearch } from "$lib/chat/channel-search";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import StarredIconBadge from "$lib/components/ui/StarredIconBadge.svelte";
  import SidebarOrderMenu from "$lib/components/ui/SidebarOrderMenu.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getChatSidebarOrder } from "$lib/stores/chat/sidebar-order.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { createSidebarOrderDrag } from "$lib/utils/sidebar-order-drag.svelte";
  import { openInNewTabIntent, preventMiddleClickAutoscroll, type OpenInNewTabIntent } from "$lib/workspace-tabs/gestures";


  let {
    rootElement = $bindable<HTMLDivElement | undefined>(),
    channels,
    orderProjectId = null,
    selectedChannelId = null,
    isChannelStarred = () => false,
    frameStyle,
    className = "relative",
    zIndexClass = "",
    iconStrokeWidth = 1.6,
    onChannelSelected,
    onChannelOpenInNewTab = undefined,
    onCreateChannel,
    onLayoutChange = undefined,
    onPointerEnter = undefined,
    onPointerLeave = undefined,
    showSearch = true,
    mobileLayout = false,
    title = undefined,
    onBack = undefined,
    onClose = undefined,
  }: {
    rootElement?: HTMLDivElement;
    channels: readonly ChatChannelRead[];
    /** Project whose channel order the panel reorders and, beside the search, picks; without one both are off. */
    orderProjectId?: string | null;
    selectedChannelId?: string | null;
    /** Whether a channel is a favorite, which shows a small star after its name. */
    isChannelStarred?: (channelId: string) => boolean;
    frameStyle: string;
    className?: string;
    zIndexClass?: string;
    iconStrokeWidth?: number;
    onChannelSelected: (channel: ChatChannelRead) => MaybePromise<void>;
    /** Opens an active channel in a new workspace tab; rows leave out the gesture without it. */
    onChannelOpenInNewTab?: (channel: ChatChannelRead, intent: OpenInNewTabIntent) => void;
    onCreateChannel: () => MaybePromise<void>;
    onLayoutChange?: () => void;
    onPointerEnter?: (event: PointerEvent) => void;
    onPointerLeave?: (event: PointerEvent) => void;
    showSearch?: boolean;
    mobileLayout?: boolean;
    title?: string;
    onBack?: () => void;
    onClose?: () => void;
  } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  const mobileBackStack = getMobileBackStack();
  const chatSidebarOrder = getChatSidebarOrder();
  const iconSize = $derived(mobileLayout ? 18 : 13);
  let search = $state("");
  let scrollElement = $state<HTMLElement | undefined>();
  const normalizedSearch = $derived(showSearch ? normalizeChatChannelSearch(search) : "");
  /** Every channel picker follows one eye state, so a hover subpanel lists archived channels like its parent. */
  const showArchived = $derived(chat.showArchivedChannelsInPickers && orderProjectId !== null);
  let archivedError = $state<string | null>(null);
  const rows = $derived(chatChannelPickerRows(
    channels,
    orderProjectId ? chat.archivedChannelsForProject(orderProjectId) : [],
    showArchived,
    normalizedSearch,
  ));
  const rowCount = $derived(rows.active.length + rows.archived.length);

  $effect(() => {
    const projectId = orderProjectId;
    archivedError = null;
    if (!showArchived || !projectId) return;
    let cancelled = false;
    chat.loadArchivedChannelsForPicker(projectId).catch((error: unknown) => {
      if (!cancelled) archivedError = error instanceof Error ? error.message : String(error);
    });
    return () => { cancelled = true; };
  });

  /** Active channels drag by their row in the custom order of the project, outside search; archived ones keep no place. */
  const orderDrag = createSidebarOrderDrag({
    getIds: () => rows.active.map((channel) => channel.id),
    enabled: () => !mobileLayout && orderProjectId !== null && !normalizedSearch
      && chatSidebarOrder.orderForProject(orderProjectId) === "custom",
    isStarred: (channelId) => isChannelStarred(channelId),
    save: async (orderedIds) => {
      if (orderProjectId) chatSidebarOrder.setCustomSiblingOrder(orderProjectId, orderedIds);
    },
    description: "channels from the channel picker",
  });
  $effect(() => orderDrag.reset);

  /** Route an active channel click to a new tab when the gesture asks for one, else select the channel. */
  function handleActiveChannelClick(event: MouseEvent, channel: ChatChannelRead): void {
    const intent = onChannelOpenInNewTab ? openInNewTabIntent(event) : null;
    if (intent) {
      event.preventDefault();
      onChannelOpenInNewTab?.(channel, intent);
      return;
    }
    if (event.type === "click") void onChannelSelected(channel);
  }

  function handleMobileBack(): void {
    if (normalizedSearch) {
      search = "";
      return;
    }
    onBack?.();
  }

  $effect(() => {
    void rowCount;
    requestAnimationFrame(() => onLayoutChange?.());
  });

  $effect(() => {
    if (!mobileLayout || !onBack) return;
    return mobileBackStack.activate({ handle: handleMobileBack });
  });

  $effect(() => {
    if (mobileLayout || !rootElement || !onLayoutChange) return;
    const observer = new ResizeObserver(() => onLayoutChange?.());
    observer.observe(rootElement);
    return () => observer.disconnect();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={rootElement}
  class={cn(
    "chat-channel-picker-panel surface-floating flex min-h-0 flex-col overflow-hidden",
    mobileLayout && "h-full rounded-2xl",
    className,
    zIndexClass,
  )}
  style={mobileLayout ? frameStyle : `${frameStyle}; height: auto`}
  data-chat-channel-picker
  onpointerenter={onPointerEnter}
  onpointerleave={onPointerLeave}
>
  {#if mobileLayout || showSearch}
  <div class={cn("shrink-0", mobileLayout ? "border-b border-border/70 p-2" : "px-1.5 pt-1.5")}>
    {#if mobileLayout}
      <div class="flex min-h-14 items-center gap-1">
        {#if onBack}
          <button
            type="button"
            class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
            aria-label={t("projects.navigator.backToProjects")}
            onclick={handleMobileBack}
          >
            <ChevronLeft size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
        <h2 class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{title ?? t("chat.channels.navigatorLabel")}</h2>
        {#if onClose}
          <button
            type="button"
            data-project-picker-initial-focus="true"
            class="flex min-h-12 min-w-12 items-center justify-center rounded-xl active:bg-accent"
            aria-label={t("common.close")}
            onclick={onClose}
          >
            <X size={22} strokeWidth={iconStrokeWidth} aria-hidden="true" />
          </button>
        {/if}
      </div>
    {/if}
    {#if showSearch}
    <div class={cn(
      "field flex items-center gap-1.5",
      mobileLayout ? "min-h-12 rounded-xl px-3" : "py-0",
      orderProjectId && (mobileLayout ? "pr-0" : "pr-1"),
    )}>
      <Search size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0 text-popover-foreground/60" />
      <input
        bind:value={search}
        placeholder={t("chat.channels.search")}
        aria-label={t("chat.channels.search")}
        class={cn(
          "field-bare text-popover-foreground",
          mobileLayout && "h-12 text-base",
        )}
      />
      {#if mobileLayout && normalizedSearch}
        <button
          type="button"
          class="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl text-popover-foreground/60 active:bg-accent"
          aria-label={t("chat.channels.clearSearch")}
          onclick={() => { search = ""; }}
        >
          <X size={18} strokeWidth={iconStrokeWidth} aria-hidden="true" />
        </button>
      {/if}
      {#if orderProjectId}
        {@const projectId = orderProjectId}
        <div class="flex shrink-0 items-center">
          <SidebarOrderMenu
            order={chatSidebarOrder.orderForProject(projectId)}
            override={chatSidebarOrder.orderOverride(projectId)}
            allowGlobal
            label={t("chat.channels.sort")}
            buttonClass={cn(
              "flex shrink-0 items-center justify-center text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground",
              mobileLayout ? "h-12 w-12 rounded-xl" : "h-6 w-6 rounded",
            )}
            activeClass="bg-accent text-accent-foreground"
            {iconSize}
            {iconStrokeWidth}
            menuZIndexClass="z-90"
            onSelect={(order) => chatSidebarOrder.setProjectOrder(projectId, order)}
          />
          <button
            type="button"
            class={cn(
              "flex shrink-0 items-center justify-center text-popover-foreground/60 hover:bg-accent hover:text-accent-foreground",
              mobileLayout ? "h-12 w-12 rounded-xl" : "h-6 w-6 rounded",
            )}
            aria-label={chat.showArchivedChannelsInPickers ? t("chat.channels.hideArchived") : t("chat.channels.showArchived")}
            aria-pressed={chat.showArchivedChannelsInPickers}
            title={chat.showArchivedChannelsInPickers ? t("chat.channels.hideArchived") : t("chat.channels.showArchived")}
            data-chat-channel-archived-toggle
            onclick={() => { chat.showArchivedChannelsInPickers = !chat.showArchivedChannelsInPickers; }}
          >
            {#if chat.showArchivedChannelsInPickers}
              <EyeOff size={iconSize} strokeWidth={iconStrokeWidth} />
            {:else}
              <Eye size={iconSize} strokeWidth={iconStrokeWidth} />
            {/if}
          </button>
        </div>
      {/if}
    </div>
    {/if}
  </div>
  {/if}

  <div class="relative flex min-h-0 flex-auto flex-col">
    <div bind:this={scrollElement} use:scrollEdgeFadeAction class={cn("hide-scrollbar min-h-0 flex-auto overflow-y-auto", mobileLayout ? "overscroll-contain px-2 py-2" : "surface-floating-body")}>
      {#if archivedError}
        <Notice class="px-3 py-2" message={t("chat.channels.archivedLoadFailed", archivedError)} copy={archivedError} />
      {/if}
      {#if rowCount === 0}
        <p class="px-3 py-2 text-panel-detail text-popover-foreground/60">
          {normalizedSearch ? t("chat.channels.noResults") : t("chat.channels.empty")}
        </p>
      {:else}
        <div class="grid">
          {#each rows.active as channel (channel.id)}
            <button
              type="button"
              class={cn(
                "menu-item gap-2 text-popover-foreground",
                mobileLayout && "min-h-12 rounded-xl px-3 text-sm active:bg-accent",
                selectedChannelId === channel.id && "font-medium",
              )}
              aria-current={selectedChannelId === channel.id ? "page" : undefined}
              data-chat-channel-option={channel.id}
              {@attach orderDrag.item(channel.id)}
              onpointerdown={(event) => orderDrag.pointerDown(event, channel.id)}
              onmousedown={preventMiddleClickAutoscroll}
              onclick={(event) => handleActiveChannelClick(event, channel)}
              onauxclick={(event) => handleActiveChannelClick(event, channel)}
            >
              <StarredIconBadge starred={isChannelStarred(channel.id)}><Hash size={iconSize} strokeWidth={iconStrokeWidth} /></StarredIconBadge>
              <span class="min-w-0 flex-1 truncate">{channel.name}</span>
            </button>
          {/each}
          {#each rows.archived as channel (channel.id)}
            <button
              type="button"
              class={cn(
                "menu-item gap-2 text-popover-foreground",
                mobileLayout && "min-h-12 rounded-xl px-3 text-sm active:bg-accent",
                selectedChannelId === channel.id && "font-medium",
              )}
              aria-current={selectedChannelId === channel.id ? "page" : undefined}
              data-chat-channel-option={channel.id}
              data-chat-channel-archived
              onclick={() => { void onChannelSelected(channel); }}
            >
              <Hash size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
              <span class="min-w-0 flex-1 truncate">{channel.name}</span>
              <span role="img" aria-label={t("chat.channels.archivedChannel")} title={t("chat.channels.archivedChannel")} class="inline-flex shrink-0 text-popover-foreground/60">
                <Archive size={iconSize} strokeWidth={iconStrokeWidth} aria-hidden="true" />
              </span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {#if !mobileLayout}
      <CalendarScrollbar scrollContainer={scrollElement} stickyTop={4} stickyBottom={4} wheelPassthrough />
    {/if}
  </div>

  <div class={cn("relative z-10 shrink-0 bg-popover", mobileLayout ? "p-2" : "p-1.5")}>
    <div class="pointer-events-none absolute left-1.5 right-1.5 top-0 border-t border-border/70"></div>
    <button
      type="button"
      class={cn(
        "menu-item justify-center gap-1.5 text-popover-foreground",
        mobileLayout && "min-h-12 rounded-xl text-sm active:bg-accent",
      )}
      onclick={() => { void onCreateChannel(); }}
    >
      <Plus size={iconSize} strokeWidth={iconStrokeWidth} />
      <span>{t("chat.channels.createTitle")}</span>
    </button>
  </div>
</div>

<style>
  .chat-channel-picker-panel {
    --cal-scrollbar-thumb: color-mix(in srgb, var(--popover-foreground) 18%, var(--popover));
    --cal-scrollbar-thumb-hover: color-mix(in srgb, var(--popover-foreground) 36%, var(--popover));
  }
</style>
