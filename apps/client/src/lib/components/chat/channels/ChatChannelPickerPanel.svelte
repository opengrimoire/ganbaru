<script lang="ts">
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Hash from "@lucide/svelte/icons/hash";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import CalendarScrollbar from "$lib/components/calendar/CalendarScrollbar.svelte";
  import type { ChatChannelRead } from "$lib/chat/contracts";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";


  let {
    rootElement = $bindable<HTMLDivElement | undefined>(),
    channels,
    selectedChannelId = null,
    frameStyle,
    className = "relative",
    zIndexClass = "",
    iconStrokeWidth = 1.6,
    onChannelSelected,
    onCreateChannel,
    onLayoutChange = undefined,
    onPointerLeave = undefined,
    showSearch = true,
    mobileLayout = false,
    title = undefined,
    onBack = undefined,
    onClose = undefined,
  }: {
    rootElement?: HTMLDivElement;
    channels: readonly ChatChannelRead[];
    selectedChannelId?: string | null;
    frameStyle: string;
    className?: string;
    zIndexClass?: string;
    iconStrokeWidth?: number;
    onChannelSelected: (channel: ChatChannelRead) => MaybePromise<void>;
    onCreateChannel: () => MaybePromise<void>;
    onLayoutChange?: () => void;
    onPointerLeave?: (event: PointerEvent) => void;
    showSearch?: boolean;
    mobileLayout?: boolean;
    title?: string;
    onBack?: () => void;
    onClose?: () => void;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  const iconSize = $derived(mobileLayout ? 18 : 13);
  let search = $state("");
  let scrollElement = $state<HTMLElement | undefined>();
  const normalizedSearch = $derived(showSearch ? search.trim().toLocaleLowerCase() : "");
  const visibleChannels = $derived(channels.filter((channel) => (
    !normalizedSearch
    || channel.name.toLocaleLowerCase().includes(normalizedSearch)
    || channel.topic.toLocaleLowerCase().includes(normalizedSearch)
  )));

  function handleMobileBack(): void {
    if (normalizedSearch) {
      search = "";
      return;
    }
    onBack?.();
  }

  $effect(() => {
    void visibleChannels.length;
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
    </div>
    {/if}
  </div>
  {/if}

  <div class="relative flex min-h-0 flex-auto flex-col">
    <div bind:this={scrollElement} use:scrollEdgeFadeAction class={cn("hide-scrollbar min-h-0 flex-auto overflow-y-auto", mobileLayout ? "overscroll-contain px-2 py-2" : "surface-floating-body")}>
      {#if visibleChannels.length === 0}
        <p class="px-3 py-2 text-panel-detail text-popover-foreground/60">
          {normalizedSearch ? t("chat.channels.noResults") : t("chat.channels.empty")}
        </p>
      {:else}
        <div class="grid">
          {#each visibleChannels as channel (channel.id)}
            <button
              type="button"
              class={cn(
                "menu-item gap-2 text-popover-foreground",
                mobileLayout && "min-h-12 rounded-xl px-3 text-sm active:bg-accent",
                selectedChannelId === channel.id && "font-medium",
              )}
              aria-current={selectedChannelId === channel.id ? "page" : undefined}
              data-chat-channel-option={channel.id}
              onclick={() => { void onChannelSelected(channel); }}
            >
              <Hash size={iconSize} strokeWidth={iconStrokeWidth} class="shrink-0" />
              <span class="min-w-0 flex-1 truncate">{channel.name}</span>
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
