<script lang="ts">
  import { tick, untrack } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import type { DetachableTabView } from "$lib/navigation";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import {
    DETACHED_VIEW_DRAG_MIME,
    serializeDetachedViewDragPayload,
  } from "$lib/windows/detached";
  import type { WorkspaceTab, WorkspaceTabPlacement } from "$lib/workspace-tabs/model";
  import {
    workspaceTabKindIcon,
    workspaceTabLabel,
    workspaceTabPositionShortcut,
  } from "$lib/workspace-tabs/presentation";
  import TitleBarTabLauncher from "./TitleBarTabLauncher.svelte";

  let {
    detachedWindowView,
    windowLabel,
    pomodoroMenuOpen,
    onVolumeWheel,
    onOpenContextMenu,
  }: {
    detachedWindowView: DetachableTabView | undefined;
    windowLabel: string;
    pomodoroMenuOpen: boolean;
    onVolumeWheel: (event: WheelEvent) => void;
    onOpenContextMenu: (event: MouseEvent, tabId: string) => void;
  } = $props();

  /** Overflow steps: full tabs, icon-only tabs, then only the active tab with the rest in the launcher. */
  type StripDensity = 0 | 1 | 2;

  const TAB_REORDER_MIME = "application/x-ganbaru-ai-workspace-tab";
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;

  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const viewport = getViewport();
  const { t } = getLocalization();

  const baseDensity = $derived<StripDensity>(
    viewport.below("narrow")
      ? 2
      : preferences.titleBarVisibility.compactTabs || viewport.below("regular") ? 1 : 0,
  );
  let measuredDensity = $state<StripDensity>(0);
  const density = $derived<StripDensity>(Math.max(baseDensity, measuredDensity) as StripDensity);
  const compact = $derived(density >= 1);
  const activeOnly = $derived(density === 2);
  const canOpenTabs = $derived(!detachedWindowView);

  let strip: HTMLDivElement | undefined = $state();
  let addButton: HTMLButtonElement | undefined = $state();
  let tabElements: Record<string, HTMLElement> = $state({});
  let indicatorStyle = $state("");
  let indicatorFrame = 0;
  let wheelCooldown = false;
  let launcherOpen = $state(false);
  let draggingId = $state<string | null>(null);
  let dropTarget = $state<{ id: string; placement: WorkspaceTabPlacement } | null>(null);
  let measureSequence = 0;

  /**
   * Whether the tabs are wider than the strip. Only tabs count: the active indicator is absolutely positioned and
   * can still sit where a closed tab was until the next frame, which would read as overflow on a short strip.
   */
  function stripOverflows(): boolean {
    if (!strip) return false;
    let contentRight = 0;
    for (const element of strip.querySelectorAll<HTMLElement>(".titlebar-tab")) {
      contentRight = Math.max(contentRight, element.offsetLeft + element.offsetWidth);
    }
    return contentRight > strip.clientWidth + 1;
  }

  /** Find the lightest density at which every tab fits, measuring each step before the browser paints. */
  async function measureDensity(): Promise<void> {
    const sequence = ++measureSequence;
    measuredDensity = 0;
    await tick();
    while (sequence === measureSequence && measuredDensity < 2 && stripOverflows()) {
      measuredDensity = (measuredDensity + 1) as StripDensity;
      await tick();
    }
    if (sequence === measureSequence) scheduleIndicatorUpdate();
  }

  $effect(() => {
    void tabs.tabs.map((tab) => workspaceTabLabel(tab, t)).join("\n");
    void baseDensity;
    void preferences.fontFamilyId;
    void preferences.fontScale;
    untrack(() => { void measureDensity(); });
  });

  $effect(() => {
    // The title bar width depends only on the window, so density changes cannot feed back into this observer.
    const element = strip?.closest<HTMLElement>(".app-title-bar");
    if (!element || typeof ResizeObserver === "undefined") return;
    let lastWidth = element.getBoundingClientRect().width;
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? lastWidth;
      if (Math.abs(width - lastWidth) < 1) return;
      lastWidth = width;
      void measureDensity();
    });
    observer.observe(element);
    return () => observer.disconnect();
  });

  function updateIndicator(): void {
    const element = tabElements[tabs.activeTabId];
    const parent = element?.parentElement;
    if (!element || !parent || element.offsetParent === null) {
      indicatorStyle = "";
      return;
    }
    const parentRect = parent.getBoundingClientRect();
    const elementRect = element.getBoundingClientRect();
    indicatorStyle = `left: ${elementRect.left - parentRect.left + parent.scrollLeft}px; width: ${elementRect.width}px;`;
  }

  function scheduleIndicatorUpdate(): void {
    if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
    indicatorFrame = requestAnimationFrame(() => {
      indicatorFrame = 0;
      updateIndicator();
    });
  }

  $effect(() => {
    void tabs.activeTabId;
    void tabs.tabs;
    void density;
    scheduleIndicatorUpdate();
    return () => {
      if (indicatorFrame) cancelAnimationFrame(indicatorFrame);
      indicatorFrame = 0;
    };
  });

  $effect(() => {
    if (tabs.launcherRequests === 0) return;
    untrack(() => {
      if (canOpenTabs) launcherOpen = true;
    });
  });

  export function handleWheel(event: WheelEvent): void {
    if (pomodoroMenuOpen) {
      onVolumeWheel(event);
      return;
    }
    event.preventDefault();
    if (wheelCooldown || Math.abs(event.deltaY) < 5) return;
    wheelCooldown = true;
    tabs.activateRelative(event.deltaY > 0 ? 1 : -1);
    setTimeout(() => { wheelCooldown = false; }, 300);
  }

  function tabTitle(tab: WorkspaceTab, index: number): string {
    const name = workspaceTabLabel(tab, t);
    const label = tab.locked ? t("titleBar.tab.lockedTab", name) : name;
    const shortcut = workspaceTabPositionShortcut(index, tabs.tabs.length);
    return shortcut ? t("titleBar.tab.withShortcut", label, shortcut) : label;
  }

  function handleAuxClick(event: MouseEvent, tab: WorkspaceTab): void {
    if (event.button !== 1) return;
    event.preventDefault();
    tabs.close(tab.id);
  }

  function handleMouseDown(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
  }

  function closeFromButton(event: MouseEvent, tab: WorkspaceTab): void {
    event.stopPropagation();
    tabs.close(tab.id);
  }

  function handleDragStart(event: DragEvent, tab: WorkspaceTab): void {
    const transfer = event.dataTransfer;
    if (!transfer) return;
    if (detachedWindowView) {
      const payload = serializeDetachedViewDragPayload(detachedWindowView, windowLabel);
      transfer.effectAllowed = "move";
      transfer.setData(DETACHED_VIEW_DRAG_MIME, payload);
      transfer.setData("text/plain", payload);
      return;
    }
    draggingId = tab.id;
    transfer.effectAllowed = "move";
    transfer.setData(TAB_REORDER_MIME, tab.id);
  }

  function isReorderDrag(event: DragEvent): boolean {
    return draggingId !== null && Array.from(event.dataTransfer?.types ?? []).includes(TAB_REORDER_MIME);
  }

  function handleDragOver(event: DragEvent, tab: WorkspaceTab): void {
    if (!isReorderDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    if (tab.id === draggingId) {
      dropTarget = null;
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const placement: WorkspaceTabPlacement = event.clientX < rect.left + rect.width / 2 ? "before" : "after";
    dropTarget = { id: tab.id, placement };
  }

  function handleDrop(event: DragEvent): void {
    if (!isReorderDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (draggingId && dropTarget) tabs.move(draggingId, dropTarget.id, dropTarget.placement);
    endDrag();
  }

  function endDrag(): void {
    draggingId = null;
    dropTarget = null;
  }
</script>

<div class="flex min-w-0 shrink items-center gap-0.5 pl-1.5">
  <div
    bind:this={strip}
    class="relative flex min-w-0 items-center gap-0.5 overflow-hidden"
    role="tablist"
    aria-orientation="horizontal"
  >
    {#if indicatorStyle}
      <div
        class="tab-indicator absolute top-0 h-full rounded-md bg-card shadow-(--tab-active-shadow)"
        style={indicatorStyle}
      ></div>
    {/if}
    {#each tabs.tabs as tab, index (tab.id)}
      {@const Icon = workspaceTabKindIcon(tab.location.kind)}
      {@const active = tab.id === tabs.activeTabId}
      {@const label = workspaceTabLabel(tab, t)}
      <div
        bind:this={tabElements[tab.id]}
        class={cn(
          "titlebar-tab group relative z-1 flex shrink-0 items-center rounded-md text-sm font-medium transition-colors",
          activeOnly && !active ? "hidden" : "",
          active
            ? "text-foreground dark:text-white"
            : "text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
          draggingId === tab.id && "opacity-60",
          dropTarget?.id === tab.id && (dropTarget.placement === "before" ? "drop-before" : "drop-after"),
        )}
        draggable="true"
        role="presentation"
        ondragstart={(event) => handleDragStart(event, tab)}
        ondragover={(event) => handleDragOver(event, tab)}
        ondrop={handleDrop}
        ondragend={endDrag}
        oncontextmenu={(event) => onOpenContextMenu(event, tab.id)}
        onmousedown={handleMouseDown}
        onauxclick={(event) => handleAuxClick(event, tab)}
      >
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-label={tab.locked ? t("titleBar.tab.lockedTab", label) : label}
          class={cn(
            "titlebar-tab-button flex h-full min-w-0 items-center rounded-md",
            compact ? "titlebar-tab-compact justify-center" : "max-w-48 gap-1.5 pl-3 pr-1",
          )}
          title={tabTitle(tab, index)}
          onclick={() => tabs.activate(tab.id)}
        >
          <Icon class="shrink-0" size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
          {#if !compact}<span class="min-w-0 truncate">{label}</span>{/if}
          {#if !compact && tab.locked}<Lock class="shrink-0 opacity-60" size={11} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />{/if}
        </button>
        {#if !compact}
          <button
            type="button"
            tabindex="-1"
            class={cn(
              "titlebar-tab-close mr-1 flex shrink-0 items-center justify-center rounded-sm transition-opacity hover:bg-sidebar-accent",
              active ? "opacity-70 hover:opacity-100" : "opacity-0 group-hover:opacity-70 hover:opacity-100",
            )}
            aria-label={t("titleBar.tab.closeTab")}
            title={t("titleBar.tab.withShortcut", t("titleBar.tab.closeTab"), formatShortcut("Mod + W"))}
            onclick={(event) => closeFromButton(event, tab)}
          >
            <X size={12} strokeWidth={1.75} />
          </button>
        {/if}
      </div>
    {/each}
  </div>

  {#if canOpenTabs}
    <button
      bind:this={addButton}
      type="button"
      class={cn(
        "titlebar-add flex shrink-0 items-center justify-center rounded-md text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        launcherOpen && "bg-sidebar-accent text-sidebar-accent-foreground",
      )}
      aria-label={t("titleBar.tab.launcher")}
      aria-haspopup="menu"
      aria-expanded={launcherOpen}
      title={t("titleBar.tab.withShortcut", t("titleBar.tab.newTab"), formatShortcut("Mod + T"))}
      onclick={() => { launcherOpen = !launcherOpen; }}
    >
      <Plus size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
    </button>
  {/if}
</div>

{#if launcherOpen && canOpenTabs}
  <TitleBarTabLauncher
    anchor={addButton}
    showOpenTabs={activeOnly}
    onClose={() => { launcherOpen = false; }}
  />
{/if}

<style>
  .titlebar-tab {
    height: 28px;
  }

  .titlebar-tab-compact {
    width: 28px;
  }

  .titlebar-tab-close {
    width: 18px;
    height: 18px;
  }

  .titlebar-add {
    width: 28px;
    height: 28px;
  }

  .tab-indicator {
    transition: none;
  }

  .drop-before {
    box-shadow: inset 2px 0 0 var(--ring);
  }

  .drop-after {
    box-shadow: inset -2px 0 0 var(--ring);
  }

  :global(html[data-focus-intent="keyboard"]) .titlebar-tab-button:focus {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }
</style>
