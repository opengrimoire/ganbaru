<script lang="ts" module>
  /**
   * Transparent image the platform drags under the pointer. It is created once, ahead of any drag, because a
   * drag image must already be loaded when the drag starts.
   */
  const EMPTY_DRAG_IMAGE = (() => {
    const image = new Image(1, 1);
    image.src = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
    return image;
  })();
</script>

<script lang="ts">
  import { flushSync, tick, untrack } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatShortcut } from "$lib/keyboard-shortcuts";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { TAB_ARRIVAL_TIMEOUT_MS, getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
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
    tabDragPassedTrack,
    tabStripDropIndex,
    tabStripInsertIndex,
    tabStripInsertPosition,
    tabStripInsertTarget,
    tabStripMoveTarget,
    tabStripRight,
    tabStripVisualLeft,
    type TabStripSlot,
  } from "$lib/workspace-tabs/strip-drag";
  import TitleBarTabLauncher from "./TitleBarTabLauncher.svelte";
  import {
    TAB_WINDOW_DRAG_MIME,
    dragEndedOutsideWindow,
    tabWindowDragData,
    tearOffWindowPosition,
    type TabDragGrab,
    type TabWindowDrag,
    type TabWindowDragStarted,
  } from "$lib/windows/tab-windows";

  let {
    pomodoroMenuOpen,
    onVolumeWheel,
    onOpenContextMenu,
    trackEnd,
  }: {
    pomodoroMenuOpen: boolean;
    onVolumeWheel: (event: WheelEvent) => void;
    onOpenContextMenu: (event: MouseEvent, tabId: string) => void;
    /** Empty title bar space after the strip; a tab dragged past its end leaves the bar. */
    trackEnd: HTMLElement | undefined;
  } = $props();

  interface OwnTabDrag extends TabDragGrab {
    id: string;
    /** Whether the tab may open in a new window of its own, which a window's last tab may not. */
    canTearOff: boolean;
    /** Neighbor that puts the tab back where it started when the drag is cancelled. */
    origin: { id: string; placement: WorkspaceTabPlacement } | null;
  }

  const TAB_REORDER_MIME = "application/x-ganbaru-ai-workspace-tab";
  /** How long tabs take to slide into their new slots while one is dragged across them, and to settle when dropped. */
  const TAB_SLIDE_MS = 220;
  /** Ease-out curve of those slides, matching the cubic ease-out of the strip's reorder animation. */
  const TAB_EASE = "cubic-bezier(0.33, 1, 0.68, 1)";
  /** Marks the animations that slide tabs and the new tab button into place, apart from the page's other animations. */
  const SLIDE_ANIMATION_ID = "titlebar-tab-slide";
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
  let drag = $state.raw<OwnTabDrag | null>(null);
  let draggingId = $state<string | null>(null);
  /** The dragged tab has been pulled off the title bar and floats under the pointer, leaving the window when dropped. */
  let offBar = $state(false);
  /** The drag has left this window, so nothing here draws the tab. */
  let outsideWindow = $state(false);
  /** Pointer position over this window while a tab is dragged. */
  let pointer = $state<{ x: number; y: number } | null>(null);
  /** How far the dragged tab is drawn from its slot so it stays under the pointer. */
  let movingOffset = $state(0);
  /** How far the new tab button is pushed right by a tab dragged past the last slot. */
  let addOffset = $state(0);
  /** This window's tab just dropped on the bar, easing from where it was drawn into its slot. */
  let settlingId = $state<string | null>(null);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  /** This window's tab dropped elsewhere, kept out of the strip while it moves to its new window. */
  let departingId = $state<string | null>(null);
  let departureTimer: ReturnType<typeof setTimeout> | undefined;
  /** Hover highlights stay off from a drag's start until the pointer moves after it, so tabs a drag slides under the pointer do not light up. */
  let hoverHeld = $state(false);
  /** The dragged tab no longer holds a slot in the strip. */
  const draggedTabLifted = $derived(draggingId !== null && (offBar || (outsideWindow && drag?.canTearOff === true)));
  /** The drag this window told the others about, until it ends. */
  let announcedDrag: TabWindowDrag | null = null;
  /** Another window's tab being dragged, drawn in this strip while over the bar and floating below it. */
  const incoming = $derived(tabs.incomingDrag);
  /** Slot another window's tab takes in this strip, or null while it is off the bar or outside this window. */
  let incomingIndex = $state<number | null>(null);
  /** How far another window's tab is drawn from its slot so it stays under the pointer. */
  let incomingOffset = $state(0);
  /** Another window's tab was dropped here and keeps its slot until it arrives. */
  let incomingDropped = $state(false);
  /** Another window's tab while it holds a slot in this strip. */
  const incomingTab = $derived(incoming && incomingIndex !== null ? incoming.tab : null);
  /** The tabs the strip draws: this window's, with another window's dragged tab in the slot it would take. */
  const shownTabs = $derived.by((): readonly WorkspaceTab[] => {
    if (!incomingTab || incomingIndex === null || tabs.tabs.some((tab) => tab.id === incomingTab.id)) return tabs.tabs;
    const shown = [...tabs.tabs];
    shown.splice(Math.min(incomingIndex, shown.length), 0, incomingTab);
    return shown;
  });
  /** A tab is dragged along this strip, so the others slide out of its way. */
  const stripDragActive = $derived(draggingId !== null || incomingTab !== null);
  const hoverQuiet = $derived(stripDragActive || hoverHeld);
  /** Tabs are dragged or still settling, so each draws its own background instead of the sliding indicator. */
  const stripInMotion = $derived(stripDragActive || settlingId !== null);
  /** The copy of a dragged tab that floats under the pointer once it is off the bar. */
  const floating = $derived.by((): { tab: WorkspaceTab; style: string } | null => {
    const at = pointer;
    const current = drag;
    if (!at || outsideWindow) return null;
    const placed = (grab: TabDragGrab) =>
      `left: ${at.x - grab.grabX}px; top: ${at.y - grab.grabY}px; width: ${grab.width}px;`;
    if (current) {
      const tab = offBar ? tabs.tabs.find((candidate) => candidate.id === current.id) : undefined;
      return tab ? { tab, style: placed(current) } : null;
    }
    return incoming && incomingIndex === null && !incomingDropped ? { tab: incoming.tab, style: placed(incoming) } : null;
  });
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
    // A tab kept off the strip on its way to another window has no slot to mark.
    if (!element || !parent || element.offsetParent === null || tabs.activeTabId === departingId) {
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
    // The indicator comes back once tabs stop moving, already under the active tab.
    if (!stripInMotion) updateIndicator();
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

  /** A tab opens as soon as it is pressed, as in a browser, so a tab dragged away is the one shown. */
  function handleMouseDown(event: MouseEvent, tab: WorkspaceTab): void {
    if (event.button === 1) event.preventDefault();
    const onClose = event.target instanceof Element && event.target.closest(".titlebar-tab-close") !== null;
    if (event.button === 0 && !onClose) tabs.activate(tab.id);
  }

  function closeFromButton(event: MouseEvent, tab: WorkspaceTab): void {
    event.stopPropagation();
    tabs.close(tab.id);
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
    const grab = { grabX: event.clientX - rect.left, grabY: event.clientY - rect.top, width: rect.width };
    transfer.effectAllowed = "move";
    transfer.setData(TAB_REORDER_MIME, tab.id);
    const payload = tabs.beginDrag(tab.id, grab);
    if (payload) transfer.setData(TAB_WINDOW_DRAG_MIME, tabWindowDragData(payload));
    announcedDrag = payload;
    // The tab itself is drawn under the pointer while it is over a window, so the platform drags an empty image.
    transfer.setDragImage(EMPTY_DRAG_IMAGE, 0, 0);
    drag = {
      id: tab.id,
      ...grab,
      canTearOff: payload !== null && tabs.detachBlock(tab.id) === null,
      origin: originOf(tab.id),
    };
    draggingId = tab.id;
    offBar = false;
    outsideWindow = false;
    pointer = { x: event.clientX, y: event.clientY };
    movingOffset = 0;
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
    return shownTabs.flatMap((tab) => {
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

  /**
   * Right end, in strip coordinates, of the track a dragged tab slides along: past the last slot through the
   * empty title bar, up to the buttons at its end, keeping room for the new tab button that the tab pushes along.
   */
  function trackRight(): number {
    if (!strip || !trackEnd) return tabStripRight(shownSlots());
    const button = addButton
      ? addButton.offsetLeft + addButton.offsetWidth - strip.offsetLeft - strip.offsetWidth
      : 0;
    return stripX(trackEnd.getBoundingClientRect().right) - button;
  }

  /** Whether a tab dragged to `clientX` would reach the buttons at the end of the bar, which takes it off the bar. */
  function passedTrack(clientX: number, grabX: number, width: number): boolean {
    return tabDragPassedTrack(stripX(clientX), grabX, width, trackRight());
  }

  /** Push the new tab button along with a tab dragged past the last slot, whose right edge is at `right`. */
  function pushAddButton(right: number): void {
    addOffset = Math.max(0, right - tabStripRight(shownSlots()));
    // A pushed button rides with the tab instead of sliding into place.
    if (addOffset > 0 && addButton) stopSliding(addButton);
  }

  function stopSliding(element: HTMLElement): void {
    for (const animation of element.getAnimations()) {
      if (animation.id === SLIDE_ANIMATION_ID) animation.cancel();
    }
  }

  /** Shown tabs other than `skip`, and the new tab button: everything that moves when the strip changes. */
  function slidingElements(skip: string | null): HTMLElement[] {
    const elements = shownTabs.flatMap((tab) => {
      const element = tabElements[tab.id];
      return tab.id !== skip && element && element.offsetParent !== null ? [element] : [];
    });
    return addButton ? [...elements, addButton] : elements;
  }

  /**
   * Apply `change` to the strip, then slide the tabs and the new tab button from where they were drawn into their
   * new places, so nothing jumps when a tab is passed, lifted off the bar, or brought onto it. Only sideways
   * movement is animated, so a tab's text never stretches. `skip` is the dragged tab, which follows the pointer.
   */
  function slideLayout(change: () => void, skip: string | null): void {
    const before = reducedMotion
      ? new Map<HTMLElement, number>()
      : new Map(slidingElements(skip).map((element) => [element, element.getBoundingClientRect().left]));
    change();
    flushSync();
    for (const element of slidingElements(skip)) {
      const from = before.get(element);
      if (from === undefined) continue;
      stopSliding(element);
      const offset = from - element.getBoundingClientRect().left;
      if (Math.abs(offset) < 0.5) continue;
      const animation = element.animate(
        [{ transform: `translateX(${offset}px)` }, { transform: "none" }],
        { duration: TAB_SLIDE_MS, easing: TAB_EASE },
      );
      animation.id = SLIDE_ANIMATION_ID;
    }
  }

  /**
   * Keep the dragged tab under the pointer along the strip. The tabs it passes trade places with it as soon as
   * it covers half of them, and it is then drawn shifted from its new slot so it never jumps.
   */
  function slideAlongStrip(current: OwnTabDrag, clientX: number): void {
    const slots = shownSlots();
    const left = tabStripVisualLeft(stripX(clientX), current.grabX, current.width, trackRight());
    const target = tabStripMoveTarget(
      slots.map((slot) => slot.id),
      current.id,
      tabStripDropIndex(slots, left + current.width / 2),
    );
    if (target) slideLayout(() => tabs.move(current.id, target.id, target.placement), current.id);
    const element = tabElements[current.id];
    movingOffset = element ? left - element.offsetLeft : 0;
    pushAddButton(left + current.width);
  }

  /** Slide the dragged tab along the bar under the pointer, and lift it off once pulled away from the bar. */
  function dragOwnTab(event: DragEvent, current: OwnTabDrag): void {
    acceptDrag(event);
    outsideWindow = false;
    pointer = { x: event.clientX, y: event.clientY };
    const leaving = current.canTearOff
      && (pulledOffBar(event.clientY) || passedTrack(event.clientX, current.grabX, current.width));
    // The strip copy goes or comes at once, at full width, while the tabs around it slide.
    if (leaving !== offBar) slideLayout(() => { offBar = leaving; }, current.id);
    if (leaving) addOffset = 0;
    else slideAlongStrip(current, event.clientX);
  }

  /** Position among this window's tabs that a tab entering the strip at `clientX` takes. */
  function entryIndex(clientX: number): number {
    const slots = shownSlots();
    const target = tabStripInsertTarget(slots.map((slot) => slot.id), tabStripInsertIndex(slots, stripX(clientX)));
    return tabStripInsertPosition(tabs.tabs.map((tab) => tab.id), target);
  }

  /**
   * Draw another window's tab under the pointer in this strip, taking a slot as it enters and then trading
   * places with the tabs it passes, just like a tab of this window.
   */
  function slideIncoming(current: TabWindowDragStarted, clientX: number): void {
    if (incomingIndex === null) {
      const entry = entryIndex(clientX);
      slideLayout(() => { incomingIndex = entry; }, current.tab.id);
    }
    const element = tabElements[current.tab.id];
    if (!element) return;
    const width = element.offsetWidth;
    const slots = shownSlots();
    const left = tabStripVisualLeft(stripX(clientX), Math.min(current.grabX, width), width, trackRight());
    const target = tabStripMoveTarget(
      slots.map((slot) => slot.id),
      current.tab.id,
      tabStripDropIndex(slots, left + width / 2),
    );
    if (target) {
      const next = tabStripInsertPosition(tabs.tabs.map((tab) => tab.id), target);
      slideLayout(() => { incomingIndex = next; }, current.tab.id);
    }
    incomingOffset = left - element.offsetLeft;
    pushAddButton(left + width);
  }

  /** Whether another window's tab dragged to the pointer of `event` is off this window's bar. */
  function foreignOffBar(event: DragEvent): boolean {
    if (pulledOffBar(event.clientY)) return true;
    if (!incoming) return false;
    const width = (incomingIndex === null ? undefined : tabElements[incoming.tab.id]?.offsetWidth) ?? incoming.width;
    return passedTrack(event.clientX, Math.min(incoming.grabX, width), width);
  }

  /** Draw another window's tab in the strip while it is over the bar, and floating under the pointer off it. */
  function dragForeignTab(event: DragEvent): void {
    if (incomingDropped) return;
    pointer = { x: event.clientX, y: event.clientY };
    if (foreignOffBar(event)) {
      leaveStrip();
      return;
    }
    acceptDrag(event);
    if (incoming) slideIncoming(incoming, event.clientX);
  }

  /**
   * Take another window's tab dropped on the bar into the slot it is drawn in, or, without a drawing, the
   * slot under the pointer. It keeps that slot until it arrives.
   */
  function dropForeignTab(event: DragEvent): void {
    if (foreignOffBar(event)) {
      resetIncoming();
      return;
    }
    acceptDrag(event);
    const ids = tabs.tabs.map((tab) => tab.id);
    const target = tabStripInsertTarget(ids, incomingIndex ?? entryIndex(event.clientX));
    const taken = tabs.receiveDrop(event.dataTransfer?.getData(TAB_WINDOW_DRAG_MIME) ?? "", target);
    if (!taken || !incomingTab) {
      resetIncoming();
      return;
    }
    incomingDropped = true;
    settle(incomingTab.id);
    incomingOffset = 0;
    addOffset = 0;
    pointer = null;
    dragDepth = 0;
  }

  /** Take another window's tab out of the strip once it is off the bar or outside the window, sliding the rest back. */
  function leaveStrip(): void {
    if (incomingIndex !== null) slideLayout(() => { incomingIndex = null; }, incoming?.tab.id ?? null);
    addOffset = 0;
  }

  /** Stop drawing another window's tab, once its drag ended or it arrived. */
  function resetIncoming(): void {
    incomingIndex = null;
    incomingOffset = 0;
    incomingDropped = false;
    if (drag) return;
    addOffset = 0;
    pointer = null;
    dragDepth = 0;
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

  /**
   * A drop on this window ends the drag here. Another window's tab moves in unless it was pulled off the bar,
   * and this window's tab, already in its slot, opens in a new window right away when it was pulled off.
   */
  function handleWindowDrop(event: DragEvent): void {
    const kind = dragKind(event);
    if (kind === "window") {
      dropForeignTab(event);
      return;
    }
    const current = drag;
    if (kind !== "reorder" || !current) return;
    acceptDrag(event);
    if (offBar) holdDeparture(current.id, tabs.detach(current.id, tearOffWindowPosition(event.screenX, event.screenY)));
    else settle(current.id);
    endDrag();
  }

  /** Ease a tab dropped on the bar from where it was drawn into its slot. */
  function settle(id: string): void {
    clearTimeout(settleTimer);
    settlingId = id;
    settleTimer = setTimeout(() => { settlingId = null; }, TAB_SLIDE_MS);
  }

  /**
   * Keep a tab that left by drag out of the strip until it is gone, so it never shows here again on its way
   * out. It comes back when its `move` fails, or when it has not gone in time.
   */
  function holdDeparture(id: string, move: Promise<void> | null): void {
    clearTimeout(departureTimer);
    departingId = id;
    const release = () => {
      clearTimeout(departureTimer);
      if (departingId === id) slideLayout(() => { departingId = null; }, null);
    };
    departureTimer = setTimeout(release, TAB_ARRIVAL_TIMEOUT_MS);
    void move?.catch((error: unknown) => {
      console.error("Failed to move tab to a new window:", error);
      release();
    });
  }

  /**
   * End a drag of this window's tab. A drag no window took opens the tab in a new window when it ended outside
   * this one, and otherwise, as when cancelled, puts the tab back where it started.
   */
  function handleDragEnd(event: DragEvent): void {
    const current = drag;
    const leftWindow = dragEndedOutsideWindow(event, { width: window.innerWidth, height: window.innerHeight }, dragDepth);
    if (announcedDrag) tabs.finishDrag(announcedDrag);
    announcedDrag = null;
    if (!current) {
      endDrag();
      return;
    }
    // A tab another window took is already off this strip, and stays off until it has moved there.
    if (event.dataTransfer?.dropEffect !== "none") {
      if (current.canTearOff) holdDeparture(current.id, null);
      endDrag();
      return;
    }
    if (leftWindow && current.canTearOff) {
      holdDeparture(current.id, tabs.tearOff(current.id, tearOffWindowPosition(event.screenX, event.screenY)));
      endDrag();
      return;
    }
    const origin = current.origin;
    slideLayout(() => {
      endDrag();
      if (origin && tabs.tabs.some((tab) => tab.id === origin.id)) tabs.move(current.id, origin.id, origin.placement);
    }, null);
  }

  /** Count tab drag entries and exits across the document, to tell when a drag left the window. */
  function trackDragDepth(event: DragEvent, change: 1 | -1): void {
    if (!dragKind(event)) return;
    dragDepth += change;
    const current = drag;
    if (current) {
      const outside = dragDepth <= 0;
      if (outside !== outsideWindow) slideLayout(() => { outsideWindow = outside; }, current.id);
      if (outside) addOffset = 0;
    } else if (dragDepth <= 0 && !incomingDropped) {
      leaveStrip();
      pointer = null;
    }
  }

  function endDrag(): void {
    drag = null;
    draggingId = null;
    offBar = false;
    outsideWindow = false;
    pointer = null;
    movingOffset = 0;
    addOffset = 0;
    dragDepth = 0;
  }

  $effect(() => () => {
    clearTimeout(settleTimer);
    clearTimeout(departureTimer);
  });

  $effect(() => {
    // The departing tab has gone to its new window.
    if (departingId !== null && !tabs.tabs.some((tab) => tab.id === departingId)) {
      untrack(() => {
        clearTimeout(departureTimer);
        departingId = null;
      });
    }
  });

  $effect(() => {
    if (stripDragActive) hoverHeld = true;
  });

  $effect(() => {
    // The platform sends no pointer moves during a drag, so the first one after it means the pointer moved again.
    if (!hoverHeld || stripDragActive) return;
    const release = () => { hoverHeld = false; };
    window.addEventListener("pointermove", release, { once: true });
    return () => window.removeEventListener("pointermove", release);
  });

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

  $effect(() => {
    // The drag ended, or its tab arrived and now holds the slot it was drawn in.
    if (!incoming) {
      untrack(resetIncoming);
      return;
    }
    // Another drag started over this window, which must not keep the previous one's slot.
    untrack(() => {
      if (!incomingDropped) incomingIndex = null;
    });
  });
</script>

<svelte:document
  ondragenter={(event) => trackDragDepth(event, 1)}
  ondragleave={(event) => trackDragDepth(event, -1)}
/>

{#snippet tabFace(tab: WorkspaceTab, iconOnly: boolean)}
  {@const Icon = workspaceTabKindIcon(tab.location.kind)}
  {@const project = workspaceTabProjectHint(tab)}
  <Icon class="shrink-0" size={ICON_SIZE} strokeWidth={ICON_STROKE_WIDTH} />
  {#if !iconOnly}
    <span class="titlebar-tab-label min-w-0 flex-1 truncate text-start">
      {workspaceTabLabel(tab, t)}{#if project}<span class="opacity-60"> {t("titleBar.tab.projectHint", shortWorkspaceTabProject(project))}</span>{/if}
    </span>
  {/if}
{/snippet}

<div
  class="flex min-w-0 shrink items-center gap-0.5 pl-1.5"
  style:--tab-slide={`${TAB_SLIDE_MS}ms`}
  style:--tab-ease={TAB_EASE}
>
  <div
    bind:this={strip}
    class={cn(
      "relative flex min-w-0 items-center gap-0.5",
      // A dragged tab may slide past the last slot, and stays drawn there until it settles.
      stripInMotion ? "overflow-visible" : "overflow-hidden",
    )}
    role="tablist"
    aria-orientation="horizontal"
  >
    {#if indicatorStyle && !stripInMotion}
      <div
        class="tab-indicator absolute top-0 h-full rounded-md bg-card shadow-(--tab-active-shadow)"
        style={indicatorStyle}
      ></div>
    {/if}
    {#each shownTabs as tab, index (tab.id)}
      {@const arriving = tab.id === incomingTab?.id}
      {@const active = !arriving && tab.id === tabs.activeTabId}
      {@const fullLabel = workspaceTabFullLabel(tab, t)}
      {@const moving = arriving || draggingId === tab.id}
      {@const lifted = !arriving && ((moving && draggedTabLifted) || departingId === tab.id)}
      {@const settling = settlingId === tab.id}
      <div
        bind:this={tabElements[tab.id]}
        data-active={active ? "" : undefined}
        class={cn(
          "titlebar-tab group relative z-1 flex items-center rounded-md text-sm font-medium",
          compact && "titlebar-tab-compact",
          activeOnly && !active && !moving ? "hidden" : "",
          // The highlight passes between the indicator and the tab itself at once, so it never fades in or out.
          active || moving
            ? "text-foreground dark:text-white"
            : cn("text-sidebar-foreground/70 transition-colors", !hoverQuiet && "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"),
          (active || moving) && stripInMotion && "bg-card shadow-(--tab-active-shadow)",
          moving && "titlebar-tab-moving",
          lifted && "titlebar-tab-lifted",
          settling && "titlebar-tab-settling",
        )}
        style:translate={moving && !lifted ? `${arriving ? incomingOffset : movingOffset}px 0` : undefined}
        draggable={!arriving}
        inert={arriving}
        role="presentation"
        ondragstart={(event) => handleDragStart(event, tab)}
        ondragend={handleDragEnd}
        oncontextmenu={(event) => onOpenContextMenu(event, tab.id)}
        onmousedown={(event) => handleMouseDown(event, tab)}
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
          {@render tabFace(tab, compact)}
          {#if !compact && tab.locked}<Lock class="titlebar-tab-lock shrink-0 opacity-60" size={11} strokeWidth={ICON_STROKE_WIDTH} aria-hidden="true" />{/if}
        </button>
        {#if !compact}
          <button
            type="button"
            tabindex="-1"
            class={cn(
              "titlebar-tab-close mr-1 flex shrink-0 items-center justify-center rounded-sm transition-opacity",
              !hoverQuiet && "hover:bg-sidebar-accent hover:opacity-100",
              active ? "opacity-70" : cn("opacity-0", !hoverQuiet && "group-hover:opacity-70"),
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
        "titlebar-add flex shrink-0 items-center justify-center rounded-md text-sidebar-foreground/70",
        !hoverQuiet && "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        addOffset > 0 && "titlebar-add-pushed",
        launcherOpen && "bg-sidebar-accent text-sidebar-accent-foreground",
      )}
      style:translate={addOffset > 0 ? `${addOffset}px 0` : undefined}
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

{#if floating}
  <div
    class={cn(
      "titlebar-tab-floating pointer-events-none fixed z-60 flex h-7 items-center rounded-md bg-card text-sm font-medium text-foreground shadow-(--tab-active-shadow) dark:text-white",
      compact ? "justify-center" : "gap-1.5 px-3",
    )}
    style={floating.style}
    aria-hidden="true"
  >
    {@render tabFace(floating.tab, compact)}
  </div>
{/if}

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

  /* The new tab button eases back to its place once a tab dragged past the last slot stops pushing it. */
  .titlebar-add {
    width: 28px;
    height: 28px;
    transition: color 150ms ease, background-color 150ms ease, translate var(--tab-slide) var(--tab-ease);
  }

  .titlebar-add.titlebar-add-pushed {
    transition: color 150ms ease, background-color 150ms ease;
  }

  .tab-indicator {
    transition: none;
  }

  /* The dragged tab rides above its neighbors, following the pointer without easing. */
  .titlebar-tab.titlebar-tab-moving {
    z-index: 2;
  }

  .titlebar-tab-moving .titlebar-tab-close {
    visibility: hidden;
  }

  /* Dropped on the bar, the tab eases from where it was drawn into its slot. */
  .titlebar-tab.titlebar-tab-settling {
    z-index: 2;
    transition: translate var(--tab-slide) var(--tab-ease);
  }

  /* Pulled off the bar, the tab gives its slot back and the strip closes around it. */
  .titlebar-tab.titlebar-tab-lifted {
    width: 0;
    min-width: 0;
    margin-right: -0.125rem;
    overflow: hidden;
    visibility: hidden;
  }

  @media (prefers-reduced-motion: reduce) {
    .titlebar-add,
    .titlebar-tab.titlebar-tab-settling {
      transition: none;
    }
  }

  .titlebar-tab-floating {
    max-width: 12.5rem;
  }

  :global(html[data-focus-intent="keyboard"]) .titlebar-tab-button:focus {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }
</style>
