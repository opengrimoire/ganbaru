<script lang="ts">
  import { tick, untrack } from "svelte";
  import { flip } from "svelte/animate";
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
    workspaceTabProjectHint,
  } from "$lib/workspace-tabs/presentation";
  import {
    tabDragLeftBar,
    tabStripDropIndex,
    tabStripMoveTarget,
    type TabStripSlot,
  } from "$lib/workspace-tabs/strip-drag";
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

  interface OwnTabDrag {
    id: string;
    /** Pointer offset from the tab's left edge when the drag started. */
    grabX: number;
    width: number;
    /** Whether the tab may leave this window, which a window's last tab may not. */
    canLeave: boolean;
    /** Neighbor that puts the tab back where it started when the drag is cancelled. */
    origin: { id: string; placement: WorkspaceTabPlacement } | null;
    /** A drop in this window handled the drag. */
    dropped: boolean;
  }

  const TAB_REORDER_MIME = "application/x-ganbaru-ai-workspace-tab";
  /** How long tabs take to slide into their new slots while one is dragged across them. */
  const TAB_SLIDE_MS = 150;
  const ICON_SIZE = 14;
  const ICON_STROKE_WIDTH = 1.5;

  const tabs = getWorkspaceTabs();
  const preferences = getPreferences();
  const { t } = getLocalization();
  const reducedMotion = typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

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
  /** One of this window's tabs while it is dragged. */
  let drag: OwnTabDrag | null = null;
  let draggingId = $state<string | null>(null);
  /** The dragged tab has been pulled off the title bar and leaves the window when dropped. */
  let leavingBar = $state(false);
  /** Where another window's tab dragged over this strip would land. */
  let dropTarget = $state<{ id: string; placement: WorkspaceTabPlacement } | null>(null);
  /** Document drag entries minus exits while a tab is dragged, which reaches zero once the drag leaves the window. */
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
    void draggingId;
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

  /** Copy of the tab, without its close button, that the platform shows under the pointer while it is dragged. */
  function setTabDragImage(transfer: DataTransfer, element: HTMLElement, grabX: number, grabY: number): void {
    const preview = element.cloneNode(true);
    if (!(preview instanceof HTMLElement)) return;
    preview.querySelector(".titlebar-tab-close")?.remove();
    preview.classList.add("titlebar-tab-preview");
    preview.setAttribute("aria-hidden", "true");
    preview.inert = true;
    preview.style.width = `${element.offsetWidth}px`;
    document.body.append(preview);
    transfer.setDragImage(preview, grabX, grabY);
    requestAnimationFrame(() => preview.remove());
  }

  /** Neighbor that puts a tab back where it was, or null for a tab alone in its window. */
  function originOf(id: string): { id: string; placement: WorkspaceTabPlacement } | null {
    const index = tabs.tabs.findIndex((tab) => tab.id === id);
    const next = tabs.tabs[index + 1];
    const previous = tabs.tabs[index - 1];
    if (next) return { id: next.id, placement: "before" };
    return previous ? { id: previous.id, placement: "after" } : null;
  }

  function handleDragStart(event: DragEvent, tab: WorkspaceTab): void {
    const transfer = event.dataTransfer;
    const element = event.currentTarget;
    if (!transfer || !(element instanceof HTMLElement)) return;
    const rect = element.getBoundingClientRect();
    const grabX = event.clientX - rect.left;
    transfer.effectAllowed = "move";
    transfer.setData(TAB_REORDER_MIME, tab.id);
    const payload = tabs.dragPayload(tab.id);
    if (payload) transfer.setData(TAB_WINDOW_DRAG_MIME, tabWindowDragData(payload));
    setTabDragImage(transfer, element, grabX, event.clientY - rect.top);
    drag = { id: tab.id, grabX, width: rect.width, canLeave: payload !== null, origin: originOf(tab.id), dropped: false };
    draggingId = tab.id;
    leavingBar = false;
    dropTarget = null;
    dragDepth = 0;
  }

  /** A drag of one of this window's tabs, or of another window's tab that may move here. */
  function dragKind(event: DragEvent): "reorder" | "window" | null {
    const types = Array.from(event.dataTransfer?.types ?? []);
    if (drag) return types.includes(TAB_REORDER_MIME) ? "reorder" : null;
    return types.includes(TAB_WINDOW_DRAG_MIME) ? "window" : null;
  }

  function acceptDrag(event: DragEvent): void {
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  /** Tabs the strip shows, by their layout position so slide animations in flight do not move the slots. */
  function shownSlots(): TabStripSlot[] {
    return tabs.tabs.flatMap((tab) => {
      const element = tabElements[tab.id];
      return element && element.offsetParent !== null
        ? [{ id: tab.id, left: element.offsetLeft, width: element.offsetWidth }]
        : [];
    });
  }

  /** Horizontal pointer position in the strip's layout coordinates. */
  function stripX(clientX: number): number {
    return strip ? clientX - strip.getBoundingClientRect().left + strip.scrollLeft : clientX;
  }

  /** Whether the pointer has been pulled far enough above or below the title bar to take a tab off it. */
  function pulledOffBar(clientY: number): boolean {
    const bar = strip?.closest<HTMLElement>(".app-title-bar");
    if (!strip || !bar) return false;
    return tabDragLeftBar(clientY, bar.getBoundingClientRect(), strip.getBoundingClientRect().height / 2);
  }

  /** Slide the dragged tab into the slot under it while it stays on the bar, and lift it off once pulled away. */
  function dragOwnTab(event: DragEvent, current: OwnTabDrag): void {
    acceptDrag(event);
    const leaving = current.canLeave && pulledOffBar(event.clientY);
    if (leaving !== leavingBar) leavingBar = leaving;
    if (leaving) return;
    const slots = shownSlots();
    const center = stripX(event.clientX) - current.grabX + current.width / 2;
    const target = tabStripMoveTarget(slots.map((slot) => slot.id), current.id, tabStripDropIndex(slots, center));
    if (target) tabs.move(current.id, target.id, target.placement);
  }

  /** Mark where another window's tab would land, before or after the tab under the pointer. */
  function dragForeignTab(event: DragEvent): void {
    if (pulledOffBar(event.clientY)) {
      dropTarget = null;
      return;
    }
    acceptDrag(event);
    const slots = shownSlots();
    const x = stripX(event.clientX);
    const slot = slots[tabStripDropIndex(slots, x)];
    dropTarget = slot ? { id: slot.id, placement: x < slot.left + slot.width / 2 ? "before" : "after" } : null;
  }

  /**
   * Tab drags are handled for the whole window before any other drop target sees them, so a tab can be dropped
   * anywhere on the title bar and pulled off it over content that accepts other drags.
   */
  function handleWindowDragOver(event: DragEvent): void {
    const kind = dragKind(event);
    if (kind === "reorder" && drag) dragOwnTab(event, drag);
    if (kind === "window") dragForeignTab(event);
  }

  function handleWindowDrop(event: DragEvent): void {
    const kind = dragKind(event);
    if (!kind) return;
    acceptDrag(event);
    const current = drag;
    if (kind === "window") {
      tabs.receiveDrop(event.dataTransfer?.getData(TAB_WINDOW_DRAG_MIME) ?? "", dropTarget);
      endDrag();
      return;
    }
    if (!current) return;
    current.dropped = true;
    if (leavingBar) tearOff(current.id, event.screenX, event.screenY);
  }

  function tearOff(id: string, screenX: number, screenY: number): void {
    void tabs.tearOff(id, tearOffWindowPosition(screenX, screenY)).catch((error: unknown) => {
      console.error("Failed to move tab to a new window:", error);
    });
  }

  /**
   * End a drag of this window's tab. A drag no window took opens the tab in a new window when it ended outside
   * this one, and otherwise, as when cancelled, puts the tab back where it started.
   */
  function handleDragEnd(event: DragEvent): void {
    const current = drag;
    const leftWindow = dragEndedOutsideWindow(event, { width: window.innerWidth, height: window.innerHeight }, dragDepth);
    endDrag();
    if (!current || current.dropped || event.dataTransfer?.dropEffect !== "none") return;
    if (leftWindow && current.canLeave) {
      tearOff(current.id, event.screenX, event.screenY);
      return;
    }
    const origin = current.origin;
    if (origin && tabs.tabs.some((tab) => tab.id === origin.id)) tabs.move(current.id, origin.id, origin.placement);
  }

  /** Count tab drag entries and exits across the document, to tell when a drag left the window. */
  function trackDragDepth(event: DragEvent, change: 1 | -1): void {
    if (!dragKind(event)) return;
    dragDepth += change;
    if (dragDepth <= 0 && !drag) dropTarget = null;
  }

  function endDrag(): void {
    drag = null;
    draggingId = null;
    leavingBar = false;
    dropTarget = null;
    dragDepth = 0;
  }

  $effect(() => {
    const options = { capture: true };
    window.addEventListener("dragover", handleWindowDragOver, options);
    window.addEventListener("drop", handleWindowDrop, options);
    return () => {
      window.removeEventListener("dragover", handleWindowDragOver, options);
      window.removeEventListener("drop", handleWindowDrop, options);
    };
  });

  $effect(() => {
    // A tab another window took may disappear before its drag ends.
    if (draggingId !== null && !tabs.tabs.some((tab) => tab.id === draggingId)) untrack(endDrag);
  });
</script>

<svelte:document
  ondragenter={(event) => trackDragDepth(event, 1)}
  ondragleave={(event) => trackDragDepth(event, -1)}
/>

<div class="flex min-w-0 shrink items-center gap-0.5 pl-1.5">
  <div
    bind:this={strip}
    class="relative flex min-w-0 items-center gap-0.5 overflow-hidden"
    role="tablist"
    aria-orientation="horizontal"
  >
    {#if indicatorStyle && draggingId === null}
      <div
        class="tab-indicator absolute top-0 h-full rounded-md bg-card shadow-(--tab-active-shadow)"
        style={indicatorStyle}
      ></div>
    {/if}
    {#each tabs.tabs as tab, index (tab.id)}
      {@const Icon = workspaceTabKindIcon(tab.location.kind)}
      {@const active = tab.id === tabs.activeTabId}
      {@const label = workspaceTabLabel(tab, t)}
      {@const project = workspaceTabProjectHint(tab)}
      {@const fullLabel = workspaceTabFullLabel(tab, t)}
      <div
        bind:this={tabElements[tab.id]}
        animate:flip={{ duration: draggingId === null || reducedMotion ? 0 : TAB_SLIDE_MS }}
        data-active={active ? "" : undefined}
        class={cn(
          "titlebar-tab group relative z-1 flex items-center rounded-md text-sm font-medium transition-colors",
          compact && "titlebar-tab-compact",
          activeOnly && !active ? "hidden" : "",
          active
            ? "text-foreground dark:text-white"
            : "text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
          active && draggingId !== null && "bg-card shadow-(--tab-active-shadow)",
          draggingId === tab.id && "titlebar-tab-dragging",
          draggingId === tab.id && leavingBar && "titlebar-tab-leaving",
          dropTarget?.id === tab.id && (dropTarget.placement === "before" ? "drop-before" : "drop-after"),
        )}
        draggable="true"
        role="presentation"
        ondragstart={(event) => handleDragStart(event, tab)}
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
            <span class="titlebar-tab-label min-w-0 flex-1 truncate text-start">
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

  /* The dragged tab stays behind as a faint slot that shows where it lands. */
  .titlebar-tab.titlebar-tab-dragging {
    opacity: 0.45;
    transition: width 150ms ease, min-width 150ms ease, margin 150ms ease, opacity 150ms ease;
  }

  .titlebar-tab-dragging .titlebar-tab-close {
    visibility: hidden;
  }

  /* Pulled off the bar, the tab gives its slot back and the strip closes around it. */
  .titlebar-tab.titlebar-tab-leaving {
    width: 0;
    min-width: 0;
    margin-right: -0.125rem;
    overflow: hidden;
    opacity: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .titlebar-tab.titlebar-tab-dragging {
      transition: none;
    }
  }

  /* The image the platform drags: the tab as it looks when active, built off screen for a moment. */
  :global(.titlebar-tab.titlebar-tab-preview) {
    position: fixed;
    top: 0;
    left: -10000px;
    border: 1px solid var(--color-border);
    background-color: var(--color-card);
    color: var(--color-foreground);
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
