<script lang="ts">
  import { tick, untrack } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import type { WorkspaceTab, WorkspaceTabPlacement } from "$lib/workspace-tabs/model";
  import {
    shortWorkspaceTabProject,
    workspaceTabFullLabel,
    workspaceTabKindIcon,
    workspaceTabLabel,
    workspaceTabPositionShortcut,
    workspaceTabProject,
  } from "$lib/workspace-tabs/presentation";
  import TitleBarTabLauncher from "./TitleBarTabLauncher.svelte";
  import {
    TAB_WINDOW_DRAG_MIME,
    dragEndedOutsideWindow,
    tabWindowDragData,
    tearOffWindowPosition,
  } from "$lib/windows/tab-windows";

  let {
    pomodoroMenuOpen,
    onVolumeWheel,
    onOpenContextMenu,
  }: {
    pomodoroMenuOpen: boolean;
    onVolumeWheel: (event: WheelEvent) => void;
    onOpenContextMenu: (event: MouseEvent, tabId: string) => void;
  } = $props();

  const TAB_REORDER_MIME = "application/x-ganbaru-ai-workspace-tab";
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;

  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const { t } = getLocalization();

  /** Icon-only tabs, chosen in the title bar settings. */
  const compact = $derived(preferences.titleBarVisibility.compactTabs);
  /** Tabs shrink like browser tabs; only when they no longer fit at their narrowest does the strip keep just the active one. */
  let activeOnly = $state(false);
  const canOpenTabs = $derived(tabs.canOpenTabs);

  let strip: HTMLDivElement | undefined = $state();
  let addButton: HTMLButtonElement | undefined = $state();
  let tabElements: Record<string, HTMLElement> = $state({});
  let indicatorStyle = $state("");
  let indicatorFrame = 0;
  let wheelCooldown = false;
  let launcherOpen = $state(false);
  let draggingId = $state<string | null>(null);
  let dropTarget = $state<{ id: string; placement: WorkspaceTabPlacement } | null>(null);
  /** Document drag entries minus exits while one of this window's tabs is dragged. */
  let dragDepth = 0;
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

  /** Show every tab unless they overflow at their minimum width, measuring before the browser paints. */
  async function measureDensity(): Promise<void> {
    const sequence = ++measureSequence;
    activeOnly = false;
    await tick();
    if (sequence !== measureSequence) return;
    if (stripOverflows()) {
      activeOnly = true;
      await tick();
    }
    if (sequence === measureSequence) scheduleIndicatorUpdate();
  }

  $effect(() => {
    void tabs.tabs.length;
    void compact;
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
    void activeOnly;
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
    const name = workspaceTabFullLabel(tab, t);
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
    draggingId = tab.id;
    dragDepth = 0;
    transfer.effectAllowed = "move";
    transfer.setData(TAB_REORDER_MIME, tab.id);
    const drag = tabs.dragPayload(tab.id);
    if (drag) transfer.setData(TAB_WINDOW_DRAG_MIME, tabWindowDragData(drag));
  }

  /** A drag of one of this window's tabs, or of another window's tab that may move here. */
  function dragKind(event: DragEvent): "reorder" | "window" | null {
    const types = Array.from(event.dataTransfer?.types ?? []);
    if (draggingId !== null) return types.includes(TAB_REORDER_MIME) ? "reorder" : null;
    return types.includes(TAB_WINDOW_DRAG_MIME) ? "window" : null;
  }

  function acceptDrag(event: DragEvent): boolean {
    if (!dragKind(event)) return false;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    return true;
  }

  function handleDragOver(event: DragEvent, tab: WorkspaceTab): void {
    if (!acceptDrag(event)) return;
    if (tab.id === draggingId) {
      dropTarget = null;
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const placement: WorkspaceTabPlacement = event.clientX < rect.left + rect.width / 2 ? "before" : "after";
    dropTarget = { id: tab.id, placement };
  }

  /** A tab from another window can also land on the strip around the tabs, at the end. */
  function handleStripDragOver(event: DragEvent): void {
    if (dragKind(event) !== "window" || !acceptDrag(event)) return;
    dropTarget = null;
  }

  function handleStripDragLeave(event: DragEvent): void {
    const next = event.relatedTarget;
    if (draggingId === null && !(next instanceof Node && event.currentTarget instanceof Node && event.currentTarget.contains(next))) {
      dropTarget = null;
    }
  }

  function handleDrop(event: DragEvent): void {
    const kind = dragKind(event);
    if (!kind) return;
    event.preventDefault();
    event.stopPropagation();
    if (kind === "reorder" && draggingId && dropTarget) tabs.move(draggingId, dropTarget.id, dropTarget.placement);
    if (kind === "window") tabs.receiveDrop(event.dataTransfer?.getData(TAB_WINDOW_DRAG_MIME) ?? "", dropTarget);
    endDrag();
  }

  /** A drag that no window took and that ended outside this one opens the tab in a new window there. */
  function handleDragEnd(event: DragEvent): void {
    const id = draggingId;
    const leftWindow = dragEndedOutsideWindow(event, { width: window.innerWidth, height: window.innerHeight }, dragDepth);
    endDrag();
    if (!id || !leftWindow || event.dataTransfer?.dropEffect !== "none") return;
    void tabs.tearOff(id, tearOffWindowPosition(event.screenX, event.screenY)).catch((error: unknown) => {
      console.error("Failed to move tab to a new window:", error);
    });
  }

  /** Count drag entries and exits across the document while dragging a tab, to tell when it left the window. */
  function trackDragDepth(change: 1 | -1): void {
    if (draggingId !== null) dragDepth += change;
  }

  function endDrag(): void {
    draggingId = null;
    dropTarget = null;
    dragDepth = 0;
  }

  $effect(() => {
    // A tab another window took may disappear before its drag ends.
    if (draggingId !== null && !tabs.tabs.some((tab) => tab.id === draggingId)) untrack(endDrag);
  });
</script>

<svelte:document ondragenter={() => trackDragDepth(1)} ondragleave={() => trackDragDepth(-1)} />

<div
  class="flex min-w-0 shrink items-center gap-0.5 pl-1.5"
  role="presentation"
  ondragover={handleStripDragOver}
  ondragleave={handleStripDragLeave}
  ondrop={handleDrop}
>
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
      {@const project = workspaceTabProject(tab)}
      {@const fullLabel = workspaceTabFullLabel(tab, t)}
      <div
        bind:this={tabElements[tab.id]}
        data-active={active ? "" : undefined}
        class={cn(
          "titlebar-tab group relative z-1 flex items-center rounded-md text-sm font-medium transition-colors",
          compact && "titlebar-tab-compact",
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
        ondragend={handleDragEnd}
        oncontextmenu={(event) => onOpenContextMenu(event, tab.id)}
        onmousedown={handleMouseDown}
        onauxclick={(event) => handleAuxClick(event, tab)}
      >
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-label={tab.locked ? t("titleBar.tab.lockedTab", fullLabel) : fullLabel}
          class={cn(
            "titlebar-tab-button flex h-full min-w-0 flex-1 items-center rounded-md",
            compact ? "justify-center" : "gap-1.5 pl-3 pr-1",
          )}
          title={tabTitle(tab, index)}
          onclick={() => tabs.activate(tab.id)}
        >
          <Icon class="shrink-0" size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
          {#if !compact}
            <span class="titlebar-tab-label min-w-0 flex-1 truncate">
              {label}{#if project}<span class="opacity-60"> {t("titleBar.tab.projectHint", shortWorkspaceTabProject(project))}</span>{/if}
            </span>
          {/if}
          {#if !compact && tab.locked}<Lock class="titlebar-tab-lock shrink-0 opacity-60" size={11} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />{/if}
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
  /*
   * Browser-like sizing: every tab starts at the same width and all of them shrink evenly when the bar runs out
   * of room. Each tab is a size container, so its own width decides what it can still show.
   */
  .titlebar-tab {
    container-type: inline-size;
    flex: 0 1 auto;
    width: 12.5rem;
    min-width: 2.25rem;
    height: 28px;
  }

  .titlebar-tab.titlebar-tab-compact {
    width: 28px;
    min-width: 28px;
  }

  @container (width < 6rem) {
    .titlebar-tab:not([data-active]) .titlebar-tab-close {
      display: none;
    }
  }

  @container (width < 4.5rem) {
    .titlebar-tab-label,
    .titlebar-tab :global(.titlebar-tab-lock) {
      display: none;
    }

    .titlebar-tab-button {
      justify-content: center;
      padding-left: 0;
    }
  }

  @container (width < 3.5rem) {
    .titlebar-tab-close {
      display: none;
    }

    .titlebar-tab-button {
      padding-right: 0;
    }
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
